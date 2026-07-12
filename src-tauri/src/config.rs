use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

pub const STORE_FILE: &str = "settings.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub theme: String,              // "light" | "dark" | "system"
    pub language: String,           // whisper language code or "auto"
    pub model: String,              // model filename, "" = none selected
    pub mic_device: String,         // "" = system default
    pub dictation_shortcut: String, // e.g. "Ctrl+Shift+D"
    pub keep_model_warm: bool,
    pub restore_clipboard: bool,
    pub auto_paste: bool,
    pub notifications_enabled: bool,
    pub start_minimized: bool,
    pub models_dir_override: String, // "" = default project models/ dir
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "system".into(),
            language: "auto".into(),
            model: String::new(),
            mic_device: String::new(),
            dictation_shortcut: "Ctrl+Shift+D".into(),
            keep_model_warm: true,
            restore_clipboard: true,
            auto_paste: false,
            notifications_enabled: true,
            start_minimized: false,
            models_dir_override: String::new(),
        }
    }
}

pub fn load(app: &AppHandle) -> Result<Settings, String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    match store.get("settings") {
        Some(v) => serde_json::from_value(v).map_err(|e| e.to_string()),
        None => Ok(Settings::default()),
    }
}

pub fn save(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    let value = serde_json::to_value(settings).map_err(|e| e.to_string())?;
    store.set("settings", value);
    store.save().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Result<Settings, String> {
    load(&app)
}

#[tauri::command]
pub fn update_settings(app: AppHandle, settings: Settings) -> Result<(), String> {
    save(&app, &settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_privacy_safe() {
        let s = Settings::default();
        assert!(!s.auto_paste, "auto-paste must be opt-in");
        assert!(s.restore_clipboard);
        assert_eq!(s.theme, "system");
        assert_eq!(s.dictation_shortcut, "Ctrl+Shift+D");
    }

    #[test]
    fn deserializes_camel_case_and_fills_missing_fields_with_defaults() {
        let json = serde_json::json!({ "theme": "dark", "autoPaste": true });
        let s: Settings = serde_json::from_value(json).unwrap();
        assert_eq!(s.theme, "dark");
        assert!(s.auto_paste);
        assert_eq!(s.language, "auto"); // missing field -> default
    }

    #[test]
    fn serializes_to_camel_case() {
        let v = serde_json::to_value(Settings::default()).unwrap();
        assert!(v.get("dictationShortcut").is_some());
        assert!(v.get("dictation_shortcut").is_none());
    }
}
