mod conversation;

use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, State};

use crate::{ai, config, tts};
use conversation::Conversation;

pub const ASSISTANT_LABEL: &str = "assistant";

/// Logical window sizes (tauri.conf.json defines the window at INPUT size).
/// `show_input` scales these to physical pixels via the monitor scale factor.
const INPUT_SIZE: (u32, u32) = (420, 150);
/// Taller input popup when image thumbnails are shown, so the row doesn't
/// crowd the textarea.
const INPUT_WITH_ATTACHMENTS_SIZE: (u32, u32) = (420, 264);
/// Remind Mode adds the resolved-time chip under the textarea; without the
/// extra height the chip and the hint row crowd each other.
const INPUT_REMIND_SIZE: (u32, u32) = (420, 190);
const PANEL_SIZE: (u32, u32) = (460, 380);
/// Height of one label row above the textarea, measured as what the Remind Mode
/// label already costs. The follow-up chip occupies that same row.
const LABEL_ROW_HEIGHT: u32 = INPUT_REMIND_SIZE.1 - INPUT_SIZE.1;
/// Logical gap between the cursor and the window's near corner.
const CURSOR_OFFSET: i32 = 14;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    #[default]
    Idle,
    Input,
    Loading,
    Answering,
    Speaking,
    Error,
}

/// Shared, race-safe assistant state. `generation` is bumped whenever a new
/// question starts or the panel closes; async flows compare against their
/// captured generation and drop stale results (answers, speech chunks).
#[derive(Default)]
pub struct AssistantState {
    pub phase: Mutex<Phase>,
    pub generation: AtomicU64,
    pub last_question: Mutex<Option<String>>,
    pub last_answer: Mutex<Option<String>>,
    /// True while a native file-picker dialog is open. The Input-phase
    /// blur-dismiss (lib.rs) honors this so picking an image doesn't close
    /// the panel when the dialog steals focus.
    pub dialog_open: AtomicBool,
    /// Last logical size the user dragged the answer panel to, remembered for
    /// the rest of the session so follow-up questions reopen at that size.
    /// `None` until the user resizes; cleared naturally on app restart.
    pub panel_size: Mutex<Option<(u32, u32)>>,
    /// The live quick-chat session, or `None` when no panel is open. Memory
    /// only — never persisted, and `close()` is the single place it is dropped,
    /// which is what makes "closing ends the conversation" true for Esc, the X
    /// button, blur-dismiss, the auto-close timer and the shortcut alike.
    pub conversation: Mutex<Option<Conversation>>,
}

/// Top-left position for the assistant window, anchored near the cursor and
/// clamped to the monitor work area. Default is below-right of the cursor;
/// flips left when it would overflow the right edge and above when it would
/// overflow the bottom edge. All values are physical pixels.
pub fn anchor_at_cursor(
    cursor: (i32, i32),
    work_pos: (i32, i32),
    work_size: (u32, u32),
    window: (u32, u32),
    offset: i32,
) -> (i32, i32) {
    let (cx, cy) = cursor;
    let (wx, wy) = work_pos;
    let (ww, wh) = (work_size.0 as i32, work_size.1 as i32);
    let (win_w, win_h) = (window.0 as i32, window.1 as i32);

    // Prefer right of the cursor; flip to the left if it overflows the right edge.
    let mut x = cx + offset;
    if x + win_w > wx + ww {
        x = cx - offset - win_w;
    }
    // Prefer below the cursor; flip above if it overflows the bottom edge.
    let mut y = cy + offset;
    if y + win_h > wy + wh {
        y = cy - offset - win_h;
    }
    // Clamp into the work area. Upper bound never drops below the origin, so a
    // window larger than the work area pins to the top-left without panicking.
    x = x.clamp(wx, (wx + ww - win_w).max(wx));
    y = y.clamp(wy, (wy + wh - win_h).max(wy));
    (x, y)
}

fn set_phase(app: &AppHandle, phase: Phase) {
    *app.state::<AssistantState>().phase.lock().unwrap() = phase;
}

/// Bump the generation counter and return the new value.
fn bump_generation(app: &AppHandle) -> u64 {
    app.state::<AssistantState>()
        .generation
        .fetch_add(1, Ordering::SeqCst)
        + 1
}

