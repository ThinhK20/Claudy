use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

pub const STORE_FILE: &str = "settings.json";

/// Single source of truth for provider ids — `secrets` and `ai` validate
/// against this list.
pub const PROVIDER_IDS: [&str; 5] =
    ["openai_compatible", "ollama", "anthropic", "gemini", "deepseek"];

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct ProviderSettings {
    pub base_url: String, // "" = provider's built-in default
    pub model: String,    // "" = provider's built-in default
    /// Models tried, in order, when `model` is overloaded. Empty = the
    /// provider's built-in chain (`ai::fallback::builtin_chain`).
    pub fallback_models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct AiSettings {
    pub active_provider: String, // one of PROVIDER_IDS
    /// Master switch for automatic model fallback. On, an overloaded model is
    /// transparently swapped for the next one in its chain; off restores the
    /// old single-shot behavior of surfacing the provider's error.
    pub model_fallback_enabled: bool,
    pub openai_compatible: ProviderSettings,
    pub ollama: ProviderSettings,
    pub anthropic: ProviderSettings,
    pub gemini: ProviderSettings,
    pub deepseek: ProviderSettings,
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            active_provider: "openai_compatible".into(),
            // On by default: losing a dictation prompt to a peak-hour 503 is a
            // worse surprise than being answered by a sibling model, and the
            // switch is always announced.
            model_fallback_enabled: true,
            openai_compatible: ProviderSettings::default(),
            ollama: ProviderSettings::default(),
            anthropic: ProviderSettings::default(),
            gemini: ProviderSettings::default(),
            deepseek: ProviderSettings::default(),
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
            "deepseek" => Ok(&self.deepseek),
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
    /// Spoken alternative to the shortcut. Lives in `wake.rs` next to the
    /// listener that reads it, like `DictationMode` does for dictation.
    pub wake_word: crate::wake::WakeWordSettings,
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
            wake_word: crate::wake::WakeWordSettings::default(),
        }
    }
}

/// Local usage log (Usage page). Recording is on by default: the log never
/// leaves this machine and holds metadata only — no prompt text, no selected
/// text, no answers. Turn it off or clear it from Settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct UsageSettings {
    pub enabled: bool,
    /// Events older than this are dropped on the next write. 0 = keep
    /// forever (a hard cap of `usage::MAX_EVENTS` still applies).
    pub retention_days: u32,
    /// Per-model price overrides for the cost estimate. Empty = use the
    /// bundled table.
    pub model_prices: Vec<crate::usage::ModelPrice>,
}

impl Default for UsageSettings {
    fn default() -> Self {
        Self { enabled: true, retention_days: 90, model_prices: Vec::new() }
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
    /// Hold-to-talk vs press-to-toggle. Read at press time, so a change
    /// applies immediately without re-registering the combo.
    pub dictation_mode: crate::dictation::DictationMode,
    pub keep_model_warm: bool,
    pub restore_clipboard: bool,
    pub auto_paste: bool,
    pub notifications_enabled: bool,
    pub start_minimized: bool,
    pub models_dir_override: String, // "" = default project models/ dir
    pub ai: AiSettings,
    pub assistant: AssistantSettings,
    pub usage: UsageSettings,
    /// Corrections applied to every finished transcript. Lives in
    /// `vocabulary.rs` next to the matcher that reads it.
    pub vocabulary: crate::vocabulary::VocabularySettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: "system".into(),
            language: "auto".into(),
            model: String::new(),
            mic_device: String::new(),
            dictation_shortcut: "Ctrl+Shift+D".into(),
            dictation_mode: crate::dictation::DictationMode::Hold,
            keep_model_warm: true,
            restore_clipboard: true,
            auto_paste: false,
            notifications_enabled: true,
            start_minimized: false,
            models_dir_override: String::new(),
            ai: AiSettings::default(),
            assistant: AssistantSettings::default(),
            usage: UsageSettings::default(),
            vocabulary: crate::vocabulary::VocabularySettings::default(),
        }
    }
}

/// Serializes every load-modify-save of the settings object. `update_settings`
/// reads the old settings, re-registers shortcuts against the difference, then
/// writes the whole object back; two overlapping writers would each act on a
/// stale "old" and the later save would silently undo the earlier one.
static STORE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Take the settings write lock. A poisoned lock is recovered: one panicked
/// update must not block every later settings change for the session.
pub fn lock_store() -> std::sync::MutexGuard<'static, ()> {
    STORE_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Read a stored JSON array one row at a time.
