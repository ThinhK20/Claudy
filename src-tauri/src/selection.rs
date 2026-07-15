use std::{thread, time::Duration};

use tauri::AppHandle;
use tauri_plugin_clipboard_manager::ClipboardExt;

/// Written to the clipboard before Ctrl+C. If Ctrl+C copies nothing (no
/// selection), most apps leave the clipboard untouched — so finding the
/// sentinel afterwards means "no selection". Invisible-separator framing
/// makes a collision with real user data practically impossible.
const SENTINEL: &str = "\u{2063}claudy-selection-probe\u{2063}";
/// Clipboard write must be observable to the target app before Ctrl+C.
const SENTINEL_SETTLE_MS: u64 = 50;
/// Target apps write the clipboard asynchronously after Ctrl+C.
const COPY_SETTLE_MS: u64 = 250;

pub struct Selection {
    /// The focused app's current selection; "" = nothing selected.
    pub text: String,
    /// The user's original clipboard text ("" if empty or non-text).
    pub clipboard: String,
}

/// Pure: what did the probe capture?
pub fn interpret_capture(captured: &str) -> String {
    if captured == SENTINEL {
        String::new()
    } else {
        captured.to_string()
    }
}

/// Read the focused app's selection via clipboard probe:
/// save clipboard -> write sentinel -> Ctrl+C -> read -> restore clipboard.
/// The user's clipboard is restored on EVERY path — the probe must never
/// eat it. Same documented limitation as inject.rs: a non-text clipboard
/// (image/files) can't be snapshotted and is not restored.
/// Blocking (~300 ms of sleeps): always call via spawn_blocking.
pub fn read(app: &AppHandle) -> Result<Selection, String> {
    let original = app.clipboard().read_text().unwrap_or_default();

    app.clipboard()
        .write_text(SENTINEL.to_string())
        .map_err(|e| format!("Clipboard write failed: {e}"))?;
    thread::sleep(Duration::from_millis(SENTINEL_SETTLE_MS));

    let copy_result = crate::inject::send_ctrl_key('c');
    if copy_result.is_ok() {
        thread::sleep(Duration::from_millis(COPY_SETTLE_MS));
    }
    let captured = app.clipboard().read_text().unwrap_or_default();

    let restore_result = app
        .clipboard()
        .write_text(original.clone())
        .map_err(|e| format!("Clipboard restore failed: {e}"));
    // The copy is the root cause when both fail; surface it first.
    copy_result?;
    restore_result?;

    Ok(Selection {
        text: interpret_capture(&captured),
        clipboard: original,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sentinel_still_on_the_clipboard_means_no_selection() {
        assert_eq!(interpret_capture(SENTINEL), "");
    }

    #[test]
    fn anything_else_is_the_captured_selection_verbatim() {
        assert_eq!(interpret_capture("Hello  world"), "Hello  world");
        assert_eq!(interpret_capture(""), ""); // clipboard cleared by target app
    }
}