fn current_generation(app: &AppHandle) -> u64 {
    app.state::<AssistantState>().generation.load(Ordering::SeqCst)
}

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct AssistantEvent {
    phase: &'static str,
    question: Option<String>,
    answer: Option<String>,
    message: Option<String>,
    tts_error: Option<String>,
    /// Set when the wake word opened the box: the frontend starts the
    /// microphone straight away so the flow stays hands-free.
    voice: bool,
    /// Exchanges already in this session. 0 = a fresh conversation; the ask box
    /// shows its follow-up chip above that. Stamped by `publish`, so it can
    /// never disagree with the stored session.
    turn_count: usize,
}

/// The common shape — a phase plus the question/answer it applies to. Anything
/// rarer is spread over this: `AssistantEvent { voice: true, ..event(..) }`.
fn event(
    phase: &'static str,
    question: Option<String>,
    answer: Option<String>,
) -> AssistantEvent {
    AssistantEvent { phase, question, answer, ..Default::default() }
}

/// Single choke point for state fan-out to the assistant webview. Also
/// reconciles the wake listener, which must not hold the microphone while the
/// panel is open — nor hear Claudy speaking its own answer.
fn publish(app: &AppHandle, mut event: AssistantEvent) {
    // Stamped here rather than at each call site: `publish` is already the one
    // choke point, so the count can never drift from the stored session.
    event.turn_count = turn_count(app);
    let _ = app.emit("assistant-state", event);
    crate::wake::reconcile(app);
}

/// Exchanges recorded in the live session, or 0 when none is open.
fn turn_count(app: &AppHandle) -> usize {
    app.state::<AssistantState>()
        .conversation
        .lock()
        .unwrap()
        .as_ref()
        .map_or(0, Conversation::turn_count)
}

/// Resize the assistant window to a logical size, converting to physical via
/// the window's current scale factor. Keeps the top-left position anchored.
fn resize_to(app: &AppHandle, size: (u32, u32)) {
    if let Some(w) = app.get_webview_window(ASSISTANT_LABEL) {
        let scale = w.scale_factor().unwrap_or(1.0);
        let phys = PhysicalSize::new(
            (size.0 as f64 * scale).round() as u32,
            (size.1 as f64 * scale).round() as u32,
        );
        let _ = w.set_size(phys);
    }
}

/// The answer-panel size to open at: the user's remembered dragged size, or the
/// default `PANEL_SIZE` if they haven't resized this session.
fn remembered_panel_size(app: &AppHandle) -> (u32, u32) {
    app.state::<AssistantState>()
        .panel_size
        .lock()
        .unwrap()
        .unwrap_or(PANEL_SIZE)
}

/// Snapshot the answer panel's current on-screen size (converted to logical px)
/// so the next answer reopens at it. Called right before we leave the answer
/// view — a native resize drag doesn't reliably emit a `Resized` event on
/// Windows, so we read the live window size on demand instead of tracking drags.
/// No-ops unless we're currently showing an answer, so it never captures the
/// input/loading size.
fn capture_panel_size(app: &AppHandle) {
    let phase = *app.state::<AssistantState>().phase.lock().unwrap();
    if !matches!(phase, Phase::Answering | Phase::Speaking | Phase::Error) {
        return;
    }
    if let Some(w) = app.get_webview_window(ASSISTANT_LABEL) {
        if let Ok(size) = w.inner_size() {
            let scale = w.scale_factor().unwrap_or(1.0);
            let logical = (
                (size.width as f64 / scale).round() as u32,
                (size.height as f64 / scale).round() as u32,
            );
            *app.state::<AssistantState>().panel_size.lock().unwrap() = Some(logical);
        }
    }
}

/// THE entry point — global shortcut and the toggle command both call this.
/// Hidden → show the input popup; visible → close.
pub fn toggle(app: &AppHandle) {
    let visible = app
        .get_webview_window(ASSISTANT_LABEL)
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false);
    if visible {
        close(app);
    } else if let Err(e) = show_input(app) {
        eprintln!("assistant show failed: {e}");
    }
}

