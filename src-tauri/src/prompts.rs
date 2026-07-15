use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

pub const STORE_FILE: &str = "prompts.json";
const KEY: &str = "prompts";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Prompt {
    pub id: String,
    pub name: String,
    pub template: String, // may contain {{selected_text}} {{clipboard}} {{date}} {{time}}
    pub shortcut: String, // "" = no global shortcut assigned
    pub enabled: bool,
}

impl Default for Prompt {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            template: String::new(),
            shortcut: String::new(),
            enabled: true,
        }
    }
}

/// Seeded on first run so Phase 4 is E2E-verifiable before the Phase 5
/// prompt manager UI exists. Fixed id keeps re-seeding deterministic.
pub fn default_prompts() -> Vec<Prompt> {
    vec![Prompt {
        id: "default-fix-grammar".into(),
        name: "Fix grammar & spelling".into(),
        template: "Correct the grammar and spelling of the following text. \
                   Reply with only the corrected text, nothing else:\n\n{{selected_text}}"
            .into(),
        shortcut: "Ctrl+Shift+G".into(),
        enabled: true,
    }]
}

pub fn load(app: &AppHandle) -> Result<Vec<Prompt>, String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    match store.get(KEY) {
        Some(v) => serde_json::from_value(v).map_err(|e| e.to_string()),
        None => {
            let seeded = default_prompts();
            save_list(app, &seeded)?;
            Ok(seeded)
        }
    }
}

pub fn save_list(app: &AppHandle, prompts: &[Prompt]) -> Result<(), String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    let value = serde_json::to_value(prompts).map_err(|e| e.to_string())?;
    store.set(KEY, value);
    store.save().map_err(|e| e.to_string())
}

/// Pure list ops — persistence stays a thin wrapper around them.
pub fn upsert(mut list: Vec<Prompt>, prompt: Prompt) -> Vec<Prompt> {
    match list.iter().position(|p| p.id == prompt.id) {
        Some(i) => list[i] = prompt,
        None => list.push(prompt),
    }
    list
}

pub fn remove(list: Vec<Prompt>, id: &str) -> Vec<Prompt> {
    list.into_iter().filter(|p| p.id != id).collect()
}

#[derive(Debug, Default)]
pub struct TemplateVars {
    pub selected_text: String,
    pub clipboard: String,
    pub date: String,
    pub time: String,
}

/// Fixed placeholder set (spec line 48). Unknown {{tokens}} pass through
/// verbatim — a template typo stays visible instead of vanishing silently.
pub fn render(template: &str, v: &TemplateVars) -> String {
    template
        .replace("{{selected_text}}", &v.selected_text)
        .replace("{{clipboard}}", &v.clipboard)
        .replace("{{date}}", &v.date)
        .replace("{{time}}", &v.time)
}

/// Templates without {{selected_text}} skip the selection probe entirely.
pub fn needs_selection(template: &str) -> bool {
    template.contains("{{selected_text}}")
}

pub fn now_vars(selected_text: String, clipboard: String) -> TemplateVars {
    let now = chrono::Local::now();
    TemplateVars {
        selected_text,
        clipboard,
        date: now.format("%Y-%m-%d").to_string(),
        time: now.format("%H:%M").to_string(),
    }
}

#[tauri::command]
pub fn list_prompts(app: AppHandle) -> Result<Vec<Prompt>, String> {
    load(&app)
}

/// Upsert. Empty id = create (uuid assigned); returns the stored prompt.
#[tauri::command]
pub fn save_prompt(app: AppHandle, mut prompt: Prompt) -> Result<Prompt, String> {
    if prompt.name.trim().is_empty() {
        return Err("Prompt name must not be empty".into());
    }
    if prompt.template.trim().is_empty() {
        return Err("Prompt template must not be empty".into());
    }
    if !prompt.shortcut.trim().is_empty() {
        crate::shortcuts::parse(&prompt.shortcut)?; // reject bad accelerators at the boundary
    }
    if prompt.id.is_empty() {
        prompt.id = uuid::Uuid::new_v4().to_string();
    }
    let list = upsert(load(&app)?, prompt.clone());
    save_list(&app, &list)?;
    Ok(prompt)
}

#[tauri::command]
pub fn delete_prompt(app: AppHandle, id: String) -> Result<(), String> {
    let list = remove(load(&app)?, &id);
    save_list(&app, &list)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(id: &str, name: &str) -> Prompt {
        Prompt { id: id.into(), name: name.into(), ..Prompt::default() }
    }

    #[test]
    fn render_replaces_every_placeholder_including_repeats() {
        let vars = TemplateVars {
            selected_text: "SEL".into(),
            clipboard: "CLIP".into(),
            date: "2026-07-14".into(),
            time: "19:30".into(),
        };
        let out = render(
            "{{selected_text}} + {{clipboard}} on {{date}} at {{time}}; again {{selected_text}}",
            &vars,
        );
        assert_eq!(out, "SEL + CLIP on 2026-07-14 at 19:30; again SEL");
    }

    #[test]
    fn render_leaves_unknown_placeholders_untouched() {
        let vars = TemplateVars::default();
        assert_eq!(render("keep {{unknown}}", &vars), "keep {{unknown}}");
    }

    #[test]
    fn needs_selection_detects_the_placeholder() {
        assert!(needs_selection("Fix: {{selected_text}}"));
        assert!(!needs_selection("Summarize my clipboard: {{clipboard}}"));
    }

    #[test]
    fn now_vars_formats_date_and_time() {
        let v = now_vars("s".into(), "c".into());
        assert_eq!(v.date.len(), 10, "YYYY-MM-DD, got: {}", v.date);
        assert_eq!(v.time.len(), 5, "HH:MM, got: {}", v.time);
        assert_eq!(v.selected_text, "s");
        assert_eq!(v.clipboard, "c");
    }

    #[test]
    fn upsert_replaces_by_id_or_appends() {
        let list = vec![p("a", "A"), p("b", "B")];
        let updated = upsert(list.clone(), p("a", "A2"));
        assert_eq!(updated.len(), 2);
        assert_eq!(updated[0].name, "A2");
        let grown = upsert(list, p("c", "C"));
        assert_eq!(grown.len(), 3);
        assert_eq!(grown[2].id, "c");
    }

    #[test]
    fn remove_drops_only_the_matching_id() {
        let out = remove(vec![p("a", "A"), p("b", "B")], "a");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, "b");
    }

    #[test]
    fn prompt_serde_is_camel_case_and_enabled_defaults_true() {
        let json = serde_json::json!({ "id": "x", "name": "N", "template": "T {{selected_text}}" });
        let prompt: Prompt = serde_json::from_value(json).unwrap();
        assert!(prompt.enabled);
        assert_eq!(prompt.shortcut, "");
        let v = serde_json::to_value(&prompt).unwrap();
        assert!(v.get("enabled").is_some());
    }

    #[test]
    fn seed_prompt_is_enabled_with_a_valid_shortcut_and_uses_selection() {
        let seeds = default_prompts();
        assert_eq!(seeds.len(), 1);
        let seed = &seeds[0];
        assert!(seed.enabled);
        assert!(!seed.id.is_empty());
        assert!(crate::shortcuts::parse(&seed.shortcut).is_ok());
        assert!(needs_selection(&seed.template));
    }
}
