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