/// Position the input popup at the cursor and show it focused.
pub fn show_input(app: &AppHandle) -> Result<(), String> {
    show_input_with(app, false)
}

/// Same, but the frontend starts recording immediately — the wake word's
/// entry point, where the user's hands are busy by definition.
pub fn show_input_voice(app: &AppHandle) -> Result<(), String> {
    show_input_with(app, true)
}

fn show_input_with(app: &AppHandle, voice: bool) -> Result<(), String> {
    let w = app
        .get_webview_window(ASSISTANT_LABEL)
        .ok_or("assistant window missing")?;
    let cursor = app.cursor_position().map_err(|e| e.to_string())?;
    let monitor = app
        .monitor_from_point(cursor.x, cursor.y)
        .ok()
        .flatten()
        .or_else(|| app.primary_monitor().ok().flatten())
        .ok_or("no monitor found")?;
    let area = monitor.work_area();
    let scale = monitor.scale_factor();

    let win = (
        (INPUT_SIZE.0 as f64 * scale).round() as u32,
        (INPUT_SIZE.1 as f64 * scale).round() as u32,
    );
    let (x, y) = anchor_at_cursor(
        (cursor.x.round() as i32, cursor.y.round() as i32),
        (area.position.x, area.position.y),
        (area.size.width, area.size.height),
        win,
        (CURSOR_OFFSET as f64 * scale).round() as i32,
    );

    // Opening a fresh input drops any stale in-flight question/answer, and
    // clears any lingering file-dialog flag from a prior session.
    bump_generation(app);
    app.state::<AssistantState>().dialog_open.store(false, Ordering::SeqCst);
    // A fresh conversation per open. Unconditional rather than "only when
    // None": the window was hidden, so `close()` has already cleared it — and
    // if that invariant ever broke, starting clean is the safe failure, never
    // resurrecting a conversation the user believed was over.
    *app.state::<AssistantState>().conversation.lock().unwrap() =
        Some(Conversation::new(chrono::Local::now()));
    w.set_size(PhysicalSize::new(win.0, win.1))
        .map_err(|e| e.to_string())?;
    w.set_position(PhysicalPosition::new(x, y))
        .map_err(|e| e.to_string())?;
    set_phase(app, Phase::Input);
    w.show().map_err(|e| e.to_string())?;
    w.set_focus().map_err(|e| e.to_string())?;
    publish(app, AssistantEvent { voice, ..event("input", None, None) });
    Ok(())
}

