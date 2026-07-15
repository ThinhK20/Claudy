use std::collections::HashMap;
use std::sync::Mutex;

use tauri::{AppHandle, Manager};
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

/// Currently registered prompt shortcuts: accel string (as stored on the
/// prompt) -> prompt id. The registered handler closure captures only the
/// accel string and resolves the prompt id here at FIRE time, so pointing
/// an accel at a different prompt is just a map update.
#[derive(Default)]
pub struct PromptShortcuts(pub Mutex<HashMap<String, String>>);

/// Pure: which (accel, prompt_id) pairs SHOULD be registered, plus warnings
/// for prompts whose binding was skipped (invalid accelerator, dictation
/// conflict, duplicate combo). Comparison happens on PARSED shortcuts so
/// "Ctrl+G" and "Control+G" count as the same combo.
pub fn desired_prompt_bindings(
    list: &[crate::prompts::Prompt],
    dictation_accel: &str,
) -> (Vec<(String, String)>, Vec<String>) {
    let dictation = parse(dictation_accel).ok();
    let mut bindings: Vec<(String, String)> = Vec::new();
    let mut taken: Vec<Shortcut> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    for p in list {
        if !p.enabled || p.shortcut.trim().is_empty() {
            continue;
        }
        let shortcut = match parse(&p.shortcut) {
            Ok(s) => s,
            Err(e) => {
                warnings.push(format!("\"{}\": {e}", p.name));
                continue;
            }
        };
        if Some(shortcut) == dictation {
            warnings.push(format!(
                "\"{}\": {} is already the dictation shortcut",
                p.name, p.shortcut
            ));
            continue;
        }
        if taken.contains(&shortcut) {
            warnings.push(format!(
                "\"{}\": {} is already used by another prompt",
                p.name, p.shortcut
            ));
            continue;
        }
        taken.push(shortcut);
        bindings.push((p.shortcut.trim().to_string(), p.id.clone()));
    }
    (bindings, warnings)
}

fn on_prompt_shortcut(app: &AppHandle, accel: &str) -> Result<(), String> {
    let shortcut = parse(accel)?;
    let accel_key = accel.to_string();
    app.global_shortcut()
        .on_shortcut(shortcut, move |app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                let prompt_id = app
                    .state::<PromptShortcuts>()
                    .0
                    .lock()
                    .unwrap()
                    .get(&accel_key)
                    .cloned();
                if let Some(id) = prompt_id {
                    crate::prompt_flow::trigger(app, &id);
                }
            }
        })
        .map_err(|e| e.to_string())
}

/// Reconcile registered prompt shortcuts with the prompt store. Called at
/// startup and after every prompt/settings mutation. Returns warnings for
/// skipped bindings; only a store/settings read failure is a hard error.
pub fn sync_prompts(app: &AppHandle) -> Result<Vec<String>, String> {
    let prompts = crate::prompts::load(app)?;
    let settings = crate::config::load(app)?;
    let (desired, mut warnings) = desired_prompt_bindings(&prompts, &settings.dictation_shortcut);
    let desired: HashMap<String, String> = desired.into_iter().collect();

    let state = app.state::<PromptShortcuts>();
    let mut current = state.0.lock().unwrap();

    // Unregister accels that should no longer be bound (before registering,
    // so an accel-string rename of the same combo frees it first).
    for accel in current.keys().cloned().collect::<Vec<_>>() {
        if !desired.contains_key(&accel) {
            if let Ok(s) = parse(&accel) {
                if let Err(e) = app.global_shortcut().unregister(s) {
                    warnings.push(format!("Could not release {accel}: {e}"));
                }
            }
            current.remove(&accel);
        }
    }

    // Register new accels; an id change on an existing accel is map-only.
    for (accel, id) in desired {
        if !current.contains_key(&accel) {
            if let Err(e) = on_prompt_shortcut(app, &accel) {
                warnings.push(format!("Could not register {accel}: {e}"));
                continue;
            }
        }
        current.insert(accel, id);
    }
    Ok(warnings)
}

/// Fan skipped-binding warnings out as notifications (always shown — these
/// are direct responses to a user action or startup issues worth knowing).
pub fn notify_sync_warnings(app: &AppHandle, warnings: &[String]) {
    for w in warnings {
        crate::notify::send(app, true, &format!("Prompt shortcut skipped — {w}"));
    }
}

/// Startup registration from settings. A conflict (combo owned by another
/// app) is NON-FATAL: notify and keep running — the tray toggle still works.
pub fn init(app: &AppHandle) {
    let settings = crate::config::load(app).unwrap_or_default();
    if let Err(e) = register_dictation(app, None, &settings.dictation_shortcut) {
        // Settings may be unreadable at this point: always show.
        crate::notify::send(app, true, &format!("Dictation shortcut unavailable: {e}"));
    }

    match sync_prompts(app) {
        Ok(warnings) => notify_sync_warnings(app, &warnings),
        Err(e) => crate::notify::send(app, true, &format!("Prompt shortcuts unavailable: {e}")),
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

    fn prompt(id: &str, name: &str, shortcut: &str, enabled: bool) -> crate::prompts::Prompt {
        crate::prompts::Prompt {
            id: id.into(),
            name: name.into(),
            shortcut: shortcut.into(),
            enabled,
            ..crate::prompts::Prompt::default()
        }
    }

    #[test]
    fn desired_bindings_include_only_enabled_prompts_with_shortcuts() {
        let (bindings, warnings) = desired_prompt_bindings(
            &[
                prompt("a", "A", "Ctrl+Shift+G", true),
                prompt("b", "B", "", true),              // no shortcut
                prompt("c", "C", "Ctrl+Shift+H", false), // disabled
            ],
            "Ctrl+Shift+D",
        );
        assert_eq!(bindings, vec![("Ctrl+Shift+G".to_string(), "a".to_string())]);
        assert!(warnings.is_empty(), "got: {warnings:?}");
    }

    #[test]
    fn desired_bindings_warn_on_invalid_accelerators() {
        let (bindings, warnings) =
            desired_prompt_bindings(&[prompt("a", "Bad", "NotAKey+Q", true)], "Ctrl+Shift+D");
        assert!(bindings.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("Bad"), "got: {}", warnings[0]);
    }

    #[test]
    fn desired_bindings_warn_on_dictation_conflict() {
        let (bindings, warnings) =
            desired_prompt_bindings(&[prompt("a", "Clash", "Ctrl+Shift+D", true)], "Ctrl+Shift+D");
        assert!(bindings.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("dictation"), "got: {}", warnings[0]);
    }

    #[test]
    fn desired_bindings_dedupe_by_parsed_shortcut_first_wins() {
        // Different accel STRINGS, same parsed combo.
        let (bindings, warnings) = desired_prompt_bindings(
            &[
                prompt("a", "First", "Ctrl+Shift+G", true),
                prompt("b", "Second", "Control+Shift+G", true),
            ],
            "Ctrl+Shift+D",
        );
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].1, "a");
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("Second"), "got: {}", warnings[0]);
    }
}
