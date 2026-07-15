use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::{ai, config, inject, notify, prompts, selection};

/// One prompt at a time — a second shortcut press while one is in flight
/// would race the clipboard probe and the result write.
#[derive(Default)]
pub struct PromptFlowState {
    busy: AtomicBool,
}

/// Pure: resolve a triggered prompt id against the stored list.
pub fn find_enabled(list: &[prompts::Prompt], id: &str) -> Result<prompts::Prompt, String> {
    match list.iter().find(|p| p.id == id) {
        Some(p) if p.enabled => Ok(p.clone()),
        Some(p) => Err(format!("Prompt \"{}\" is disabled", p.name)),
        None => Err(format!("Prompt \"{id}\" no longer exists")),
    }
}

/// THE entry point — shortcut handler (Task 8) and `run_prompt` command both
/// call this. Runs on the caller's thread: only flips the busy flag and
/// spawns, never blocks (same contract as `dictation::toggle`).
pub fn trigger(app: &AppHandle, prompt_id: &str) {
    if app
        .state::<PromptFlowState>()
        .busy
        .swap(true, Ordering::SeqCst)
    {
        let enabled = config::load(app)
            .map(|s| s.notifications_enabled)
            .unwrap_or(true);
        notify::send(app, enabled, "A prompt is already running — wait for it to finish");
        return;
    }
    let app = app.clone();
    let prompt_id = prompt_id.to_string();
    tauri::async_runtime::spawn(async move {
        run(&app, &prompt_id).await;
        app.state::<PromptFlowState>()
            .busy
            .store(false, Ordering::SeqCst);
    });
}

/// Spec flow (line 53): shortcut -> read selection -> empty? notify+abort ->
/// render -> provider -> result to clipboard -> notification. Every exit
/// path notifies — no silent failures (spec line 80).
async fn run(app: &AppHandle, prompt_id: &str) {
    let settings = match config::load(app) {
        Ok(s) => s,
        Err(e) => {
            notify::send(app, true, &format!("Could not load settings: {e}"));
            return;
        }
    };
    let notif = settings.notifications_enabled;

    let prompt = match prompts::load(app).and_then(|list| find_enabled(&list, prompt_id)) {
        Ok(p) => p,
        Err(e) => {
            notify::send(app, notif, &e);
            return;
        }
    };

    // Probe the selection only when the template needs it — the probe costs
    // ~300 ms and briefly touches the clipboard.
    let (selected, clipboard) = if prompts::needs_selection(&prompt.template) {
        let probe_app = app.clone();
        let probed = tauri::async_runtime::spawn_blocking(move || selection::read(&probe_app))
            .await
            .map_err(|e| e.to_string())
            .and_then(|r| r);
        match probed {
            Ok(s) if s.text.trim().is_empty() => {
                notify::send(app, notif, &format!("\"{}\": no text selected", prompt.name));
                return;
            }
            Ok(s) => (s.text, s.clipboard),
            Err(e) => {
                notify::send(app, notif, &format!("Could not read selection: {e}"));
                return;
            }
        }
    } else {
        (String::new(), app.clipboard().read_text().unwrap_or_default())
    };

    let rendered = prompts::render(&prompt.template, &prompts::now_vars(selected, clipboard));

    // Provider calls take seconds; without this the user stares at nothing.
    notify::send(app, notif, &format!("Running \"{}\"…", prompt.name));

    let result = match ai::complete(app, &rendered).await {
        Ok(r) => r,
        Err(e) => {
            notify::send(app, notif, &format!("\"{}\" failed: {e}", prompt.name));
            return;
        }
    };

    // Result -> clipboard. The original selection is never overwritten;
    // auto-paste (opt-in, default off — spec line 58) is the one exception.
    if settings.auto_paste {
        let paste_app = app.clone();
        let text = result.clone();
        // restore_clipboard=false on purpose: the result must STAY on the
        // clipboard even when auto-pasted.
        let pasted = tauri::async_runtime::spawn_blocking(move || {
            inject::insert_text(&paste_app, &text, false)
        })
        .await
        .map_err(|e| e.to_string())
        .and_then(|r| r);
        match pasted {
            Ok(()) => {
                notify::send(app, notif, &format!("\"{}\" done — result pasted", prompt.name))
            }
            Err(e) => notify::send(
                app,
                notif,
                &format!("\"{}\": result copied, but auto-paste failed: {e}", prompt.name),
            ),
        }
    } else {
        match app.clipboard().write_text(result) {
            Ok(()) => notify::send(
                app,
                notif,
                &format!("\"{}\" done — result copied to clipboard", prompt.name),
            ),
            Err(e) => notify::send(
                app,
                notif,
                &format!("\"{}\" succeeded but the clipboard write failed: {e}", prompt.name),
            ),
        }
    }
}

/// Manual/E2E trigger and Phase 5's "run now" button.
#[tauri::command]
pub fn run_prompt(app: AppHandle, id: String) {
    trigger(&app, &id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prompts::Prompt;

    fn p(id: &str, name: &str, enabled: bool) -> Prompt {
        Prompt { id: id.into(), name: name.into(), enabled, ..Prompt::default() }
    }

    #[test]
    fn find_enabled_returns_the_matching_enabled_prompt() {
        let list = vec![p("a", "A", true), p("b", "B", true)];
        assert_eq!(find_enabled(&list, "b").unwrap().name, "B");
    }

    #[test]
    fn find_enabled_rejects_disabled_prompts_by_name() {
        let err = find_enabled(&[p("a", "Fix it", false)], "a").unwrap_err();
        assert!(err.contains("Fix it") && err.contains("disabled"), "got: {err}");
    }

    #[test]
    fn find_enabled_rejects_unknown_ids() {
        let err = find_enabled(&[], "ghost").unwrap_err();
        assert!(err.contains("ghost"), "got: {err}");
    }
}
