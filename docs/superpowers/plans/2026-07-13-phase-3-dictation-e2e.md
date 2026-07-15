# Phase 3 — Dictation E2E Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Global-shortcut-driven dictation that works system-wide: press `Ctrl+Shift+D` anywhere → overlay pill appears and recording starts → press again → whisper transcribes → text is pasted into the still-focused app. *First success criterion of the spec.*

**Architecture:** Four new Rust modules — `overlay` (pill window lifecycle), `inject` (clipboard-paste text insertion via enigo), `dictation` (state machine + orchestration, the single entry point for shortcut/tray/UI), `shortcuts` (accelerator registration with live re-registration). The frontend gains a real `OverlayPage` driven by a `dictation-state` event and the existing `mic-level` event. Reuses Phase 2's `audio::start/stop` and `stt::transcribe_samples` unchanged.

**Tech Stack:** Tauri 2 (`global-shortcut`, `clipboard-manager`, `notification` plugins — all already registered), `enigo` 0.6 (new dep, Ctrl+V simulation via SendInput), React + Tailwind for the overlay pill.

**Spec:** `docs/superpowers/specs/2026-07-12-claudy-ai-assistant-design.md`
**Roadmap context:** Phase 3 of 6 — "Dictation E2E — global shortcut, overlay window, paste injection." Out of scope: shortcut-editor UI and conflict-warning UI (Phase 5), AI providers/prompt shortcuts and `auto_paste` (Phase 4).

## Global Constraints

- Windows 11 is the dev/verification target; keep code cross-platform-shaped (no `#[cfg(windows)]` unless unavoidable).
- Rust-core monolith: all logic in Rust; the webview is purely presentational (spec line 23).
- Injection strategy is clipboard-paste (save clipboard → set text → simulate Ctrl+V → restore), never per-key typing (spec line 25).
- Audio stays in memory; no new files or storage locations; zero telemetry.
- No silent failures: every user-triggered action ends in visible success or visible error (spec line 80).
- `restore_clipboard` (default true) and `notifications_enabled` (default true) settings must be honored; `auto_paste` is **not** consulted in dictation (injection *is* the deliverable — `auto_paste` gates Phase 4 prompt results only).
- Run Rust commands from PowerShell (`cargo` is not on Git Bash PATH). All existing tests must stay green: `cd src-tauri; cargo test`. Frontend gate: `npx tsc --noEmit`.
- Commit format: `<type>: <description>`, no attribution footer (globally disabled).

## Existing interfaces you will consume (already implemented — do not modify)

- `audio::start(app: AppHandle, device_name: String) -> Result<CaptureHandle, String>` — blocks up to ~5 s while the stream spins up; `""` = default device.
- `audio::AudioState(pub Mutex<Option<CaptureHandle>>)` — managed state; the single capture slot.
- `audio::stop(state: &AudioState) -> Result<Vec<f32>, String>` — joins the capture thread, returns 16 kHz mono f32 samples; `Err` if nothing is capturing.
- `stt::transcribe_samples(app: &AppHandle, samples: Vec<f32>) -> Result<TranscriptionResult, String>` — async, runs whisper on `spawn_blocking`, honors keep-warm cache, rejects < 0.5 s of audio. `TranscriptionResult { text: String, duration_ms: u64 }`.
- `stt::resolve_model_path(settings: &Settings) -> Result<PathBuf, String>` — user-friendly errors for "no model selected" / "model file missing".
- `config::load(app: &AppHandle) -> Result<Settings, String>` / `config::save(...)` — fields used here: `mic_device`, `dictation_shortcut`, `restore_clipboard`, `notifications_enabled`.
- Event `"mic-level"` `{ level: f32 }` emitted every 50 ms while capturing (reused by the overlay meter).
- `tauri.conf.json` already defines the `overlay` window (300×70, hidden, transparent, alwaysOnTop, skipTaskbar, no decorations, `focus: false`). Capabilities in `capabilities/default.json` already cover both windows for global-shortcut, clipboard read/write-text, and notifications. **No config/capability changes are needed in this phase.**
- `index.css` already paints `html, body, #root` transparent (unlayered rule) — the overlay window needs no CSS work to stay transparent.

---

### Task 1: `overlay.rs` — pill window positioning + lifecycle (TDD)

**Files:**
- Create: `src-tauri/src/overlay.rs`
- Modify: `src-tauri/src/lib.rs` (module decl + setup wiring)