///
/// A row that no longer deserializes — a hand edit, a field written by a
/// different build — costs that one row, never the whole list. Shared by the
/// prompt and reminder stores; `usage::parse_rows` applies the same rule.
///
/// `Err` is reserved for a top-level shape that isn't an array. Callers must
/// read it as "leave the file alone" — never as "start from empty".
pub fn parse_rows<T: serde::de::DeserializeOwned>(
    value: serde_json::Value,
) -> Result<Vec<T>, String> {
    let rows: Vec<serde_json::Value> = serde_json::from_value(value).map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .filter_map(|row| serde_json::from_value(row).ok())
        .collect())
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
    crate::vocabulary::validate(&settings.vocabulary)?;
    // Held from the read of `old` to the save, and dropped before the
    // follow-up work below, none of which writes settings.
    let guard = lock_store();
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

    let wake_changed = old.assistant.wake_word != settings.assistant.wake_word;

    save(&app, &settings)?;
    drop(guard);

    // A changed reserved combo can free/claim a prompt binding, so reconcile.
    if dictation_changed || assistant_changed {
        crate::shortcuts::notify_sync_warnings(&app, &crate::shortcuts::sync_prompts(&app)?);
    }
    // Turning the wake word on/off, or changing the phrase or model, only
    // takes effect once the listener is restarted against the saved settings.
    if wake_changed || old.mic_device != settings.mic_device {
        crate::wake::restart(&app);
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
        assert!(
            !s.assistant.wake_word.enabled,
            "an always-on microphone must be opt-in"
        );
        assert!(s.restore_clipboard);
        assert_eq!(s.theme, "system");
        assert_eq!(s.dictation_shortcut, "Ctrl+Shift+D");
        assert_eq!(s.dictation_mode, crate::dictation::DictationMode::Hold);
    }

    #[test]
    fn settings_written_before_hold_to_talk_load_into_hold_mode() {
        // No migration: the field is simply absent in existing settings.json.
        let json = serde_json::json!({ "dictationShortcut": "Ctrl+Shift+D" });
        let s: Settings = serde_json::from_value(json).unwrap();
        assert_eq!(s.dictation_mode, crate::dictation::DictationMode::Hold);
    }

    #[test]
    fn dictation_mode_round_trips_camel_case() {
        let json = serde_json::json!({ "dictationMode": "toggle" });
        let s: Settings = serde_json::from_value(json).unwrap();
        assert_eq!(s.dictation_mode, crate::dictation::DictationMode::Toggle);
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["dictationMode"], "toggle");
    }

    #[test]
    fn an_unusable_dictation_mode_does_not_fail_the_whole_settings_load() {
        // A hand-edit typo must cost you the mode, not every other setting.
        for bad in [serde_json::json!("hodl"), serde_json::json!(true)] {
            let json = serde_json::json!({ "theme": "dark", "dictationMode": bad });
            let s: Settings = serde_json::from_value(json)
                .unwrap_or_else(|e| panic!("dictationMode {bad} broke the load: {e}"));
            assert_eq!(s.dictation_mode, crate::dictation::DictationMode::Hold);
            assert_eq!(s.theme, "dark", "the rest of the settings must survive");
        }
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
        assert!(v.get("dictationMode").is_some());
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
    fn settings_written_before_model_fallback_load_with_it_on_and_no_overrides() {
        // No migration: the fields are simply absent in existing settings.json,
        // and their absence must mean "use the built-in chain", not "never
        // fall back".
        let json = serde_json::json!({
            "ai": { "activeProvider": "gemini", "gemini": { "model": "gemini-2.5-pro" } }
        });
        let s: Settings = serde_json::from_value(json).unwrap();
        assert!(s.ai.model_fallback_enabled);
        assert!(s.ai.gemini.fallback_models.is_empty());
        assert_eq!(s.ai.gemini.model, "gemini-2.5-pro", "the rest must survive");
    }

    #[test]
    fn model_fallback_settings_round_trip_camel_case() {
        let json = serde_json::json!({
            "ai": {
                "modelFallbackEnabled": false,
                "gemini": { "fallbackModels": ["gemini-2.5-flash", "gemini-2.0-flash"] }
            }
        });
        let s: Settings = serde_json::from_value(json).unwrap();
        assert!(!s.ai.model_fallback_enabled);
        assert_eq!(s.ai.gemini.fallback_models, ["gemini-2.5-flash", "gemini-2.0-flash"]);

        let v = serde_json::to_value(&s).unwrap();
        assert!(v["ai"].get("modelFallbackEnabled").is_some());
        assert!(v["ai"].get("model_fallback_enabled").is_none());
        assert!(v["ai"]["gemini"].get("fallbackModels").is_some());
        assert!(v["ai"]["gemini"].get("fallback_models").is_none());
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
    fn settings_written_before_the_wake_word_load_with_it_off() {
        // No migration: the block is simply absent in existing settings.json,
        // and its absence must never turn the microphone on.
        let json = serde_json::json!({ "assistant": { "autoSpeak": false } });
        let s: Settings = serde_json::from_value(json).unwrap();
        assert_eq!(s.assistant.wake_word, crate::wake::WakeWordSettings::default());
        assert!(!s.assistant.wake_word.enabled);
    }

    #[test]
    fn wake_word_round_trips_nested_camel_case() {
        let json = serde_json::json!({
            "assistant": { "wakeWord": { "enabled": true, "phrase": "ok computer" } }
        });
        let s: Settings = serde_json::from_value(json).unwrap();
        assert!(s.assistant.wake_word.enabled);
        assert_eq!(s.assistant.wake_word.phrase, "ok computer");
        assert_eq!(s.assistant.wake_word.model, "tiny"); // missing -> default
        let v = serde_json::to_value(&s).unwrap();
        assert_eq!(v["assistant"]["wakeWord"]["phrase"], "ok computer");
        assert!(v["assistant"]["wakeWord"].get("sensitivity").is_some());
    }

    #[test]
    fn settings_written_before_usage_stats_load_with_recording_on_and_90_day_retention() {
        // No migration: the block is simply absent in existing settings.json.
        let s: Settings = serde_json::from_value(serde_json::json!({ "theme": "dark" })).unwrap();
        assert_eq!(s.usage, UsageSettings::default());
        assert!(s.usage.enabled);
        assert_eq!(s.usage.retention_days, 90);
        assert!(s.usage.model_prices.is_empty());
        assert_eq!(s.theme, "dark", "the rest of the settings must survive");
    }

    #[test]
    fn usage_settings_round_trip_camel_case_and_fill_missing_with_defaults() {
        let json = serde_json::json!({
            "usage": {
                "enabled": false,
                "modelPrices": [
                    { "model": "my-deployment", "inputPerMtok": 7.0, "outputPerMtok": 9.0 }
                ]
            }
        });
        let s: Settings = serde_json::from_value(json).unwrap();
        assert!(!s.usage.enabled);
        assert_eq!(s.usage.retention_days, 90, "missing nested field -> default");
        assert_eq!(s.usage.model_prices[0].model, "my-deployment");
        assert_eq!(s.usage.model_prices[0].output_per_mtok, 9.0);

        let v = serde_json::to_value(&s).unwrap();
        assert!(v["usage"].get("retentionDays").is_some());
        assert!(v["usage"].get("retention_days").is_none());
        assert!(v["usage"]["modelPrices"][0].get("inputPerMtok").is_some());
    }

    #[test]
    fn settings_written_before_custom_vocabulary_load_with_an_empty_rule_list() {
        // No migration: the block is simply absent in existing settings.json,
        // and its absence must leave transcription behaving exactly as before.
        let s: Settings = serde_json::from_value(serde_json::json!({ "theme": "dark" })).unwrap();
        assert_eq!(s.vocabulary, crate::vocabulary::VocabularySettings::default());
        assert!(s.vocabulary.rules.is_empty());
        assert!(s.vocabulary.enabled, "an empty list changes nothing, so leave the switch on");
        assert_eq!(s.theme, "dark", "the rest of the settings must survive");
    }

    #[test]
    fn vocabulary_round_trips_nested_camel_case() {
        let json = serde_json::json!({
            "vocabulary": { "rules": [{ "from": "get hub", "to": "GitHub", "matchCase": true }] }
        });
        let s: Settings = serde_json::from_value(json).unwrap();
        assert_eq!(s.vocabulary.rules[0].to, "GitHub");
        assert!(s.vocabulary.rules[0].match_case);
        assert!(s.vocabulary.enabled, "missing nested field -> default");

        let v = serde_json::to_value(&s).unwrap();
        assert!(v["vocabulary"]["rules"][0].get("matchCase").is_some());
        assert!(v["vocabulary"]["rules"][0].get("match_case").is_none());
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