/// Ask the AI. Publishes `loading`, then `answering`/`error`. A newer question
/// (or a close) bumps the generation and this flow's result is discarded.
pub fn ask(app: &AppHandle, question: String, images: Vec<ai::ImageAttachment>) {
    let question = question.trim().to_string();
    if question.is_empty() && images.is_empty() {
        return;
    }
    // Backstop the frontend's warn-and-block: never ship images to a model that
    // can't read them (bump generation so any in-flight flow is superseded).
    if !images.is_empty() && !ai::active_provider_supports_images(app.clone()).unwrap_or(false) {
        bump_generation(app);
        set_phase(app, Phase::Error);
        publish(
            app,
            AssistantEvent {
                message: Some(
                    "The current model can't read images — remove them or switch to a \
                     vision-capable provider in Settings."
                        .to_string(),
                ),
                ..event("error", Some(question), None)
            },
        );
        return;
    }
    let generation = bump_generation(app);
    *app.state::<AssistantState>().last_question.lock().unwrap() = Some(question.clone());
    set_phase(app, Phase::Loading);
    resize_to(app, remembered_panel_size(app));
    publish(app, event("loading", Some(question.clone()), None));

    // Web search only when the user opted in AND the active provider offers a
    // native tool; otherwise it silently no-ops (ollama/openai_compatible).
    let settings = config::load(app).ok();
    let web_search = settings
        .as_ref()
        .map(|s| s.assistant.auto_web_search)
        .unwrap_or(false)
        && ai::active_provider_supports_web_search(app).unwrap_or(false);
    // Custom system prompt (None when unset) — assistant path only; the
    // dictation flow builds default options and stays system-free.
    let user_system = settings.as_ref().and_then(|s| s.assistant.system_prompt());
    // One lock, one pass, released before any await: refresh the session's copy
    // of the system prompt (so a Settings edit applies mid-session), then take
    // the budgeted replay of earlier turns.
    let had_images = !images.is_empty();
    let (history, system, session_id) = {
        let state = app.state::<AssistantState>();
        let mut guard = state.conversation.lock().unwrap();
        let conv = guard.get_or_insert_with(|| Conversation::new(chrono::Local::now()));
        conv.system = user_system;
        (conv.history(), conv.request_system(web_search), conv.id.clone())
    };
    let opts = ai::RequestOptions { web_search, system, images, history };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let result =
            ai::complete_with_options(&app, &question, opts, ai::UsageSource::Assistant).await;
        if current_generation(&app) != generation {
            return; // superseded by a newer question or a close
        }
        match result {
            Ok(answer) => {
                *app.state::<AssistantState>().last_answer.lock().unwrap() = Some(answer.clone());
                // Record before publishing: `publish` stamps the turn count the
                // ask box shows, and it must already include this exchange.
                record_turn(&app, &session_id, &question, &answer, web_search, had_images);
                set_phase(&app, Phase::Answering);
                publish(
                    &app,
                    event("answering", Some(question.clone()), Some(answer.clone())),
                );
                // Dispatched before `maybe_speak`, which blocks for the whole
                // playback — the fold has no reason to wait for speech.
                spawn_fold_if_needed(&app, session_id);
                maybe_speak(&app, generation, question, answer).await;
            }
            Err(message) => {
                set_phase(&app, Phase::Error);
                publish(
                    &app,
                    AssistantEvent {
                        message: Some(message),
                        ..event("error", Some(question), None)
                    },
                );
            }
        }
    });
}

/// Append a completed exchange to the live session.
///
/// Silently no-ops when the session was closed or replaced while the answer was
/// in flight — the generation guard catches most of that, but after a close and
/// a reopen the session id is the thing that actually distinguishes them.
fn record_turn(
    app: &AppHandle,
    session_id: &str,
    question: &str,
    answer: &str,
    web_search: bool,
    had_images: bool,
) {
    let state = app.state::<AssistantState>();
    let mut guard = state.conversation.lock().unwrap();
    if let Some(conv) = guard.as_mut().filter(|c| c.id == session_id) {
        conv.record_turn(
            question.to_string(),
            answer.to_string(),
            web_search,
            had_images,
            chrono::Local::now(),
        );
    }
}

/// Fold older exchanges into a compact summary once the session outgrows the
/// verbatim window.
///
/// Runs in the background AFTER the answer is on screen, so it never costs the
/// user latency, and with `RequestOptions::default()` so the user's own system
/// prompt ("answer as a pirate") can't corrupt the summary. The result is
/// discarded unless the session id still matches.
fn spawn_fold_if_needed(app: &AppHandle, session_id: String) {
    let request = {
        let state = app.state::<AssistantState>();
        let mut guard = state.conversation.lock().unwrap();
        guard
            .as_mut()
            .filter(|c| c.id == session_id)
            .and_then(conversation::Conversation::take_fold_request)
    };
    let Some(request) = request else { return };

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = ai::complete_with_options(
            &app,
            &request.prompt,
            ai::RequestOptions::default(),
            ai::UsageSource::AssistantSummary,
        )
        .await;
        let state = app.state::<AssistantState>();
        let mut guard = state.conversation.lock().unwrap();
        let Some(conv) = guard.as_mut().filter(|c| c.id == session_id) else {
            return; // session closed while the summarizer ran
        };
        match result {
            Ok(summary) => {
                conv.apply_fold(request.boundary, summary);
            }
            // A failed fold is not a failed session: the next answer simply
            // tries again, and selection hard-trims meanwhile.
            Err(_) => conv.fold_failed(),
        }
    });
}

