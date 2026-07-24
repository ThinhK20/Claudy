use std::collections::HashMap;
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

/// Validate an accelerator string ("Ctrl+Shift+D", "CmdOrCtrl+Space", ...).
///
/// NOTE: parsing OK does not imply the combo can be REGISTERED on Windows.
/// `global-hotkey` parses some keys it then has no `VK_` code for — `Code::Fn`
/// is the live example — so registration still fails at `RegisterHotKey`. The
/// recorder (`src/lib/accelerator.ts`) only ever emits keys that both parse and
/// have a VK; keep any new key names in sync with that map.
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
        .on_shortcut(shortcut, |app, _shortcut, event| match event.state {
            // Dictation is the one combo that cares about the key coming back
            // up: hold-to-talk stops on release. The backend registers with
            // MOD_NOREPEAT, so holding still yields exactly ONE Pressed.
            ShortcutState::Pressed => crate::dictation::press(app),
            ShortcutState::Released => crate::dictation::release(app),
        })
        .map_err(|e| e.to_string())
}

fn on_assistant_shortcut(app: &AppHandle, shortcut: Shortcut) -> Result<(), String> {
    app.global_shortcut()
        .on_shortcut(shortcut, |app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                crate::assistant::toggle(app);
            }
        })
        .map_err(|e| e.to_string())
}

/// Live (re-)registration for one reserved combo: unregister `old` (if any),
/// register `new`. If `new` fails (another app owns the combo), restoring the
/// old binding is attempted so the feature keeps working; a failed restore is
/// reported in the returned error rather than silently leaving nothing bound.
/// `label` names the feature ("dictation" / "assistant") in error text.
fn register_reserved(
    app: &AppHandle,
    old: Option<&str>,
    new: &str,
    label: &str,
    register: fn(&AppHandle, Shortcut) -> Result<(), String>,
) -> Result<(), String> {
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
    if let Err(e) = register(app, shortcut) {
        if let Some(old_accel) = old {
            if let Ok(old_shortcut) = parse(old_accel) {
                if let Err(restore_err) = register(app, old_shortcut) {
                    return Err(format!(
                        "Could not register \"{new}\": {e}; restoring \"{old_accel}\" also failed — {label} shortcut is currently unbound: {restore_err}"
                    ));
                }
            }
        }
        return Err(format!("Could not register \"{new}\": {e}"));
    }
    Ok(())
}

/// Live (re-)registration of the dictation combo. See [`register_reserved`].
pub fn register_dictation(app: &AppHandle, old: Option<&str>, new: &str) -> Result<(), String> {
    register_reserved(app, old, new, "dictation", on_dictation_shortcut)
}

/// Live (re-)registration of the assistant combo. See [`register_reserved`].
pub fn register_assistant(app: &AppHandle, old: Option<&str>, new: &str) -> Result<(), String> {
    register_reserved(app, old, new, "assistant", on_assistant_shortcut)
}

/// Currently registered prompt shortcuts: accel string (as stored on the
/// prompt) -> prompt id. The registered handler closure captures only the
/// accel string and resolves the prompt id here at FIRE time, so pointing
/// an accel at a different prompt is just a map update.
#[derive(Default)]
pub struct PromptShortcuts(pub Mutex<HashMap<String, String>>);