**Interfaces:**
- Consumes: the pre-existing `overlay` window from `tauri.conf.json`.
- Produces: `overlay::init(app: &AppHandle) -> Result<(), String>`, `overlay::show(app: &AppHandle) -> Result<(), String>`, `overlay::hide(app: &AppHandle) -> Result<(), String>`, pure `overlay::bottom_center(work_pos: (i32, i32), work_size: (u32, u32), window: (u32, u32), margin: u32) -> (i32, i32)`. Task 3 calls `show`/`hide`.

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/overlay.rs` containing only the test module for the pure positioning function (all units are physical pixels — `work_area()` and `outer_size()` both return physical units, so the math is DPI-consistent):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centers_horizontally_and_sits_above_bottom_edge() {
        // 1920x1040 work area at origin, 300x70 window, 96px margin
        let (x, y) = bottom_center((0, 0), (1920, 1040), (300, 70), 96);
        assert_eq!(x, (1920 - 300) / 2);
        assert_eq!(y, 1040 - 70 - 96);
    }

    #[test]
    fn handles_secondary_monitor_with_negative_origin() {
        // Monitor to the left of primary: origin (-2560, 120)
        let (x, y) = bottom_center((-2560, 120), (2560, 1400), (300, 70), 96);
        assert_eq!(x, -2560 + (2560 - 300) / 2);
        assert_eq!(y, 120 + 1400 - 70 - 96);
    }

    #[test]
    fn window_wider_than_work_area_does_not_panic() {
        // Degenerate case must not panic; x may be negative relative to origin.
        let (x, _) = bottom_center((0, 0), (200, 400), (300, 70), 96);
        assert_eq!(x, (200 - 300) / 2);
    }
}
```

Add `mod overlay;` to `src-tauri/src/lib.rs` (alphabetical, after `mod models;`) so the file compiles as part of the crate.

- [ ] **Step 2: Run tests to verify they fail**

Run (PowerShell): `cd src-tauri; cargo test overlay`
Expected: FAIL to compile — `bottom_center` not found.

- [ ] **Step 3: Write the implementation**

Prepend to `src-tauri/src/overlay.rs` (above the test module):

```rust
use tauri::{AppHandle, Manager, PhysicalPosition};

pub const OVERLAY_LABEL: &str = "overlay";
const BOTTOM_MARGIN_PX: u32 = 96;

/// Top-left position that horizontally centers `window` in the monitor work
/// area and rests it `margin` px above the bottom edge. Physical pixels.
pub fn bottom_center(
    work_pos: (i32, i32),
    work_size: (u32, u32),
    window: (u32, u32),
    margin: u32,
) -> (i32, i32) {
    let x = work_pos.0 + (work_size.0 as i32 - window.0 as i32) / 2;
    let y = work_pos.1 + work_size.1 as i32 - window.1 as i32 - margin as i32;
    (x, y)
}

/// `focus: false` in tauri.conf.json only affects creation; non-focusable
/// (WS_EX_NOACTIVATE on Windows) guarantees `show()` never steals focus
/// from the app the user is dictating into.
pub fn init(app: &AppHandle) -> Result<(), String> {
    let w = app
        .get_webview_window(OVERLAY_LABEL)
        .ok_or("overlay window missing")?;
    w.set_focusable(false).map_err(|e| e.to_string())
}

/// Positions the pill at the bottom-center of the monitor the cursor is on
/// (dictation targets the app under the user's attention), then shows it.
/// Never calls set_focus.
pub fn show(app: &AppHandle) -> Result<(), String> {
    let w = app
        .get_webview_window(OVERLAY_LABEL)
        .ok_or("overlay window missing")?;
    let monitor = app
        .cursor_position()
        .ok()
        .and_then(|p| app.monitor_from_point(p.x, p.y).ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten())
        .ok_or("no monitor found")?;
    let area = monitor.work_area();
    let size = w.outer_size().map_err(|e| e.to_string())?;
    let (x, y) = bottom_center(
        (area.position.x, area.position.y),
        (area.size.width, area.size.height),
        (size.width, size.height),
        BOTTOM_MARGIN_PX,
    );
    w.set_position(PhysicalPosition::new(x, y))
        .map_err(|e| e.to_string())?;
    w.show().map_err(|e| e.to_string())
}

pub fn hide(app: &AppHandle) -> Result<(), String> {
    let w = app
        .get_webview_window(OVERLAY_LABEL)
        .ok_or("overlay window missing")?;
    w.hide().map_err(|e| e.to_string())
}
```