/// Speak the answer if auto-speak is on and the voice model is available. TTS
/// failure is non-fatal: the answer stays on screen with an inline note. A
/// newer question (generation bump) aborts speech between chunks.
async fn maybe_speak(app: &AppHandle, generation: u64, question: String, answer: String) {
    let settings = match config::load(app) {
        Ok(s) => s,
        Err(_) => return,
    };
    if !settings.assistant.auto_speak {
        return;
    }
    if !tts::assets_downloaded(app) {
        tts_note(app, question, answer, "Voice model not downloaded — enable it in Settings");
        return;
    }

    let tts_state = app.state::<tts::TtsState>();
    let engine = match tts_state.get_or_load_engine(app).await {
        Ok(e) => e,
        Err(e) => {
            tts_note(app, question, answer, &e);
            return;
        }
    };

    let chunks = tts::chunk_text(&tts::speakable_text(&answer));
    if chunks.is_empty() {
        return;
    }
    tts_state.playback.set_volume(settings.assistant.volume);
    let voice = settings.assistant.tts_voice;
    let speed = settings.assistant.speech_speed;

    let mut first = true;
    let mut collected: Vec<f32> = Vec::new();
    let mut rate = tts::kokoro::SAMPLE_RATE;
    for chunk in chunks {
        if current_generation(app) != generation {
            return; // superseded by a newer question / close
        }
        let audio = match engine.synth(&chunk, &voice, speed).await {
            Ok(a) => a,
            Err(e) => {
                tts_note(app, question, answer, &e);
                return;
            }
        };
        if current_generation(app) != generation {
            return;
        }
        rate = audio.sample_rate;
        collected.extend_from_slice(&audio.samples);
        if let Err(e) = tts_state.playback.enqueue(audio.samples, audio.sample_rate) {
            tts_note(app, question, answer, &e);
            return;
        }
        if first {
            set_phase(app, Phase::Speaking);
            publish(
                app,
                event("speaking", Some(question.clone()), Some(answer.clone())),
            );
            first = false;
        }
    }
    *tts_state.last_audio.lock().unwrap() = Some(tts::TtsAudio { samples: collected, sample_rate: rate });

    // Once playback drains, return to the answering phase so the auto-close
    // timer resumes; abort if a newer question superseded us.
    wait_for_drain(app, generation).await;
    if current_generation(app) == generation {
        set_phase(app, Phase::Answering);
        publish(app, event("answering", Some(question), Some(answer)));
    }
}

/// Non-fatal TTS problem: keep the answer, add an inline note.
fn tts_note(app: &AppHandle, question: String, answer: String, note: &str) {
    set_phase(app, Phase::Answering);
    publish(
        app,
        AssistantEvent {
            tts_error: Some(note.to_string()),
            ..event("answering", Some(question), Some(answer))
        },
    );
}

/// Poll until playback finishes or the generation changes.
async fn wait_for_drain(app: &AppHandle, generation: u64) {
    let poll = std::time::Duration::from_millis(120);
    loop {
        if current_generation(app) != generation {
            return;
        }
        if !app.state::<tts::TtsState>().playback.is_playing() {
            return;
        }
        tokio::time::sleep(poll).await;
    }
}

/// Close the panel: drop stale flows, stop speech, hide, reset to idle.
///
/// THE end of a quick-chat session. Every hide path funnels through here — Esc,
/// the X button, blur-while-typing, the auto-close timer, re-pressing the
/// shortcut — so "closing ends the conversation" needs no other rule. Dropping
/// the struct is the whole privacy story: it was never written anywhere else.
pub fn close(app: &AppHandle) {
    capture_panel_size(app);
    bump_generation(app);
    app.state::<tts::TtsState>().playback.stop();
    {
        let state = app.state::<AssistantState>();
        *state.conversation.lock().unwrap() = None;
        // Only read by stop/replay_assistant_speech, both unreachable with the
        // window hidden — no reason to keep the last exchange in memory.
        *state.last_question.lock().unwrap() = None;
        *state.last_answer.lock().unwrap() = None;
    }
    if let Some(w) = app.get_webview_window(ASSISTANT_LABEL) {
        let _ = w.hide();
    }
    set_phase(app, Phase::Idle);
    publish(app, event("idle", None, None));
}

#[tauri::command]
pub fn ask_assistant(app: AppHandle, question: String, images: Vec<ai::ImageAttachment>) {
    ask(&app, question, images);
}

