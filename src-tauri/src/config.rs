use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

pub const STORE_FILE: &str = "settings.json";

/// Single source of truth for provider ids — `secrets` and `ai` validate
/// against this list.
pub const PROVIDER_IDS: [&str; 4] = ["openai_compatible", "ollama", "anthropic", "gemini"];

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct ProviderSettings {
    pub base_url: String, // "" = provider's built-in default
    pub model: String,    // "" = provider's built-in default
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct AiSettings {
    pub active_provider: String, // one of PROVIDER_IDS
    pub openai_compatible: ProviderSettings,
    pub ollama: ProviderSettings,
    pub anthropic: ProviderSettings,
    pub gemini: ProviderSettings,
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            active_provider: "openai_compatible".into(),
            openai_compatible: ProviderSettings::default(),
            ollama: ProviderSettings::default(),
            anthropic: ProviderSettings::default(),
            gemini: ProviderSettings::default(),
        }
    }
}

impl AiSettings {
    pub fn provider(&self, id: &str) -> Result<&ProviderSettings, String> {
        match id {
            "openai_compatible" => Ok(&self.openai_compatible),
            "ollama" => Ok(&self.ollama),
            "anthropic" => Ok(&self.anthropic),
            "gemini" => Ok(&self.gemini),
            _ => Err(format!("Unknown AI provider \"{id}\"")),
        }
    }
}

/// Upper bound for the assistant's custom system prompt. Mirrored by
/// `MAX_SYSTEM_PROMPT_CHARS` in `src/components/assistant/system-prompt-editor.tsx`.
pub const MAX_SYSTEM_PROMPT_CHARS: usize = 10_000;

/// Quick-ask voice assistant settings (Phase 7). Nested under `Settings`
/// like `AiSettings`; missing fields fall back to `Default` via serde.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct AssistantSettings {
    pub shortcut: String,   // global combo that opens the ask popup
    pub tts_voice: String,  // Kokoro voice id, e.g. "af_heart"
    pub speech_speed: f32,  // 1.0 = normal
    pub volume: f32,        // 0.0–1.0
    pub auto_speak: bool,   // speak the answer automatically
    pub auto_web_search: bool,
    pub panel_close_secs: u32, // 0 = never auto-close
    pub keep_open_while_speaking: bool,
    pub custom_system_prompt: String, // "" = no system prompt (today's behavior)
}

impl AssistantSettings {
    /// The system prompt to send with assistant requests: trimmed, or `None`
    /// when unset/whitespace-only. Single seam for future prompt profiles,
    /// templates, or variable expansion.
    pub fn system_prompt(&self) -> Option<String> {
        let trimmed = self.custom_system_prompt.trim();
        if trimmed.is_empty() { None } else { Some(trimmed.to_string()) }
    }
}