Wire setup in `src-tauri/src/lib.rs` (`String` converts into setup's `Box<dyn Error>` automatically):

```rust
        .setup(|app| {
            tray::create(app.handle())?;
            overlay::init(app.handle())?;
            Ok(())
        })
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri; cargo test overlay`
Expected: 3 passed. Then `cd src-tauri; cargo test` — all existing suites still green.

- [ ] **Step 5: Manual smoke check (dev app)**

Run `npm run tauri dev`, open devtools on the main window (right-click → Inspect), then:

```js
const o = await window.__TAURI__.webviewWindow.WebviewWindow.getByLabel("overlay");
await o.show();   // pill appears (static "Recording…" placeholder for now)
await o.hide();
```

Expected: the placeholder pill appears without stealing focus from devtools, and hides again. (Positioning via `overlay::show` is exercised in Task 3.)

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/overlay.rs src-tauri/src/lib.rs
git commit -m "feat: add overlay window lifecycle with bottom-center positioning"
```

---

### Task 2: `inject.rs` — clipboard-paste text insertion with enigo

**Files:**
- Modify: `src-tauri/Cargo.toml` (add `enigo`)
- Create: `src-tauri/src/inject.rs`
- Modify: `src-tauri/src/lib.rs` (module decl + command registration)

**Interfaces:**
- Consumes: `tauri_plugin_clipboard_manager::ClipboardExt` (plugin already registered), `config::load`.
- Produces: `inject::insert_text(app: &AppHandle, text: &str, restore_clipboard: bool) -> Result<(), String>` (blocking, ~250 ms of sleeps — callers must use `spawn_blocking`); command `paste_text(app, text: String)`. Task 3 calls `insert_text`; `paste_text` is the manual-verification hook now and the primitive Phase 4's opt-in auto-paste will reuse.

This module is all OS side effects (clipboard, synthetic keystrokes) — no meaningful unit tests exist for it; verification is manual (Step 3). That is why this task has no TDD cycle.

- [ ] **Step 1: Add the dependency**

In `src-tauri/Cargo.toml` add to `[dependencies]` (after `hex = "0.4"`):

```toml
enigo = "0.6"
```

Run: `cd src-tauri; cargo build`
Expected: compiles clean (enigo 0.6.x uses SendInput on Windows; no extra system deps).

- [ ] **Step 2: Write the implementation**

Create `src-tauri/src/inject.rs`:

```rust
use std::{thread, time::Duration};

use enigo::{Direction, Enigo, Key, Keyboard, Settings as EnigoSettings};
use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;

/// Delay after writing the clipboard before sending Ctrl+V — the clipboard
/// write must be observable to the target app first.
const CLIPBOARD_SETTLE_MS: u64 = 50;
/// Delay after Ctrl+V before restoring the clipboard — target apps read the
/// clipboard asynchronously; restoring earlier makes them paste the OLD text.
const PASTE_SETTLE_MS: u64 = 200;

/// Insert `text` into the currently focused app via clipboard-paste:
/// save clipboard (if restoring) -> write text -> Ctrl+V -> restore.
/// Blocking (~250 ms of sleeps): always call via spawn_blocking, never on
/// the event-loop thread.
pub fn insert_text(app: &AppHandle, text: &str, restore_clipboard: bool) -> Result<(), String> {
    let previous = if restore_clipboard {
        // Err = empty or non-text clipboard (image/files): nothing we can
        // snapshot, so nothing to restore. Documented limitation.
        app.clipboard().read_text().ok()
    } else {
        None
    };

    app.clipboard()
        .write_text(text.to_string())
        .map_err(|e| format!("Clipboard write failed: {e}"))?;
    thread::sleep(Duration::from_millis(CLIPBOARD_SETTLE_MS));

    send_paste()?;

    if let Some(prev) = previous {
        thread::sleep(Duration::from_millis(PASTE_SETTLE_MS));
        app.clipboard()
            .write_text(prev)
            .map_err(|e| format!("Clipboard restore failed: {e}"))?;
    }
    Ok(())
}

fn send_paste() -> Result<(), String> {
    // Constructed per call: cheap on Windows, and enigo's default
    // release_keys_when_dropped(true) cleans up stuck keys on error.
    let mut enigo = Enigo::new(&EnigoSettings::default())
        .map_err(|e| format!("Input simulation unavailable: {e}"))?;
    enigo
        .key(Key::Control, Direction::Press)
        .map_err(|e| format!("Paste keystroke failed: {e}"))?;
    let click = enigo.key(Key::Unicode('v'), Direction::Click);
    // Always attempt the release, even if the 'v' click failed.
    let release = enigo.key(Key::Control, Direction::Release);
    click.map_err(|e| format!("Paste keystroke failed: {e}"))?;
    release.map_err(|e| format!("Could not release Ctrl: {e}"))
}

/// Manual-verification hook for this phase and the primitive for Phase 4's
/// opt-in auto-paste of prompt results.
#[tauri::command]
pub async fn paste_text(app: AppHandle, text: String) -> Result<(), String> {
    let settings = crate::config::load(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        insert_text(&app, &text, settings.restore_clipboard)
    })
    .await
    .map_err(|e| e.to_string())?
}
```

Modify `src-tauri/src/lib.rs` — add `mod inject;` (alphabetical, after `mod download;`) and register the command at the end of `invoke_handler`:

```rust
            stt::stop_capture_and_transcribe,
            inject::paste_text
