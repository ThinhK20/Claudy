use serde::{Deserialize, Deserializer, Serialize};
use std::sync::Mutex;
use std::time::Instant;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::{audio, config, inject, overlay, stt, tray};

/// How long a transient error stays on the overlay before it hides.
const ERROR_FLASH_MS: u64 = 1800;

/// A press+release shorter than this is a TAP, not a hold: the capture
/// latches and the next press ends it. Guards against the near-empty
/// recordings you get when the mic is still spinning up (`audio::start`
/// blocks while the stream opens).
pub const TAP_MS: u64 = 350;

/// Shown on the pill while a tapped (latched) capture is running — without
/// it "Recording…" gives no clue that a second press is what ends it.
const LATCHED_HINT: &str = "press again to stop";

/// How the dictation shortcut activates.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DictationMode {
    /// Hold to talk, release to transcribe. A tap falls back to `Toggle`.
    #[default]
    Hold,
    /// Press to start, press again to stop.
    Toggle,
}

// Hand-written so an unknown string (or a non-string) falls back to the
// default instead of failing the WHOLE settings load — settings.json is
// hand-editable, and one typo here must not brick every other setting.
impl<'de> Deserialize<'de> for DictationMode {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(match String::deserialize(d).unwrap_or_default().as_str() {
            "toggle" => Self::Toggle,
            _ => Self::Hold,
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    #[default]
    Idle,
    Recording,
    Transcribing,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    StartRecording,
    StopAndTranscribe,
    Ignore,
}

/// Pure transition: phase to publish + side effect to run.
/// A toggle during Transcribing is a no-op (prevents re-entrant flows).
pub fn on_toggle(current: Phase) -> (Phase, Action) {
    match current {
        Phase::Idle => (Phase::Recording, Action::StartRecording),
        Phase::Recording => (Phase::Transcribing, Action::StopAndTranscribe),
        Phase::Transcribing => (Phase::Transcribing, Action::Ignore),
    }
}

/// Pure: a key-down is identical in both modes — idle starts, recording
/// stops (in Hold mode that ends a latched tap), transcribing is ignored.
pub fn on_press(_mode: DictationMode, current: Phase) -> (Phase, Action) {
    on_toggle(current)
}

/// Pure: a key-up only ever ENDS a Hold-mode recording. Toggle mode, a
/// sub-`TAP_MS` tap, and every non-Recording phase are no-ops — notably
/// Idle, where a failed `start_flow` leaves us, so a release can never
/// start a recording of its own.
pub fn on_release(mode: DictationMode, current: Phase, held_ms: u64) -> (Phase, Action) {
    if mode == DictationMode::Toggle || current != Phase::Recording || held_ms < TAP_MS {
        return (current, Action::Ignore);
    }
    (Phase::Transcribing, Action::StopAndTranscribe)
}

/// Pure: is the running capture latched — i.e. started by a tap, with the
/// combo already back up, so only a second press can end it? `pressed_at`
/// is Some only while the combo is physically down.
pub fn is_latched(mode: DictationMode, phase: Phase, pressed_at: Option<Instant>) -> bool {
    mode == DictationMode::Hold && phase == Phase::Recording && pressed_at.is_none()
}

/// Whisper emits bracketed placeholders ("[BLANK_AUDIO]", "(wind blowing)")
/// for non-speech audio — injecting those would be worse than doing nothing.
pub fn is_effectively_empty(text: &str) -> bool {
    let t = text.trim();
    t.is_empty()
        || (t.starts_with('[') && t.ends_with(']'))
        || (t.starts_with('(') && t.ends_with(')'))
}

#[derive(Default)]
pub struct DictationState {
    /// Instant, race-safe toggle decisions (locked for nanoseconds only).
    pub phase: Mutex<Phase>,
    /// When the dictation combo went down, so a release can tell a tap from
    /// a hold. Some ONLY while the combo is physically down.
    pressed_at: Mutex<Option<Instant>>,
    /// Serializes the spawned start/stop flows so a stop queued behind a
    /// still-starting capture waits instead of racing it. tokio's Mutex is
    /// FIFO-fair, so flows run in press order.
    op: tokio::sync::Mutex<()>,
}

/// THE single dispatcher — every entry point below funnels through here.
/// Runs on the caller's thread (often the main event loop): it only flips
/// the phase mutex and spawns, never blocks.
fn apply(app: &AppHandle, decide: impl FnOnce(Phase) -> (Phase, Action)) {
    let action = {
        let state = app.state::<DictationState>();
        let mut phase = state.phase.lock().unwrap();
        let (next, action) = decide(*phase);
        *phase = next;
        action
    };
    let app = app.clone();
    match action {
        Action::StartRecording => {
            tauri::async_runtime::spawn(async move { start_flow(app).await });
        }
        Action::StopAndTranscribe => {
            tauri::async_runtime::spawn(async move { stop_flow(app).await });
        }
        Action::Ignore => {}
    }
}

/// The active activation mode, read at press/release time rather than at
/// registration — so switching it in Settings applies without re-binding
/// the combo or restarting.
fn mode_of(app: &AppHandle) -> DictationMode {
    config::load(app).map(|s| s.dictation_mode).unwrap_or_default()
}

/// Mode-independent toggle: the tray menu item and the `toggle_dictation`
/// command call this (there is no key to release behind either).
pub fn toggle(app: &AppHandle) {
    apply(app, on_toggle);
}

/// Dictation combo went DOWN.
pub fn press(app: &AppHandle) {
    let mode = mode_of(app);
    *app.state::<DictationState>().pressed_at.lock().unwrap() = Some(Instant::now());
    apply(app, |p| on_press(mode, p));
}

/// Dictation combo came UP.
pub fn release(app: &AppHandle) {
    let mode = mode_of(app);
    // A release with no recorded press counts as 0ms — i.e. a tap — so a
    // stray release can never truncate a running capture.
    let held_ms = app
        .state::<DictationState>()
        .pressed_at
        .lock()
        .unwrap()
        .take()
        .map(|t| t.elapsed().as_millis() as u64)
        .unwrap_or(0);
    apply(app, |p| on_release(mode, p, held_ms));

    // Only a tap needs anything published here: it leaves the capture
    // running with the combo already up, and nothing else would say so.
    // `pressed_at` is None by construction — we just took it above.
    let phase = *app.state::<DictationState>().phase.lock().unwrap();
    if is_latched(mode, phase, None) {
        publish_recording(app);
    }
}

async fn start_flow(app: AppHandle) {
    let dict = app.state::<DictationState>();
    let _op = dict.op.lock().await;

    let settings = match config::load(&app) {
        Ok(s) => s,
        Err(e) => {
            reset_idle(&app, &dict);
            crate::notify::send(&app, true, &format!("Could not load settings: {e}"));
            return;
        }
    };

    // Preflight: fail BEFORE recording if no usable model, and deep-link the
    // user to the download page (spec: model-missing notification deep-link).
    if let Err(e) = stt::resolve_model_path(&settings) {
        reset_idle(&app, &dict);
        crate::notify::send(&app, settings.notifications_enabled, &e);
        tray::show_main(&app);
        let _ = app.emit_to("main", "navigate", "transcription");
        return;
    }

    // The single capture slot is busy (mic test running in the main window).
    if app.state::<audio::AudioState>().0.lock().unwrap().is_some() {
        reset_idle(&app, &dict);
        crate::notify::send(
            &app,
            settings.notifications_enabled,
            "Microphone is already in use — stop the mic test first",
        );
        return;
    }

    // audio::start blocks up to ~5s while the stream spins up.
    let device = settings.mic_device.clone();
    let capture_app = app.clone();
    let started = tauri::async_runtime::spawn_blocking(move || audio::start(capture_app, device))
        .await
        .map_err(|e| e.to_string())
        .and_then(|r| r);

    match started {
        Ok(handle) => {
            *app.state::<audio::AudioState>().0.lock().unwrap() = Some(handle);
        }
        Err(e) => {
            reset_idle(&app, &dict);
            crate::notify::send(
                &app,
                settings.notifications_enabled,
                &format!("Could not start recording: {e}"),
            );
            return;
        }
    }

    // A second toggle may have queued a stop while the mic was starting; the
    // stop_flow waiting on `op` will drain the capture — don't show the pill.
    if *dict.phase.lock().unwrap() != Phase::Recording {
        return;
    }

    // Emit state BEFORE showing: the overlay webview is always alive (hidden
    // window), so this ordering avoids a stale-state flash.
    publish_recording(&app);
    if let Err(e) = overlay::show(&app) {
        eprintln!("overlay show failed: {e}"); // dictation still works without the pill
    }
}

async fn stop_flow(app: AppHandle) {
    let dict = app.state::<DictationState>();
    let _op = dict.op.lock().await;

    publish(&app, "transcribing", None);

    let audio_state = app.state::<audio::AudioState>();
    let samples = match audio::stop(&audio_state) {
        Ok(s) => s,
        Err(_) => {
            // Capture never started (start_flow failed/cancelled): just reset.
            reset_idle(&app, &dict);
            return;
        }
    };

    let settings = config::load(&app).unwrap_or_default();

    match stt::transcribe_samples(&app, samples).await {
        Err(e) => flash_error(&app, &dict, e).await,
        Ok(r) if is_effectively_empty(&r.text) => {
            flash_error(&app, &dict, "No speech detected".to_string()).await
        }
        Ok(r) => {
            // Inject while the overlay is still visible — it can never have
            // focus, so the target app keeps the caret.
            let inject_app = app.clone();
            let restore = settings.restore_clipboard;
            let injected = tauri::async_runtime::spawn_blocking(move || {
                inject::insert_text(&inject_app, &r.text, restore)
            })
            .await
            .map_err(|e| e.to_string())
            .and_then(|r| r);

            reset_idle(&app, &dict);
            if let Err(e) = injected {
                crate::notify::send(
                    &app,
                    settings.notifications_enabled,
                    &format!("Could not insert text: {e}"),
                );
            }
        }
    }
}

/// Show a transient error on the pill, then hide and reset.
async fn flash_error(app: &AppHandle, dict: &State<'_, DictationState>, message: String) {
    *dict.phase.lock().unwrap() = Phase::Idle;
    *dict.pressed_at.lock().unwrap() = None;
    publish(app, "error", Some(message));
    tokio::time::sleep(std::time::Duration::from_millis(ERROR_FLASH_MS)).await;
    let _ = overlay::hide(app);
    publish(app, "idle", None);
}

/// Reset stored phase, hide the pill, tell every listener. Used by all
/// success and abort paths.
fn reset_idle(app: &AppHandle, dict: &State<'_, DictationState>) {
    *dict.phase.lock().unwrap() = Phase::Idle;
    *dict.pressed_at.lock().unwrap() = None;
    let _ = overlay::hide(app);
    publish(app, "idle", None);
}

#[derive(Clone, Serialize)]
struct DictationEvent {
    phase: &'static str,
    message: Option<String>,
}

/// Single choke point for state fan-out: webview event + tray icon.
fn publish(app: &AppHandle, phase: &'static str, message: Option<String>) {
    let _ = app.emit("dictation-state", DictationEvent { phase, message });
    tray::set_recording(app, phase == "recording");
}

/// Publish "recording", carrying the latched hint when one applies. Called
/// from BOTH `start_flow` and `release`: a tap can be released before the
/// mic finishes opening, so either one can be the last to run.
fn publish_recording(app: &AppHandle) {
    let dict = app.state::<DictationState>();
    let phase = *dict.phase.lock().unwrap();
    let pressed_at = *dict.pressed_at.lock().unwrap();
    let latched = is_latched(mode_of(app), phase, pressed_at);
    publish(app, "recording", latched.then(|| LATCHED_HINT.to_string()));
}

#[tauri::command]
pub fn toggle_dictation(app: AppHandle) {
    toggle(&app);
}

#[tauri::command]
pub fn get_dictation_state(state: State<'_, DictationState>) -> Result<Phase, String> {
    Ok(*state.phase.lock().map_err(|e| e.to_string())?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_from_idle_starts_recording() {
        assert_eq!(
            on_toggle(Phase::Idle),
            (Phase::Recording, Action::StartRecording)
        );
    }

    #[test]
    fn toggle_from_recording_stops_and_transcribes() {
        assert_eq!(
            on_toggle(Phase::Recording),
            (Phase::Transcribing, Action::StopAndTranscribe)
        );
    }

    #[test]
    fn toggle_during_transcribing_is_ignored() {
        assert_eq!(
            on_toggle(Phase::Transcribing),
            (Phase::Transcribing, Action::Ignore)
        );
    }

    const HOLD: DictationMode = DictationMode::Hold;
    const TOGGLE: DictationMode = DictationMode::Toggle;

    #[test]
    fn press_from_idle_starts_recording_in_both_modes() {
        for mode in [HOLD, TOGGLE] {
            assert_eq!(
                on_press(mode, Phase::Idle),
                (Phase::Recording, Action::StartRecording),
                "mode {mode:?}"
            );
        }
    }

    #[test]
    fn press_while_recording_stops_and_transcribes() {
        // In hold mode this is how a latched tap gets ended.
        for mode in [HOLD, TOGGLE] {
            assert_eq!(
                on_press(mode, Phase::Recording),
                (Phase::Transcribing, Action::StopAndTranscribe),
                "mode {mode:?}"
            );
        }
    }

    #[test]
    fn press_during_transcribing_is_ignored() {
        assert_eq!(
            on_press(HOLD, Phase::Transcribing),
            (Phase::Transcribing, Action::Ignore)
        );
    }

    #[test]
    fn release_in_toggle_mode_never_changes_anything() {
        // The regression guard for the old press-twice behavior.
        for phase in [Phase::Idle, Phase::Recording, Phase::Transcribing] {
            for held_ms in [0, TAP_MS, 10_000] {
                assert_eq!(
                    on_release(TOGGLE, phase, held_ms),
                    (phase, Action::Ignore),
                    "phase {phase:?}, held {held_ms}ms"
                );
            }
        }
    }

    #[test]
    fn release_after_a_hold_stops_and_transcribes() {
        assert_eq!(
            on_release(HOLD, Phase::Recording, TAP_MS),
            (Phase::Transcribing, Action::StopAndTranscribe)
        );
    }

    #[test]
    fn release_after_a_tap_latches_the_recording() {
        assert_eq!(
            on_release(HOLD, Phase::Recording, TAP_MS - 1),
            (Phase::Recording, Action::Ignore)
        );
    }

    #[test]
    fn release_with_unknown_hold_duration_is_treated_as_a_tap() {
        assert_eq!(
            on_release(HOLD, Phase::Recording, 0),
            (Phase::Recording, Action::Ignore)
        );
    }

    #[test]
    fn release_while_idle_never_starts_a_recording() {
        // Where a failed start_flow leaves us — the key-up must not revive it.
        assert_eq!(
            on_release(HOLD, Phase::Idle, 10_000),
            (Phase::Idle, Action::Ignore)
        );
    }

    #[test]
    fn release_during_transcribing_is_ignored() {
        assert_eq!(
            on_release(HOLD, Phase::Transcribing, 10_000),
            (Phase::Transcribing, Action::Ignore)
        );
    }

    #[test]
    fn only_a_recording_with_the_key_up_in_hold_mode_is_latched() {
        assert!(is_latched(HOLD, Phase::Recording, None));
        assert!(!is_latched(HOLD, Phase::Recording, Some(Instant::now())), "key still down");
        assert!(!is_latched(TOGGLE, Phase::Recording, None), "toggle mode never latches");
        assert!(!is_latched(HOLD, Phase::Idle, None));
        assert!(!is_latched(HOLD, Phase::Transcribing, None));
    }

    #[test]
    fn dictation_mode_defaults_to_hold() {
        assert_eq!(DictationMode::default(), DictationMode::Hold);
    }

    #[test]
    fn dictation_mode_serializes_lowercase_for_the_frontend() {
        assert_eq!(serde_json::to_string(&DictationMode::Toggle).unwrap(), "\"toggle\"");
        assert_eq!(serde_json::to_string(&DictationMode::Hold).unwrap(), "\"hold\"");
    }

    #[test]
    fn dictation_mode_deserializes_toggle() {
        let m: DictationMode = serde_json::from_str("\"toggle\"").unwrap();
        assert_eq!(m, DictationMode::Toggle);
    }

    #[test]
    fn unusable_dictation_mode_values_fall_back_to_the_default() {
        // A typo in the hand-editable settings.json must not fail the load.
        // from_value is the production path (see `config::load`).
        let bad = [
            serde_json::json!("Toggle"), // right word, wrong case
            serde_json::json!("nonsense"),
            serde_json::json!(42),
            serde_json::json!(null),
            serde_json::json!({}),
        ];
        for v in bad {
            let m: DictationMode = serde_json::from_value(v.clone())
                .unwrap_or_else(|e| panic!("{v} should not fail deserialization: {e}"));
            assert_eq!(m, DictationMode::Hold, "for {v}");
        }
    }

    #[test]
    fn default_phase_is_idle() {
        assert_eq!(Phase::default(), Phase::Idle);
    }

    #[test]
    fn phase_serializes_lowercase_for_the_frontend() {
        assert_eq!(
            serde_json::to_string(&Phase::Transcribing).unwrap(),
            "\"transcribing\""
        );
    }

    #[test]
    fn blank_whisper_output_is_effectively_empty() {
        assert!(is_effectively_empty(""));
        assert!(is_effectively_empty("   \n"));
        assert!(is_effectively_empty("[BLANK_AUDIO]"));
        assert!(is_effectively_empty(" (wind blowing) "));
        assert!(!is_effectively_empty("Hello world."));
        assert!(!is_effectively_empty("[unclear] but real speech"));
    }
}