impl Default for AssistantSettings {
    fn default() -> Self {
        Self {
            shortcut: "Ctrl+Shift+Space".into(),
            tts_voice: "af_heart".into(),
            speech_speed: 1.0,
            volume: 1.0,
            auto_speak: true,
            auto_web_search: false,
            panel_close_secs: 15,
            keep_open_while_speaking: true,
            custom_system_prompt: String::new(),
        }
    }
}

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
    pub ai: AiSettings,
    pub assistant: AssistantSettings,
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
            ai: AiSettings::default(),
            assistant: AssistantSettings::default(),
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
    // Length-only guard (the UI enforces the rest); anything stricter here
    // could start rejecting unrelated settings writes.
    if settings.assistant.custom_system_prompt.chars().count() > MAX_SYSTEM_PROMPT_CHARS {
        return Err(format!(
            "Custom system prompt exceeds {MAX_SYSTEM_PROMPT_CHARS} characters — shorten the prompt"
        ));
    }
    let old = load(&app)?;
    let dictation_changed = old.dictation_shortcut != settings.dictation_shortcut;
    let assistant_changed = old.assistant.shortcut != settings.assistant.shortcut;

    // Re-register any changed reserved combo BEFORE saving so a failed
    // registration surfaces as an error and the stored value stays truthful.
    if dictation_changed {
        crate::shortcuts::register_dictation(
            &app,
            Some(&old.dictation_shortcut),
            &settings.dictation_shortcut,
        )?;
    }
    if assistant_changed {
        crate::shortcuts::register_assistant(
            &app,
            Some(&old.assistant.shortcut),
            &settings.assistant.shortcut,
        )?;
    }

    save(&app, &settings)?;

    // A changed reserved combo can free/claim a prompt binding, so reconcile.
    if dictation_changed || assistant_changed {
        crate::shortcuts::notify_sync_warnings(&app, &crate::shortcuts::sync_prompts(&app)?);
    }
    Ok(())
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

    #[test]
    fn ai_defaults_to_openai_compatible_with_empty_endpoints() {
        let s = Settings::default();
        assert_eq!(s.ai.active_provider, "openai_compatible");
        assert_eq!(s.ai.ollama, ProviderSettings::default());
        assert!(s.ai.openai_compatible.base_url.is_empty());
    }

    #[test]
    fn ai_settings_round_trip_camel_case_and_fill_missing_with_defaults() {
        let json = serde_json::json!({
            "ai": { "activeProvider": "ollama", "ollama": { "baseUrl": "http://box:11434" } }
        });
        let s: Settings = serde_json::from_value(json).unwrap();
        assert_eq!(s.ai.active_provider, "ollama");
        assert_eq!(s.ai.ollama.base_url, "http://box:11434");
        assert_eq!(s.ai.ollama.model, ""); // missing nested field -> default
        assert_eq!(s.ai.anthropic, ProviderSettings::default());
        let v = serde_json::to_value(&s).unwrap();
        assert!(v["ai"]["openaiCompatible"].get("baseUrl").is_some());
    }

    #[test]
    fn assistant_defaults_are_sensible() {
        let s = Settings::default();
        assert_eq!(s.assistant.shortcut, "Ctrl+Shift+Space");
        assert_eq!(s.assistant.tts_voice, "af_heart");
        assert_eq!(s.assistant.speech_speed, 1.0);
        assert!(s.assistant.auto_speak);
        assert!(!s.assistant.auto_web_search, "web search must be opt-in");
        assert_eq!(s.assistant.panel_close_secs, 15);
        assert!(s.assistant.keep_open_while_speaking);
    }

    #[test]
    fn assistant_settings_round_trip_camel_case_and_fill_missing_with_defaults() {
        let json = serde_json::json!({
            "assistant": { "autoWebSearch": true, "ttsVoice": "am_adam" }
        });
        let s: Settings = serde_json::from_value(json).unwrap();
        assert!(s.assistant.auto_web_search);
        assert_eq!(s.assistant.tts_voice, "am_adam");
        assert_eq!(s.assistant.shortcut, "Ctrl+Shift+Space"); // missing -> default
        assert_eq!(s.assistant.custom_system_prompt, ""); // missing -> default
        let v = serde_json::to_value(&s).unwrap();
        assert!(v["assistant"].get("keepOpenWhileSpeaking").is_some());
        assert!(v["assistant"].get("panelCloseSecs").is_some());
        assert!(v["assistant"].get("customSystemPrompt").is_some());
    }

    #[test]
    fn custom_system_prompt_round_trips_camel_case() {
        let json = serde_json::json!({
            "assistant": { "customSystemPrompt": "Be brief." }
        });
        let s: Settings = serde_json::from_value(json).unwrap();
        assert_eq!(s.assistant.custom_system_prompt, "Be brief.");
    }

    #[test]
    fn system_prompt_is_none_when_empty_or_whitespace() {
        let mut a = AssistantSettings::default();
        assert_eq!(a.system_prompt(), None, "default must mean no system prompt");
        a.custom_system_prompt = "  \n\t ".into();
        assert_eq!(a.system_prompt(), None);
    }

    #[test]
    fn system_prompt_trims_ends_but_preserves_interior_line_breaks() {
        let a = AssistantSettings {
            custom_system_prompt: "  Answer in Markdown.\n\nBe concise. \n".into(),
            ..Default::default()
        };
        assert_eq!(a.system_prompt().unwrap(), "Answer in Markdown.\n\nBe concise.");
    }

    #[test]
    fn missing_assistant_block_deserializes_to_defaults() {
        let s: Settings = serde_json::from_value(serde_json::json!({ "theme": "dark" })).unwrap();
        assert_eq!(s.assistant, AssistantSettings::default());
    }

    #[test]
    fn provider_lookup_by_id_covers_all_four_and_rejects_unknown() {
        let s = Settings::default();
        for id in PROVIDER_IDS {
            assert!(s.ai.provider(id).is_ok(), "missing provider field for {id}");
        }
        let err = s.ai.provider("skynet").unwrap_err();
        assert!(err.contains("skynet"), "got: {err}");
    }
}