#[tauri::command]
pub fn close_assistant(app: AppHandle) {
    close(&app);
}

/// Logical size the input popup needs for its current contents. Pure so the
/// arithmetic is testable without a window.
///
/// Remind Mode wins: it hides attachments, so those two are never both true.
/// It also owns the label row, so the follow-up chip never stacks on top of it
/// — the frontend hides the chip in Remind Mode for the same reason.
pub fn input_size(has_attachments: bool, remind_mode: bool, follow_up: bool) -> (u32, u32) {
    let base = match (remind_mode, has_attachments) {
        (true, _) => INPUT_REMIND_SIZE,
        (false, true) => INPUT_WITH_ATTACHMENTS_SIZE,
        (false, false) => INPUT_SIZE,
    };
    let extra = if follow_up && !remind_mode { LABEL_ROW_HEIGHT } else { 0 };
    (base.0, base.1 + extra)
}

/// Grow/shrink the input popup to fit its current contents. The frontend calls
/// this when the attachment count crosses zero, the mode toggles, or the
/// session gains its first exchange.
#[tauri::command]
pub fn resize_assistant_input(
    app: AppHandle,
    has_attachments: bool,
    remind_mode: bool,
    follow_up: bool,
) {
    resize_to(&app, input_size(has_attachments, remind_mode, follow_up));
}

/// Toggle the flag that suppresses the Input-phase blur-dismiss while a native
/// file-picker dialog is open.
#[tauri::command]
pub fn set_assistant_dialog_open(app: AppHandle, open: bool) {
    app.state::<AssistantState>().dialog_open.store(open, Ordering::SeqCst);
}

/// Return to the input phase for a follow-up question, resizing back down and
/// refocusing the textarea.
///
/// Deliberately does NOT touch `conversation`: this is "ask another thing in
/// the same chat", and the whole point is that the next question resolves "it"
/// against the answer still on screen. `bump_generation` here is cancellation,
/// not context reset. Only `close()` ends a session.
#[tauri::command]
pub fn assistant_new_question(app: AppHandle) -> Result<(), String> {
    capture_panel_size(&app);
    bump_generation(&app);
    app.state::<tts::TtsState>().playback.stop();
    // Size for the follow-up chip up front, or the window flashes at the bare
    // input height before the frontend's resize effect corrects it.
    resize_to(&app, input_size(false, false, turn_count(&app) > 0));
    if let Some(w) = app.get_webview_window(ASSISTANT_LABEL) {
        let _ = w.set_focus();
    }
    set_phase(&app, Phase::Input);
    publish(&app, event("input", None, None));
    Ok(())
}

/// Stop speaking and return to the answer view.
#[tauri::command]
pub fn stop_assistant_speech(app: AppHandle) -> Result<(), String> {
    app.state::<tts::TtsState>().playback.stop();
    let assistant = app.state::<AssistantState>();
    let q = assistant.last_question.lock().unwrap().clone();
    let a = assistant.last_answer.lock().unwrap().clone();
    if *assistant.phase.lock().unwrap() == Phase::Speaking {
        set_phase(&app, Phase::Answering);
        publish(&app, event("answering", q, a));
    }
    Ok(())
}

/// Replay the last synthesized audio without re-running synthesis.
#[tauri::command]
pub fn replay_assistant_speech(app: AppHandle) -> Result<(), String> {
    let tts_state = app.state::<tts::TtsState>();
    let audio = tts_state.last_audio.lock().unwrap().clone();
    let audio = audio.ok_or("Nothing to replay yet")?;
    tts_state.playback.stop();
    tts_state.playback.enqueue(audio.samples, audio.sample_rate)?;

    let generation = current_generation(&app);
    let assistant = app.state::<AssistantState>();
    let q = assistant.last_question.lock().unwrap().clone();
    let a = assistant.last_answer.lock().unwrap().clone();
    set_phase(&app, Phase::Speaking);
    publish(&app, event("speaking", q.clone(), a.clone()));

    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        wait_for_drain(&app2, generation).await;
        if current_generation(&app2) == generation {
            set_phase(&app2, Phase::Answering);
            publish(&app2, event("answering", q, a));
        }
    });
    Ok(())
}

