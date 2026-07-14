use serde::Serialize;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_notification::NotificationExt;

use crate::{audio, config, inject, overlay, stt, tray};

/// How long a transient error stays on the overlay before it hides.
const ERROR_FLASH_MS: u64 = 1800;

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
    /// Serializes the spawned start/stop flows so a stop queued behind a
    /// still-starting capture waits instead of racing it. tokio's Mutex is
    /// FIFO-fair, so flows run in press order.
    op: tokio::sync::Mutex<()>,
}

/// THE single entry point — the global-shortcut handler, tray menu item and
/// `toggle_dictation` command all call this. Runs on the caller's thread
/// (often the main event loop): it only flips the phase mutex and spawns,
/// never blocks.
pub fn toggle(app: &AppHandle) {
    let action = {
        let state = app.state::<DictationState>();
        let mut phase = state.phase.lock().unwrap();
        let (next, action) = on_toggle(*phase);
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

async fn start_flow(app: AppHandle) {
    let dict = app.state::<DictationState>();
    let _op = dict.op.lock().await;

    let settings = match config::load(&app) {
        Ok(s) => s,
        Err(e) => {
            reset_idle(&app, &dict);
            notify(&app, true, &format!("Could not load settings: {e}"));
            return;
        }
    };

    // Preflight: fail BEFORE recording if no usable model, and deep-link the
    // user to the download page (spec: model-missing notification deep-link).
    if let Err(e) = stt::resolve_model_path(&settings) {
        reset_idle(&app, &dict);
        notify(&app, settings.notifications_enabled, &e);
        tray::show_main(&app);
        let _ = app.emit_to("main", "navigate", "transcription");
        return;
    }

    // The single capture slot is busy (mic test running in the main window).
    if app.state::<audio::AudioState>().0.lock().unwrap().is_some() {
        reset_idle(&app, &dict);
        notify(
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
            notify(
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
    publish(&app, "recording", None);
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
                notify(
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
    publish(app, "error", Some(message));
    tokio::time::sleep(std::time::Duration::from_millis(ERROR_FLASH_MS)).await;
    let _ = overlay::hide(app);
    publish(app, "idle", None);
}

/// Reset stored phase, hide the pill, tell every listener. Used by all
/// success and abort paths.
fn reset_idle(app: &AppHandle, dict: &State<'_, DictationState>) {
    *dict.phase.lock().unwrap() = Phase::Idle;
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

fn notify(app: &AppHandle, enabled: bool, body: &str) {
    if !enabled {
        return;
    }
    let _ = app
        .notification()
        .builder()
        .title("Claudy")
        .body(body)
        .show();
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