```

Run: `cd src-tauri; cargo test`
Expected: compiles; all existing tests pass.

- [ ] **Step 3: Manual verification**

Run `npm run tauri dev`. Put `SENTINEL-BEFORE` on the clipboard (copy it from anywhere). Open Notepad. In main-window devtools:

```js
setTimeout(() => window.__TAURI__.core.invoke("paste_text", { text: "hello from claudy" }), 3000);
```

Then click into Notepad within 3 seconds. Expected:
1. `hello from claudy` appears in Notepad at the caret.
2. After ~250 ms, paste manually (Ctrl+V) in Notepad: `SENTINEL-BEFORE` appears — clipboard was restored.
3. Turn restore off and repeat — clipboard now retains `hello from claudy`:

```js
const s = await window.__TAURI__.core.invoke("get_settings");
await window.__TAURI__.core.invoke("update_settings", { settings: { ...s, restoreClipboard: false } });
// ...repeat the paste_text test, then restore:
await window.__TAURI__.core.invoke("update_settings", { settings: { ...s, restoreClipboard: true } });
```

- [ ] **Step 4: Commit**

```powershell
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/inject.rs src-tauri/src/lib.rs
git commit -m "feat: add clipboard-paste text injection via enigo"
```

---

### Task 3: `dictation.rs` — state machine + orchestration flows (TDD)

**Files:**
- Modify: `src-tauri/Cargo.toml` (tokio features)
- Create: `src-tauri/src/dictation.rs`
- Modify: `src-tauri/src/tray.rs:32` (make `show_main` pub — one-word change)
- Modify: `src-tauri/src/lib.rs` (module, managed state, commands)

**Interfaces:**
- Consumes: `overlay::show/hide` (Task 1), `inject::insert_text` (Task 2), `audio::start/stop`, `audio::AudioState`, `stt::transcribe_samples`, `stt::resolve_model_path`, `config::load`, `tray::show_main`.
- Produces: `dictation::toggle(app: &AppHandle)` — THE single entry point (Task 4's shortcut handler and Task 5's tray item call this); `DictationState` managed state; event `"dictation-state"` with payload `{ phase: "idle"|"recording"|"transcribing"|"error", message: string|null }` (Task 6 consumes); commands `toggle_dictation()`, `get_dictation_state() -> "idle"|"recording"|"transcribing"`; pure `on_toggle(Phase) -> (Phase, Action)` and `is_effectively_empty(&str) -> bool`; `publish(app, phase, message)` — the single choke point Task 5 extends with the tray-icon call.

- [ ] **Step 1: Update tokio features**

In `src-tauri/Cargo.toml` change the tokio line (the flows need an async `Mutex` and `sleep`):

```toml
tokio = { version = "1", features = ["fs", "io-util", "sync", "time"] }
```

- [ ] **Step 2: Write the failing tests**

Create `src-tauri/src/dictation.rs` containing only the test module, and add `mod dictation;` to `lib.rs` (after `mod config;`) so it compiles:

```rust
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
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cd src-tauri; cargo test dictation`
Expected: FAIL to compile — `Phase`, `Action`, `on_toggle`, `is_effectively_empty` not found.

- [ ] **Step 4: Write the implementation**

Prepend to `src-tauri/src/dictation.rs`:

```rust
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