/// Pure: which (accel, prompt_id) pairs SHOULD be registered, plus warnings
/// for prompts whose binding was skipped (invalid accelerator, reserved-combo
/// conflict, duplicate combo). `reserved` is a list of (label, accel) app
/// combos a prompt may not reuse (dictation + assistant). Comparison happens
/// on PARSED shortcuts so "Ctrl+G" and "Control+G" count as the same combo.
pub fn desired_prompt_bindings(
    list: &[crate::prompts::Prompt],
    reserved: &[(&str, &str)],
) -> (Vec<(String, String)>, Vec<String>) {
    let reserved: Vec<(&str, Shortcut)> = reserved
        .iter()
        .filter_map(|(label, accel)| parse(accel).ok().map(|s| (*label, s)))
        .collect();
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
        if let Some((label, _)) = reserved.iter().find(|(_, s)| *s == shortcut) {
            warnings.push(format!(
                "\"{}\": {} is already the {label} shortcut",
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
    let (desired, mut warnings) = desired_prompt_bindings(
        &prompts,
        &[
            ("dictation", &settings.dictation_shortcut),
            ("assistant", &settings.assistant.shortcut),
        ],
    );
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

/// Pure: does `accel` collide with any taken binding? Comparison happens on
/// PARSED shortcuts (same rule as `desired_prompt_bindings`), so
/// "Control+Shift+G" and "Ctrl+Shift+G" count as the same combo. Err =
/// `accel` itself is invalid; unparseable `taken` entries are skipped.
pub fn find_conflict(accel: &str, taken: &[(String, String)]) -> Result<Option<String>, String> {
    let shortcut = parse(accel)?;
    for (label, taken_accel) in taken {
        if parse(taken_accel).map(|s| s == shortcut).unwrap_or(false) {
            return Ok(Some(label.clone()));
        }
    }
    Ok(None)
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutCheck {
    pub ok: bool,
    pub message: String, // "" when ok
}

/// Live validation for the shortcut editors (spec line 79: conflicts are
/// surfaced in the UI). `exclude_prompt_id` = the prompt being edited;
/// `for_dictation` / `for_assistant` = that reserved combo itself is being
/// edited (then it is excluded from the conflict set, but the OTHER reserved
/// combo and all prompt shortcuts still count). A conflict is a WARNING — the
/// existing sync model skips conflicting bindings with a notification, so
/// this never blocks a save.
#[tauri::command]
pub fn check_shortcut(
    app: AppHandle,
    accel: String,
    exclude_prompt_id: Option<String>,
    for_dictation: bool,
    for_assistant: bool,
) -> Result<ShortcutCheck, String> {
    let settings = crate::config::load(&app)?;
    let prompts = crate::prompts::load(&app)?;
    let mut taken: Vec<(String, String)> = Vec::new();
    if !for_dictation {
        taken.push(("the dictation shortcut".into(), settings.dictation_shortcut));
    }
    if !for_assistant {
        taken.push(("the assistant shortcut".into(), settings.assistant.shortcut));
    }
    let exclude = exclude_prompt_id.unwrap_or_default();
    for p in &prompts {
        if p.enabled && !p.shortcut.trim().is_empty() && p.id != exclude {
            taken.push((format!("prompt \"{}\"", p.name), p.shortcut.clone()));
        }
    }
    Ok(match find_conflict(&accel, &taken) {
        Ok(None) => ShortcutCheck { ok: true, message: String::new() },
        Ok(Some(label)) => ShortcutCheck {
            ok: false,
            message: format!("Already used by {label}"),
        },
        Err(e) => ShortcutCheck { ok: false, message: e },
    })
}

/// Register the dictation shortcut and all prompt shortcuts from stored
/// settings/prompts. Shared by startup `init` and `resume_global_shortcuts`.
fn register_all(app: &AppHandle) {
    let settings = crate::config::load(app).unwrap_or_default();
    if let Err(e) = register_dictation(app, None, &settings.dictation_shortcut) {
        // Settings may be unreadable at this point: always show.
        crate::notify::send(app, true, &format!("Dictation shortcut unavailable: {e}"));
    }
    if let Err(e) = register_assistant(app, None, &settings.assistant.shortcut) {
        crate::notify::send(app, true, &format!("Assistant shortcut unavailable: {e}"));
    }

    match sync_prompts(app) {
        Ok(warnings) => notify_sync_warnings(app, &warnings),
        Err(e) => crate::notify::send(app, true, &format!("Prompt shortcuts unavailable: {e}")),
    }
}

/// Startup registration from settings. A conflict (combo owned by another
/// app) is NON-FATAL: notify and keep running — the tray toggle still works.
pub fn init(app: &AppHandle) {
    register_all(app);
}

/// Unregister every global shortcut while a ShortcutInput recorder is
/// capturing — registered combos are consumed by the OS (RegisterHotKey)
/// and never reach the webview, so capture needs them released.
/// Idempotent: `unregister_all` on an empty registry is a no-op. The
/// PromptShortcuts map MUST be cleared too — `sync_prompts` skips accels
/// already in the map, so stale entries would make resume silently skip
/// re-binding them.
#[tauri::command]
pub fn suspend_global_shortcuts(app: AppHandle) -> Result<(), String> {
    app.global_shortcut()
        .unregister_all()
        .map_err(|e| e.to_string())?;
    app.state::<PromptShortcuts>().0.lock().unwrap().clear();
    Ok(())
}

/// Re-register everything from stored settings/prompts after a capture
/// ends. Runs the same path as startup, so registration failures surface
/// as notifications, not errors. Safe to call without a prior suspend.
#[tauri::command]
pub fn resume_global_shortcuts(app: AppHandle) -> Result<(), String> {
    app.global_shortcut()
        .unregister_all()
        .map_err(|e| e.to_string())?;
    app.state::<PromptShortcuts>().0.lock().unwrap().clear();
    register_all(&app);
    Ok(())
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
    fn parses_standalone_function_key() {
        // The recorder emits bare F13–F24 for Fn-layer keys, no modifier.
        assert!(parse("F13").is_ok());
    }

    #[test]
    fn parses_standalone_media_keys() {
        // Media/volume keys are the other class the recorder allows bare.
        assert!(parse("MediaPlayPause").is_ok());
        assert!(parse("AudioVolumeUp").is_ok());
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
            &[("dictation", "Ctrl+Shift+D")],
        );
        assert_eq!(bindings, vec![("Ctrl+Shift+G".to_string(), "a".to_string())]);
        assert!(warnings.is_empty(), "got: {warnings:?}");
    }

    #[test]
    fn desired_bindings_warn_on_invalid_accelerators() {
        let (bindings, warnings) =
            desired_prompt_bindings(&[prompt("a", "Bad", "NotAKey+Q", true)], &[("dictation", "Ctrl+Shift+D")]);
        assert!(bindings.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("Bad"), "got: {}", warnings[0]);
    }

    #[test]
    fn desired_bindings_warn_on_dictation_conflict() {
        let (bindings, warnings) = desired_prompt_bindings(
            &[prompt("a", "Clash", "Ctrl+Shift+D", true)],
            &[("dictation", "Ctrl+Shift+D")],
        );
        assert!(bindings.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("dictation"), "got: {}", warnings[0]);
    }

    #[test]
    fn desired_bindings_warn_on_assistant_conflict() {
        // Both reserved combos block a prompt; the assistant one is named.
        let (bindings, warnings) = desired_prompt_bindings(
            &[prompt("a", "Clash", "Control+Shift+Space", true)],
            &[
                ("dictation", "Ctrl+Shift+D"),
                ("assistant", "Ctrl+Shift+Space"),
            ],
        );
        assert!(bindings.is_empty());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("assistant"), "got: {}", warnings[0]);
    }

    #[test]
    fn desired_bindings_dedupe_by_parsed_shortcut_first_wins() {
        // Different accel STRINGS, same parsed combo.
        let (bindings, warnings) = desired_prompt_bindings(
            &[
                prompt("a", "First", "Ctrl+Shift+G", true),
                prompt("b", "Second", "Control+Shift+G", true),
            ],
            &[("dictation", "Ctrl+Shift+D")],
        );
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].1, "a");
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("Second"), "got: {}", warnings[0]);
    }

    #[test]
    fn find_conflict_matches_equivalent_accelerator_strings() {
        let taken = vec![("prompt \"Fix\"".to_string(), "Control+Shift+G".to_string())];
        let hit = find_conflict("Ctrl+Shift+G", &taken).unwrap();
        assert_eq!(hit, Some("prompt \"Fix\"".to_string()));
    }

    #[test]
    fn find_conflict_is_none_for_a_free_combo() {
        let taken = vec![("the dictation shortcut".to_string(), "Ctrl+Shift+D".to_string())];
        assert_eq!(find_conflict("Ctrl+Shift+G", &taken).unwrap(), None);
    }

    #[test]
    fn find_conflict_rejects_invalid_accelerators() {
        assert!(find_conflict("NotAKey+Q", &[]).is_err());
    }

    #[test]
    fn find_conflict_ignores_unparseable_taken_entries() {
        let taken = vec![("junk".to_string(), "???".to_string())];
        assert_eq!(find_conflict("Ctrl+Shift+G", &taken).unwrap(), None);
    }
}
