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
/// Restore always runs, even if Ctrl+V fails, so `restore_clipboard` is
/// honored on every path; a paste failure is still returned to the caller.
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

    let paste_result = send_chord_key('v');

    if let Some(prev) = previous {
        // Only wait for the target app to read the new clipboard if the
        // paste actually happened — nothing to wait for otherwise.
        if paste_result.is_ok() {
            thread::sleep(Duration::from_millis(PASTE_SETTLE_MS));
        }
        let restore_result = app
            .clipboard()
            .write_text(prev)
            .map_err(|e| format!("Clipboard restore failed: {e}"));
        // Paste is the root cause when both fail; surface it first.
        paste_result?;
        restore_result?;
    } else {
        paste_result?;
    }

    Ok(())
}

/// The platform's copy/paste chord modifier: Cmd on macOS, Ctrl elsewhere.
/// `cfg!` in the initializer keeps both branches type-checked everywhere.
pub(crate) const CHORD_MODIFIER: Key = if cfg!(target_os = "macos") {
    Key::Meta
} else {
    Key::Control
};
pub(crate) const CHORD_LABEL: &str = if cfg!(target_os = "macos") { "Cmd" } else { "Ctrl" };

/// Modifiers the user may still be physically holding from the global
/// shortcut that triggered us (e.g. the Shift of Ctrl+Shift+G). Left held,
/// they contaminate the synthetic chord: Ctrl+C becomes Ctrl+Shift+C
/// (Chrome: DevTools inspect), Ctrl+V becomes Ctrl+Alt+V (Word/Excel:
/// Paste Special) or Win+Ctrl+V. The chord modifier itself is exempt —
/// it is part of the intended chord.
const STRAY_MODIFIERS: [Key; 3] = if cfg!(target_os = "macos") {
    [Key::Shift, Key::Alt, Key::Control]
} else {
    [Key::Shift, Key::Alt, Key::Meta]
};

/// Send <chord modifier>+<c> via input simulation — 'v' pastes
/// (dictation/auto-paste), 'c' copies (selection probe).
pub(crate) fn send_chord_key(c: char) -> Result<(), String> {
    // Constructed per call: cheap on Windows, and enigo's default
    // release_keys_when_dropped(true) cleans up stuck keys on error.
    let mut enigo = Enigo::new(&EnigoSettings::default())
        .map_err(|e| format!("Input simulation unavailable: {e}"))?;
    // Best-effort: releasing an already-up key is a no-op, and a failure
    // here would co-occur with a chord failure, which IS surfaced below.
    for modifier in STRAY_MODIFIERS {
        let _ = enigo.key(modifier, Direction::Release);
    }
    enigo
        .key(CHORD_MODIFIER, Direction::Press)
        .map_err(|e| format!("{CHORD_LABEL}+{c} keystroke failed: {e}"))?;
    let click = enigo.key(Key::Unicode(c), Direction::Click);
    // Always attempt the release, even if the click failed.
    let release = enigo.key(CHORD_MODIFIER, Direction::Release);
    click.map_err(|e| format!("{CHORD_LABEL}+{c} keystroke failed: {e}"))?;
    release.map_err(|e| format!("Could not release {CHORD_LABEL}: {e}"))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chord_modifier_matches_the_platform_convention() {
        if cfg!(target_os = "macos") {
            assert_eq!(CHORD_MODIFIER, Key::Meta);
            assert_eq!(CHORD_LABEL, "Cmd");
        } else {
            assert_eq!(CHORD_MODIFIER, Key::Control);
            assert_eq!(CHORD_LABEL, "Ctrl");
        }
    }

    #[test]
    fn stray_modifiers_never_include_the_chord_modifier() {
        assert!(!STRAY_MODIFIERS.contains(&CHORD_MODIFIER));
    }

    #[test]
    fn stray_modifiers_always_release_shift_and_alt() {
        assert!(STRAY_MODIFIERS.contains(&Key::Shift));
        assert!(STRAY_MODIFIERS.contains(&Key::Alt));
    }
}