/// Single choke point for state fan-out. Task 5 adds the tray-icon call here.
fn publish(app: &AppHandle, phase: &'static str, message: Option<String>) {
    let _ = app.emit("dictation-state", DictationEvent { phase, message });
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
```

Modify `src-tauri/src/tray.rs:32` — the deep-link needs it:

```rust
pub fn show_main(app: &AppHandle) {
```

Modify `src-tauri/src/lib.rs`: add the managed state and the two commands:

```rust
        .manage(stt::SttState::default())
        .manage(dictation::DictationState::default())
```

```rust
            inject::paste_text,
            dictation::toggle_dictation,
            dictation::get_dictation_state
```

Note on `TranscriptionResult`: this code reads `r.text` — if that field is not already `pub` in `src-tauri/src/stt.rs` (around line 25), make it `pub`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd src-tauri; cargo test`
Expected: 6 new `dictation` tests pass; all existing suites stay green.

- [ ] **Step 6: Manual verification (full flow via command, before the shortcut exists)**

`npm run tauri dev`, ensure a model is downloaded + selected on the Transcription page, open Notepad, then in main-window devtools:

```js
// start: pill should appear bottom-center of the cursor's monitor
await window.__TAURI__.core.invoke("toggle_dictation");
// speak a sentence; click into Notepad before this fires so injection lands there:
setTimeout(() => window.__TAURI__.core.invoke("toggle_dictation"), 4000);
```

Expected: pill appears (static placeholder text for now — real states come in Task 6), after the second toggle the pill hides and the spoken text is pasted into Notepad. Also verify error paths:
- Deselect the model (Transcription page) → toggle → notification "no model…", main window shows (page switch to Transcription lands in Task 6).
- Toggle twice within ~0.4 s → transient error event ("too short"), pill hides, and `await window.__TAURI__.core.invoke("get_dictation_state")` → `"idle"`.

- [ ] **Step 7: Commit**

```powershell
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/dictation.rs src-tauri/src/tray.rs src-tauri/src/lib.rs
git commit -m "feat: add dictation state machine orchestrating capture, STT and injection"
```

---

### Task 4: `shortcuts.rs` — global shortcut with live re-registration (TDD)

**Files:**
- Create: `src-tauri/src/shortcuts.rs`
- Modify: `src-tauri/src/config.rs:61-64` (`update_settings` re-registers)
- Modify: `src-tauri/src/lib.rs` (module decl + setup wiring)

**Interfaces:**
- Consumes: `dictation::toggle` (Task 3), `config::load`, `tauri_plugin_global_shortcut::GlobalShortcutExt` (plugin already registered at `lib.rs:21` — keep `Builder::new().build()` unchanged; per-shortcut handlers via `on_shortcut` are the re-registration mechanism, not the builder-time global handler; register/unregister are marshalled to the main thread internally, so they are safe to call from commands).
- Produces: `shortcuts::parse(accel: &str) -> Result<Shortcut, String>`, `shortcuts::register_dictation(app: &AppHandle, old: Option<&str>, new: &str) -> Result<(), String>`, `shortcuts::init(app: &AppHandle)`. `update_settings` becomes the live re-registration path (Phase 5's shortcut editor will call it unchanged).

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/shortcuts.rs` with only the test module (parsing is pure `global_hotkey` code — no Tauri runtime needed), and add `mod shortcuts;` to `lib.rs` (after `mod overlay;`):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_default_accelerator() {
        assert!(parse("Ctrl+Shift+D").is_ok());
    }

    #[test]
    fn parses_cross_platform_modifier() {
        assert!(parse("CmdOrCtrl+Space").is_ok());
    }

    #[test]
    fn rejects_empty_accelerator() {
        let err = parse("  ").unwrap_err();
        assert!(err.contains("empty"), "got: {err}");
    }

    #[test]
    fn rejects_unknown_key_with_the_accelerator_in_the_message() {
        let err = parse("NotAKey+Q").unwrap_err();
        assert!(err.contains("NotAKey+Q"), "got: {err}");
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri; cargo test shortcuts`
Expected: FAIL to compile — `parse` not found.

- [ ] **Step 3: Write the implementation**

Prepend to `src-tauri/src/shortcuts.rs`:

```rust
use tauri::AppHandle;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

/// Validate an accelerator string ("Ctrl+Shift+D", "CmdOrCtrl+Space", ...).
pub fn parse(accel: &str) -> Result<Shortcut, String> {
    let accel = accel.trim();
    if accel.is_empty() {
        return Err("Shortcut must not be empty".into());
    }
    accel
        .parse::<Shortcut>()
        .map_err(|e| format!("Invalid shortcut \"{accel}\": {e}"))
}

fn on_dictation_shortcut(app: &AppHandle, shortcut: Shortcut) -> Result<(), String> {
    app.global_shortcut()
        .on_shortcut(shortcut, |app, _shortcut, event| {
            // Fires for BOTH press and release — without this filter every
            // press would toggle dictation twice.
            if event.state == ShortcutState::Pressed {
                crate::dictation::toggle(app);
            }
        })
        .map_err(|e| e.to_string())
}

/// Live (re-)registration: unregister `old` (if any), register `new`.
/// If `new` fails (another app owns the combo), the old binding is restored
/// so dictation keeps working, and the error is returned to the caller.
pub fn register_dictation(app: &AppHandle, old: Option<&str>, new: &str) -> Result<(), String> {
    let shortcut = parse(new)?;
    if let Some(old_accel) = old {
        if let Ok(old_shortcut) = parse(old_accel) {
            let _ = app.global_shortcut().unregister(old_shortcut);
        }
    }
    if let Err(e) = on_dictation_shortcut(app, shortcut) {
        if let Some(old_accel) = old {
            if let Ok(old_shortcut) = parse(old_accel) {
                let _ = on_dictation_shortcut(app, old_shortcut);
            }
        }
        return Err(format!("Could not register \"{new}\": {e}"));
    }
    Ok(())
}

/// Startup registration from settings. A conflict (combo owned by another
/// app) is NON-FATAL: notify and keep running — the tray toggle still works.
pub fn init(app: &AppHandle) {
    let settings = crate::config::load(app).unwrap_or_default();
    if let Err(e) = register_dictation(app, None, &settings.dictation_shortcut) {
        use tauri_plugin_notification::NotificationExt;
        let _ = app
            .notification()
            .builder()
            .title("Claudy")
            .body(format!("Dictation shortcut unavailable: {e}"))
            .show();
    }
}
```

Modify `src-tauri/src/config.rs` — replace `update_settings` (lines 61-64) so a failed registration rejects the settings write (the UI then surfaces the error string):

```rust
#[tauri::command]
pub fn update_settings(app: AppHandle, settings: Settings) -> Result<(), String> {
    let old = load(&app)?;
    if old.dictation_shortcut != settings.dictation_shortcut {
        crate::shortcuts::register_dictation(
            &app,
            Some(&old.dictation_shortcut),
            &settings.dictation_shortcut,
        )?;
    }
    save(&app, &settings)
}
```

Modify `src-tauri/src/lib.rs` setup:

```rust
        .setup(|app| {
            tray::create(app.handle())?;
            overlay::init(app.handle())?;
            shortcuts::init(app.handle());
            Ok(())
        })
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri; cargo test`
Expected: 4 new `shortcuts` tests pass; `config` tests still green (they don't call `update_settings`).

- [ ] **Step 5: Manual verification — the E2E success criterion**

`npm run tauri dev`, model selected, focus Notepad:
1. Press **Ctrl+Shift+D** → pill appears, speak, press **Ctrl+Shift+D** again → text lands in Notepad. **This is Phase 3's success criterion.** Verify it also works with the main window hidden to tray.
2. Live re-registration via devtools:

```js
const s = await window.__TAURI__.core.invoke("get_settings");
await window.__TAURI__.core.invoke("update_settings", { settings: { ...s, dictationShortcut: "Ctrl+Shift+F9" } });
```

Expected: `Ctrl+Shift+D` is dead, `Ctrl+Shift+F9` toggles dictation — no restart.
3. Invalid accelerator rejected, current one still live:

```js
await window.__TAURI__.core.invoke("update_settings", { settings: { ...s, dictationShortcut: "NotAKey+Q" } })
  .catch((e) => e); // -> 'Invalid shortcut "NotAKey+Q": ...'; Ctrl+Shift+F9 still works
```

4. Restore the default: `update_settings` with `dictationShortcut: "Ctrl+Shift+D"`.

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/shortcuts.rs src-tauri/src/config.rs src-tauri/src/lib.rs
git commit -m "feat: register dictation global shortcut with live re-registration"
```

---

### Task 5: Tray wiring + recording state icon (TDD)

**Files:**
- Modify: `src-tauri/src/tray.rs` (menu wiring, `set_recording`, generated icon)
- Modify: `src-tauri/src/dictation.rs` (`publish` calls the tray)

**Interfaces:**
- Consumes: `dictation::toggle` (Task 3), `app.tray_by_id("main-tray")` (id set at `tray.rs:17`).
- Produces: `tray::set_recording(app: &AppHandle, recording: bool)` called only from `dictation::publish`; pure `tray::recording_icon_rgba(size: u32) -> Vec<u8>`.

The recording icon is generated at runtime (a red dot on transparency) — no PNG asset, no extra cargo feature, and the generator is unit-testable.

- [ ] **Step 1: Write the failing tests**

Append to `src-tauri/src/tray.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: u32 = 32;

    fn pixel(rgba: &[u8], x: u32, y: u32) -> &[u8] {
        let i = ((y * SIZE + x) * 4) as usize;
        &rgba[i..i + 4]
    }

    #[test]
    fn recording_icon_buffer_has_rgba_length() {
        let rgba = recording_icon_rgba(SIZE);
        assert_eq!(rgba.len(), (SIZE * SIZE * 4) as usize);
    }

    #[test]
    fn recording_icon_center_is_opaque_red() {
        let rgba = recording_icon_rgba(SIZE);
        let p = pixel(&rgba, SIZE / 2, SIZE / 2);
        assert!(p[0] > 0xC0, "red channel, got {p:?}");
        assert_eq!(p[3], 0xFF, "opaque alpha");
    }

    #[test]
    fn recording_icon_corners_are_transparent() {
        let rgba = recording_icon_rgba(SIZE);
        assert_eq!(pixel(&rgba, 0, 0)[3], 0);
        assert_eq!(pixel(&rgba, SIZE - 1, SIZE - 1)[3], 0);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri; cargo test tray`
Expected: FAIL to compile — `recording_icon_rgba` not found.

- [ ] **Step 3: Write the implementation**

In `src-tauri/src/tray.rs`, wire the menu item (line 24) to the state machine:

```rust
            "toggle_dictation" => crate::dictation::toggle(app),
```

Add below `show_main`:

```rust
const TRAY_ICON_SIZE: u32 = 32;

/// Filled red circle on transparency, generated at runtime (no asset file).
pub fn recording_icon_rgba(size: u32) -> Vec<u8> {
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let c = (size / 2) as i32;
    let r = (size / 2 - 2) as i32;
    for y in 0..size as i32 {
        for x in 0..size as i32 {
            let (dx, dy) = (x - c, y - c);
            if dx * dx + dy * dy <= r * r {
                let i = ((y as u32 * size + x as u32) * 4) as usize;
                rgba[i] = 0xE7; // red
                rgba[i + 1] = 0x00;
                rgba[i + 2] = 0x2E;
                rgba[i + 3] = 0xFF;
            }
        }
    }
    rgba
}

/// State-reflecting tray icon: red dot + tooltip while recording, app icon
/// otherwise. Called only from dictation::publish.
pub fn set_recording(app: &AppHandle, recording: bool) {
    let Some(tray) = app.tray_by_id("main-tray") else {
        return;
    };
    if recording {
        let icon = tauri::image::Image::new_owned(
            recording_icon_rgba(TRAY_ICON_SIZE),
            TRAY_ICON_SIZE,
            TRAY_ICON_SIZE,
        );
        let _ = tray.set_icon(Some(icon));
        let _ = tray.set_tooltip(Some("Claudy — recording"));
    } else {
        let _ = tray.set_icon(app.default_window_icon().cloned());
        let _ = tray.set_tooltip(Some("Claudy"));
    }
}
```

Modify `src-tauri/src/dictation.rs` — extend `publish` (the single choke point):

```rust
/// Single choke point for state fan-out: webview event + tray icon.
fn publish(app: &AppHandle, phase: &'static str, message: Option<String>) {
    let _ = app.emit("dictation-state", DictationEvent { phase, message });
    tray::set_recording(app, phase == "recording");
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri; cargo test`
Expected: 3 new `tray` tests pass; everything else green.

- [ ] **Step 5: Manual verification**

`npm run tauri dev`:
1. Tray menu → "Toggle Dictation" → recording starts, tray icon becomes a red dot, tooltip reads "Claudy — recording".
2. Tray menu → "Toggle Dictation" again → transcription runs, icon reverts to the app icon.
3. Same icon behavior when toggling via Ctrl+Shift+D.

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/tray.rs src-tauri/src/dictation.rs
git commit -m "feat: wire tray dictation toggle with recording state icon"
```

---

### Task 6: Overlay UI states + navigate deep-link (frontend)

**Files:**
- Create: `src/lib/dictation-api.ts`
- Modify: `src/windows/OverlayPage.tsx` (replace placeholder)
- Modify: `src/windows/MainApp.tsx` (navigate listener)

**Interfaces:**
- Consumes: event `"dictation-state"` `{ phase, message }` and commands `toggle_dictation` / `get_dictation_state` (Task 3), event `"mic-level"` via existing `onMicLevel` from `src/lib/stt-api.ts`, event `"navigate"` (Task 3's deep-link).
- Produces: `toggleDictation()`, `getDictationPhase()`, `onDictationState(cb)`, `onNavigate(cb)` in `dictation-api.ts`; the real overlay pill UI.

No frontend test runner exists in this repo (component-test infra arrives in Phase 5, per spec); the gate for this task is `npx tsc --noEmit` + the manual checklist.

- [ ] **Step 1: Create the typed IPC surface**

Create `src/lib/dictation-api.ts`:

```typescript
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type DictationPhase = "idle" | "recording" | "transcribing" | "error";

export interface DictationState {
  phase: DictationPhase;
  message: string | null;
}

export const toggleDictation = (): Promise<void> => invoke("toggle_dictation");

/** Stored phase (never "error" — errors are transient event-only states). */
export const getDictationPhase = (): Promise<Exclude<DictationPhase, "error">> =>
  invoke("get_dictation_state");

export const onDictationState = (
  cb: (state: DictationState) => void,
): Promise<UnlistenFn> =>
  listen<DictationState>("dictation-state", (event) => cb(event.payload));

export const onNavigate = (cb: (page: string) => void): Promise<UnlistenFn> =>
  listen<string>("navigate", (event) => cb(event.payload));
```

- [ ] **Step 2: Implement the overlay pill states**

Replace `src/windows/OverlayPage.tsx` entirely:

```tsx
import { useEffect, useState } from "react";
import { Loader2 } from "lucide-react";
import {
  getDictationPhase,
  onDictationState,
  type DictationPhase,
} from "@/lib/dictation-api";
import { onMicLevel } from "@/lib/stt-api";

const LEVEL_BARS = 5;

export default function OverlayPage() {
  const [phase, setPhase] = useState<DictationPhase>("idle");
  const [message, setMessage] = useState<string | null>(null);
  const [level, setLevel] = useState(0);

  useEffect(() => {
    // Sync on mount: covers dev hot-reload while a dictation is in flight.
    getDictationPhase().then(setPhase).catch(() => {});
    const unState = onDictationState((s) => {
      setPhase(s.phase);
      setMessage(s.message);
    });
    const unLevel = onMicLevel(setLevel);
    return () => {
      unState.then((f) => f());
      unLevel.then((f) => f());
    };
  }, []);

  return (
    <div className="flex h-screen items-center justify-center">
      <div className="flex items-center gap-2 rounded-full bg-black/80 px-4 py-2 text-sm text-white">
        {phase === "recording" && (
          <>
            <span className="h-2 w-2 animate-pulse rounded-full bg-red-500" />
            <LevelBars level={level} />
            <span>Recording…</span>
          </>
        )}
        {phase === "transcribing" && (
          <>
            <Loader2 className="h-4 w-4 animate-spin" />
            <span>Transcribing…</span>
          </>
        )}
        {phase === "error" && (
          <span className="text-red-400">{message ?? "Something went wrong"}</span>
        )}
        {phase === "idle" && <span className="opacity-60">Ready</span>}
      </div>
    </div>
  );
}

interface LevelBarsProps {
  level: number;
}

function LevelBars({ level }: LevelBarsProps) {
  // RMS levels for speech are small; scale up so normal speech lights bars.
  const active = Math.min(LEVEL_BARS, Math.round(level * LEVEL_BARS * 4));
  return (
    <div className="flex items-end gap-0.5">
      {Array.from({ length: LEVEL_BARS }, (_, i) => (
        <span
          key={i}
          className={`w-1 rounded-sm ${i < active ? "bg-green-400" : "bg-white/25"}`}
          style={{ height: `${6 + i * 2}px` }}
        />
      ))}
    </div>
  );
}
```

(The overlay window's document is already transparent — `index.css` paints `html, body, #root` transparent with an unlayered rule; only the pill div has a background.)

- [ ] **Step 3: Wire the navigate deep-link in the main window**

Modify `src/windows/MainApp.tsx` — add the import and a second effect directly below the existing `load()` effect:

```tsx
import { onNavigate } from "@/lib/dictation-api";
```

```tsx
  useEffect(() => {
    const unlisten = onNavigate((page) => {
      if (page in PAGES) setPage(page as PageKey);
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, []);
```

- [ ] **Step 4: Typecheck**

Run: `npx tsc --noEmit`
Expected: no errors.

- [ ] **Step 5: Manual verification**

`npm run tauri dev`:
1. Ctrl+Shift+D → pill shows red pulse + level bars that move when you speak + "Recording…".
2. Ctrl+Shift+D again → spinner + "Transcribing…" → pill hides, text pasted.
3. Deselect the model → Ctrl+Shift+D → notification, main window opens **and switches to the Transcription page** (navigate deep-link now visibly works).
4. Double-tap the shortcut fast → pill flashes the error message (~1.8 s) then hides.

- [ ] **Step 6: Commit**

```powershell
git add src/lib/dictation-api.ts src/windows/OverlayPage.tsx src/windows/MainApp.tsx
git commit -m "feat: add overlay dictation states with level meter and navigate deep-link"
```

---

## Verification (end of Phase 3)

Run `cd src-tauri; cargo test` (all green) and `npx tsc --noEmit` (clean), then the manual E2E checklist with `npm run tauri dev`:

1. **Happy path, three target apps:** focus Notepad / VS Code / a browser text field → Ctrl+Shift+D → pill appears bottom-center of the cursor's monitor **without stealing focus** → speak → level bars move → Ctrl+Shift+D → spinner → text pasted at the caret → pill hides → previously copied text is still on the clipboard (restore works).
2. **Tray path:** tray menu "Toggle Dictation" starts/stops dictation; tray icon shows a red dot + "Claudy — recording" tooltip while recording, reverts after.
3. **Main window closed:** hide the main window to tray → dictation still works end-to-end (tray-first requirement).
4. **No model:** deselect/delete the model → shortcut → visible notification, main window opens on the Transcription page, no recording started.
5. **Mic busy:** start the mic test on the Transcription page → shortcut → "Microphone is already in use" notification, mic test unaffected.
6. **Too short / silence:** double-tap the shortcut (<0.5 s), and separately record 2 s of silence → pill flashes the error, hides, state returns to idle (verify with `get_dictation_state`).
7. **Press during transcribing:** mash the shortcut while the spinner shows → ignored, no double-start, flow completes normally.
8. **Live re-registration:** `update_settings` with a new `dictationShortcut` → old combo dead, new combo live, invalid combo rejected with the old one still working.
9. **Multi-monitor** (if available): move the cursor to a secondary monitor before pressing the shortcut → pill appears on that monitor, correctly positioned.
10. **restore_clipboard off:** set `restoreClipboard: false` → after dictation the transcript remains on the clipboard.

Known/documented limitations (not bugs): elevated (admin) apps ignore injection from non-elevated Claudy; a non-text clipboard (image/files) is not restored; holding the shortcut's modifier keys down through the entire transcription could turn the synthetic Ctrl+V into Ctrl+Shift+V in some apps.