#[tauri::command]
pub fn get_assistant_state(state: State<'_, AssistantState>) -> Result<Phase, String> {
    Ok(*state.phase.lock().map_err(|e| e.to_string())?)
}

/// Exchanges in the live session, for a frontend that mounted mid-session (a
/// webview reload) and has no event to learn the count from yet.
#[tauri::command]
pub fn get_assistant_turn_count(state: State<'_, AssistantState>) -> Result<usize, String> {
    Ok(state
        .conversation
        .lock()
        .map_err(|e| e.to_string())?
        .as_ref()
        .map_or(0, Conversation::turn_count))
}

#[cfg(test)]
mod tests {
    use super::*;

    // 1920x1040 work area at origin, cursor mid-screen, 420x150 window.
    #[test]
    fn places_below_right_of_cursor_by_default() {
        let (x, y) = anchor_at_cursor((600, 400), (0, 0), (1920, 1040), (420, 150), 14);
        assert_eq!(x, 600 + 14);
        assert_eq!(y, 400 + 14);
    }

    #[test]
    fn flips_left_of_cursor_near_the_right_edge() {
        // Cursor near the right edge: below-right would overflow, so flip left.
        let (x, y) = anchor_at_cursor((1900, 400), (0, 0), (1920, 1040), (420, 150), 14);
        assert_eq!(x, 1900 - 14 - 420);
        assert_eq!(y, 400 + 14); // vertical still below
    }

    #[test]
    fn flips_above_cursor_near_the_bottom_edge() {
        let (x, y) = anchor_at_cursor((600, 1030), (0, 0), (1920, 1040), (420, 150), 14);
        assert_eq!(x, 600 + 14); // horizontal still right
        assert_eq!(y, 1030 - 14 - 150);
    }

    #[test]
    fn clamps_into_a_negative_origin_monitor() {
        // Secondary monitor to the left of primary; cursor at its top-left.
        let (x, y) = anchor_at_cursor((-2550, 10), (-2560, 0), (2560, 1400), (420, 150), 14);
        assert!(x >= -2560, "x must stay within the monitor: {x}");
        assert!(y >= 0, "y must stay within the monitor: {y}");
    }

    #[test]
    fn window_larger_than_work_area_pins_to_top_left_without_panicking() {
        let (x, y) = anchor_at_cursor((100, 100), (0, 0), (300, 200), (420, 500), 14);
        assert_eq!(x, 0);
        assert_eq!(y, 0);
    }

    #[test]
    fn input_size_matches_the_legacy_sizes_without_a_follow_up() {
        // Pure regression pin: nothing about the pre-session popup may move.
        assert_eq!(input_size(false, false, false), INPUT_SIZE);
        assert_eq!(input_size(true, false, false), INPUT_WITH_ATTACHMENTS_SIZE);
        assert_eq!(input_size(false, true, false), INPUT_REMIND_SIZE);
    }

    #[test]
    fn input_size_adds_one_row_for_the_follow_up_chip() {
        assert_eq!(input_size(false, false, true).1, INPUT_SIZE.1 + LABEL_ROW_HEIGHT);
        assert_eq!(
            input_size(true, false, true).1,
            INPUT_WITH_ATTACHMENTS_SIZE.1 + LABEL_ROW_HEIGHT
        );
        assert_eq!(input_size(false, false, true).0, INPUT_SIZE.0, "width never changes");
    }

    #[test]
    fn input_size_does_not_stack_the_follow_up_row_on_remind_mode() {
        // Remind Mode already owns the label row, and the frontend hides the
        // chip there — the two must never both claim height.
        assert_eq!(input_size(false, true, true), INPUT_REMIND_SIZE);
        assert_eq!(input_size(true, true, true), INPUT_REMIND_SIZE);
    }

    #[test]
    fn default_phase_is_idle_and_serializes_lowercase() {
        assert_eq!(Phase::default(), Phase::Idle);
        assert_eq!(serde_json::to_string(&Phase::Answering).unwrap(), "\"answering\"");
        assert_eq!(serde_json::to_string(&Phase::Speaking).unwrap(), "\"speaking\"");
    }
}
