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
/// If `new` fails (another app owns the combo), restoring the old binding is
/// attempted so dictation keeps working; a failed restore is reported in the
/// returned error rather than silently leaving no shortcut bound.
pub fn register_dictation(app: &AppHandle, old: Option<&str>, new: &str) -> Result<(), String> {
    let shortcut = parse(new)?;
    if let Some(old_accel) = old {
        if let Ok(old_shortcut) = parse(old_accel) {
            if let Err(e) = app.global_shortcut().unregister(old_shortcut) {
                return Err(format!(
                    "Could not release current shortcut \"{old_accel}\": {e}"
                ));
            }
        }
    }
    if let Err(e) = on_dictation_shortcut(app, shortcut) {
        if let Some(old_accel) = old {
            if let Ok(old_shortcut) = parse(old_accel) {
                if let Err(restore_err) = on_dictation_shortcut(app, old_shortcut) {
                    return Err(format!(
                        "Could not register \"{new}\": {e}; restoring \"{old_accel}\" also failed — dictation shortcut is currently unbound: {restore_err}"
                    ));
                }
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
