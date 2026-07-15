# Phase 4 — AI Layer Implementation Plan

## Context

Phases 1–3 are complete and merged to `main` (scaffold/shell, audio+STT, dictation E2E — spec's first success criterion met). Phase 4 of the spec (`docs/superpowers/specs/2026-07-12-claudy-ai-assistant-design.md` line 93) is the **AI layer**: `AiProvider` trait + 4 providers, prompt engine, selection reading, notifications — success criterion: **prompt shortcuts work** (select text anywhere → global shortcut → AI result on the clipboard + notification).

On approval, this plan is saved verbatim to `docs/superpowers/plans/2026-07-14-phase-4-ai-layer.md` (the established convention), an SDD progress ledger is started at `.superpowers/sdd/progress.md`, and tasks execute one-by-one with review between tasks — same workflow as Phase 3.

**Key design decisions (for your review):**
- **Providers are pure request-builders/response-parsers** (`build_request` / `parse_response`) with one shared `send()` executor — request construction and parsing are unit-testable without network; one `httpmock` round-trip test covers the executor (spec: "provider request construction (mock HTTP server)").
- **API keys in Windows Credential Manager** via `keyring` v3 (`windows-native` feature), never JSON — spec's locked decision. Tests use keyring's built-in mock store.
- **Minimal Providers page ships now** (active provider, base URL/model, API-key entry, connection test) — without it there is no non-hostile way to enter an API key, and the phase can't be E2E-verified. Full provider settings polish stays in Phase 5.
- **Prompt manager UI stays in Phase 5.** Phase 4 seeds one default prompt ("Fix grammar & spelling", `Ctrl+Shift+G`, enabled) so the success criterion is verifiable; CRUD exists as commands.
- **Defaults live in provider files**, not config: empty `baseUrl`/`model` in settings means "use the provider's default" (`https://api.openai.com/v1` + `gpt-4o-mini`, `http://localhost:11434` + `llama3.2`, `https://api.anthropic.com` + `claude-sonnet-5`, `https://generativelanguage.googleapis.com` + `gemini-2.5-flash`). Base URLs are configurable for all four (proxies/local servers).

---

# The plan (saved to `docs/superpowers/plans/2026-07-14-phase-4-ai-layer.md` at execution start)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** AI prompt shortcuts that work system-wide: select text in any app → press a prompt's global shortcut → the rendered prompt runs against the configured AI provider → result lands on the clipboard with a success notification (opt-in auto-paste). *Second success criterion of the spec.*

**Architecture:** Five new Rust modules — `ai/` (provider trait + `openai_compatible`, `ollama`, `anthropic`, `gemini` — one file per provider), `secrets` (API keys via OS credential store), `prompts` (CRUD + `prompts.json` + template rendering), `selection` (clipboard-probe selection reading), `notify` (preference-respecting notifications, extracted from dictation), plus `prompt_flow` (orchestration, mirroring `dictation.rs`) and a prompt-shortcut registry extension in `shortcuts.rs`. Frontend gains a typed `ai-api.ts` and a minimal Providers page.

**Tech Stack:** `reqwest` (already present; add `json` feature), `keyring` 3 (new — Windows Credential Manager), `chrono` (new — `{{date}}`/`{{time}}`), `uuid` (new — prompt ids), `httpmock` (new dev-dep). No new Tauri plugins, **no capability changes** (clipboard/notification/global-shortcut already granted to both windows).

**Spec:** `docs/superpowers/specs/2026-07-12-claudy-ai-assistant-design.md`
**Roadmap context:** Phase 4 of 6 — "AI layer — provider trait + 4 providers, prompt engine, selection reading, notifications. *Prompt shortcuts work.*" Out of scope: prompt manager UI, shortcut-editor UI, import/export, provider-settings polish (all Phase 5); packaging/theme/autostart (Phase 6).

## Global Constraints

- Windows 11 is the dev/verification target; keep code cross-platform-shaped (no `#[cfg(windows)]` unless unavoidable).
- Rust-core monolith: all logic in Rust; the webview is purely presentational (spec line 23).
- **API keys go to the OS credential store only (`keyring`), never into JSON** (spec line 26). `settings.json` / `prompts.json` must never contain a key.
- Zero telemetry; the only network traffic is to user-configured AI providers (spec line 72).
- Prompt results go to the clipboard; **the original selection is never overwritten** — `auto_paste` is opt-in, default off (spec line 58).
- No silent failures: every user-triggered action ends in visible success or visible error (spec line 80).
- New provider = one new file under `src-tauri/src/ai/` (spec line 49).
- All frontend-visible Rust types use `#[serde(default, rename_all = "camelCase")]` (established pattern in `config.rs`).
- Run Rust commands from PowerShell (`cargo` is not on Git Bash PATH). Gates: `cd src-tauri; cargo test` all green, `npx tsc --noEmit` clean.
- Commit format: `<type>: <description>`, no attribution footer (globally disabled).

## Existing interfaces you will consume (already implemented — do not modify unless a task says so)

- `config::load(app) -> Result<Settings, String>` / `config::save(app, &Settings)` — store-plugin backed; `update_settings` command already handles dictation-shortcut re-registration.
- `inject::insert_text(app, text, restore_clipboard) -> Result<(), String>` — blocking clipboard-paste (~250 ms sleeps), call via `spawn_blocking`. Its private `send_paste()` becomes the shared `send_ctrl_key(char)` in Task 6.
- `shortcuts::parse(accel) -> Result<Shortcut, String>` — pure accelerator validation (works in unit tests, no Tauri runtime). `shortcuts::register_dictation`, `shortcuts::init` — dictation registration with rollback.
- `dictation.rs` — the orchestration pattern to mirror: sync `Mutex` for instant decisions, `tokio::sync::Mutex` op-guard, `spawn` + `spawn_blocking`, notify-on-every-failure. Its private `notify()` helper moves to `notify::send` in Task 6.
- `tauri_plugin_clipboard_manager::ClipboardExt` — `app.clipboard().read_text()/write_text()`.
- Event `"navigate"` + `MainApp.tsx` listener — deep-links the main window to a page (`"providers"` is already a registered `PageKey`).
- Frontend patterns: `src/lib/dictation-api.ts` (typed invoke wrappers), `src/lib/settings-store.ts` (zustand + optimistic `update`), shadcn components in `src/components/ui/` (button, input, label, select, switch, badge, card, separator).
- `tauri::async_runtime::spawn` / `spawn_blocking`; commands may be `async`.

---

### Task 1: `config.rs` — AI provider settings schema (TDD)

**Files:**
- Modify: `src-tauri/src/config.rs` (new types + `Settings.ai` field)
- Modify: `src/lib/settings-store.ts` (mirror the new types)

**Interfaces:**
- Consumes: nothing new.
- Produces: `config::PROVIDER_IDS: [&str; 4]`; `ProviderSettings { base_url: String, model: String }`; `AiSettings { active_provider: String, openai_compatible/ollama/anthropic/gemini: ProviderSettings }` with `AiSettings::provider(&self, id: &str) -> Result<&ProviderSettings, String>`; `Settings.ai: AiSettings`. Tasks 2–4 and 9 rely on these exact names. Empty `base_url`/`model` mean "use the provider's built-in default" (Task 3's `or_default`).

- [ ] **Step 1: Write the failing tests**

Append inside the existing `mod tests` in `src-tauri/src/config.rs`:

```rust
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
    fn provider_lookup_by_id_covers_all_four_and_rejects_unknown() {
        let s = Settings::default();
        for id in PROVIDER_IDS {
            assert!(s.ai.provider(id).is_ok(), "missing provider field for {id}");
        }
        let err = s.ai.provider("skynet").unwrap_err();
        assert!(err.contains("skynet"), "got: {err}");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run (PowerShell): `cd src-tauri; cargo test config`
Expected: FAIL to compile — `PROVIDER_IDS`, `ProviderSettings`, `ai` field not found.

- [ ] **Step 3: Write the implementation**

In `src-tauri/src/config.rs`, add above `pub struct Settings`:

```rust
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
```

Add to `Settings` (after `models_dir_override`): `pub ai: AiSettings,` — and to its `Default` impl: `ai: AiSettings::default(),`.

Mirror in `src/lib/settings-store.ts` (above `interface Settings`, and add the `ai` field to `Settings`):

```typescript
export type ProviderId = "openai_compatible" | "ollama" | "anthropic" | "gemini";

export interface ProviderSettings {
  baseUrl: string;
  model: string;
}

export interface AiSettings {
  activeProvider: ProviderId;
  openaiCompatible: ProviderSettings;
  ollama: ProviderSettings;
  anthropic: ProviderSettings;
  gemini: ProviderSettings;
}
```

```typescript
export interface Settings {
  // ...existing fields unchanged...
  ai: AiSettings;
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri; cargo test` → 3 new tests pass, all suites green. Then `npx tsc --noEmit` → clean.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/config.rs src/lib/settings-store.ts
git commit -m "feat: add AI provider settings schema"
```

---

### Task 2: `secrets.rs` — API keys in the OS credential store (TDD)

**Files:**
- Modify: `src-tauri/Cargo.toml` (add `keyring`)
- Create: `src-tauri/src/secrets.rs`
- Modify: `src-tauri/src/lib.rs` (module decl + 3 commands)

**Interfaces:**
- Consumes: `config::PROVIDER_IDS` (Task 1).
- Produces: `secrets::set(provider_id: &str, key: &str) -> Result<(), String>` (empty/whitespace key = delete), `secrets::get(provider_id: &str) -> Result<Option<String>, String>` (`None` = no key stored), `secrets::delete(provider_id: &str) -> Result<(), String>` (idempotent); commands `set_api_key(provider, key)`, `has_api_key(provider) -> bool`, `delete_api_key(provider)`. Task 3's `complete_with` and Task 9's UI rely on these.

- [ ] **Step 1: Add the dependency**

In `src-tauri/Cargo.toml` `[dependencies]` (after `enigo = "0.6"`). keyring v3 compiles the platform store only when its feature is enabled; with no platform feature (or in unit tests via the builder override) it falls back to the built-in mock store:

```toml
keyring = { version = "3", features = ["windows-native", "apple-native", "sync-secret-service"] }
```

Run: `cd src-tauri; cargo build` — compiles clean.

- [ ] **Step 2: Write the failing tests**

Create `src-tauri/src/secrets.rs` with only the test module, and add `mod secrets;` to `lib.rs` (alphabetical, after `mod overlay;`):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// Swap in keyring's in-memory mock store — process-global, so every
    /// test installs it; tests use DISTINCT provider ids to stay isolated
    /// under parallel execution.
    fn use_mock_store() {
        keyring::set_default_credential_builder(keyring::mock::default_credential_builder());
    }

    #[test]
    fn set_then_get_round_trips() {
        use_mock_store();
        set("ollama", "sk-test-123").unwrap();
        assert_eq!(get("ollama").unwrap(), Some("sk-test-123".into()));
    }

    #[test]
    fn get_without_a_stored_key_is_none_not_an_error() {
        use_mock_store();
        assert_eq!(get("gemini").unwrap(), None);
    }

    #[test]
    fn empty_key_deletes_and_delete_is_idempotent() {
        use_mock_store();
        set("anthropic", "sk-x").unwrap();
        set("anthropic", "   ").unwrap(); // whitespace-only = delete
        assert_eq!(get("anthropic").unwrap(), None);
        delete("anthropic").unwrap(); // nothing stored -> still Ok
    }

    #[test]
    fn unknown_provider_is_rejected_before_touching_the_store() {
        use_mock_store();
        let err = set("skynet", "k").unwrap_err();
        assert!(err.contains("skynet"), "got: {err}");
        assert!(get("skynet").is_err());
        assert!(delete("skynet").is_err());
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cd src-tauri; cargo test secrets`
Expected: FAIL to compile — `set`, `get`, `delete` not found.

- [ ] **Step 4: Write the implementation**

Prepend to `src-tauri/src/secrets.rs`:

```rust
use keyring::Entry;

/// Windows Credential Manager entry name: "Claudy — AI provider API keys".
const SERVICE: &str = "com.claudy.app";

fn entry(provider_id: &str) -> Result<Entry, String> {
    if !crate::config::PROVIDER_IDS.contains(&provider_id) {
        return Err(format!("Unknown AI provider \"{provider_id}\""));
    }
    Entry::new(SERVICE, &format!("provider:{provider_id}"))
        .map_err(|e| format!("Credential store unavailable: {e}"))
}

/// Store an API key. An empty/whitespace key means "remove it" — the UI's
/// clear action and save-empty-field collapse to one path, and no empty
/// strings ever land in the credential store.
pub fn set(provider_id: &str, key: &str) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() {
        return delete(provider_id);
    }
    entry(provider_id)?
        .set_password(key)
        .map_err(|e| format!("Could not store API key: {e}"))
}

/// `None` = no key stored (keyring `NoEntry` is not an error for us).
pub fn get(provider_id: &str) -> Result<Option<String>, String> {
    match entry(provider_id)?.get_password() {
        Ok(k) => Ok(Some(k)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("Could not read API key: {e}")),
    }
}

pub fn delete(provider_id: &str) -> Result<(), String> {
    match entry(provider_id)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("Could not delete API key: {e}")),
    }
}

/// The key itself is NEVER returned to the webview — only whether one exists.
#[tauri::command]
pub fn has_api_key(provider: String) -> Result<bool, String> {
    Ok(get(&provider)?.is_some())
}

#[tauri::command]
pub fn set_api_key(provider: String, key: String) -> Result<(), String> {
    set(&provider, &key)
}

#[tauri::command]
pub fn delete_api_key(provider: String) -> Result<(), String> {
    delete(&provider)
}
```

Register in `lib.rs` `invoke_handler` (after `config::update_settings,`):

```rust
            secrets::set_api_key,
            secrets::has_api_key,
            secrets::delete_api_key,
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd src-tauri; cargo test`
Expected: 4 new `secrets` tests pass (mock store — no real Credential Manager writes); all suites green.

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/secrets.rs src-tauri/src/lib.rs
git commit -m "feat: store AI provider API keys in the OS credential store"
```

---

### Task 3: `ai/` core — provider trait, executor, `openai_compatible` (TDD)

**Files:**
- Modify: `src-tauri/Cargo.toml` (reqwest `json` feature; dev-deps `httpmock`, tokio test features)
- Create: `src-tauri/src/ai/mod.rs`
- Create: `src-tauri/src/ai/openai_compatible.rs`
- Modify: `src-tauri/src/lib.rs` (module decl)

**Interfaces:**
- Consumes: `config::{load, ProviderSettings, PROVIDER_IDS}` (Task 1), `secrets::get` (Task 2).
- Produces: trait `AiProvider { id() -> &'static str, requires_api_key() -> bool, build_request(cfg: &ProviderSettings, api_key: Option<&str>, prompt: &str) -> Result<HttpRequest, String>, parse_response(body: &str) -> Result<String, String> }`; `HttpRequest { url: String, headers: Vec<(&'static str, String)>, body: serde_json::Value }`; `ai::provider(id) -> Result<&'static dyn AiProvider, String>`; `ai::send(req) -> Result<String, String>` (async, 60 s timeout, friendly error mapping); `ai::complete_with(app, provider_id, prompt)` and `ai::complete(app, prompt)` (active provider) — both async `Result<String, String>`; `ai::or_default(value, default) -> &str`. Task 4 adds three providers to the registry + `test_provider`; Task 7 calls `complete`.

- [ ] **Step 1: Update dependencies**

In `src-tauri/Cargo.toml`, change the reqwest line and add dev-dependencies (httpmock needs a tokio test runtime):

```toml
reqwest = { version = "0.12", features = ["stream", "json"] }
```

```toml
[dev-dependencies]
httpmock = "0.7"
tokio = { version = "1", features = ["macros", "rt"] }
```

- [ ] **Step 2: Write the failing tests**

Create `src-tauri/src/ai/openai_compatible.rs` with only its test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProviderSettings;

    fn cfg(base_url: &str, model: &str) -> ProviderSettings {
        ProviderSettings { base_url: base_url.into(), model: model.into() }
    }

    #[test]
    fn builds_chat_completions_request_with_bearer_auth() {
        let req = OpenAiCompatible
            .build_request(&cfg("https://api.openai.com/v1", "gpt-4o"), Some("sk-k"), "Hi")
            .unwrap();
        assert_eq!(req.url, "https://api.openai.com/v1/chat/completions");
        assert!(req.headers.contains(&("authorization", "Bearer sk-k".into())));
        assert_eq!(req.body["model"], "gpt-4o");
        assert_eq!(req.body["messages"][0]["role"], "user");
        assert_eq!(req.body["messages"][0]["content"], "Hi");
    }

    #[test]
    fn trailing_slash_and_missing_key_are_handled() {
        // Local OpenAI-style servers (LM Studio, llama.cpp) run keyless.
        let req = OpenAiCompatible
            .build_request(&cfg("http://localhost:1234/v1/", "m"), None, "Hi")
            .unwrap();
        assert_eq!(req.url, "http://localhost:1234/v1/chat/completions");
        assert!(req.headers.is_empty());
    }

    #[test]
    fn empty_config_falls_back_to_provider_defaults() {
        let req = OpenAiCompatible.build_request(&cfg("", "  "), None, "Hi").unwrap();
        assert_eq!(req.url, format!("{DEFAULT_BASE_URL}/chat/completions"));
        assert_eq!(req.body["model"], DEFAULT_MODEL);
    }

    #[test]
    fn parses_the_first_choice_and_trims_it() {
        let body = r#"{"choices":[{"message":{"role":"assistant","content":"  Hello  "}}]}"#;
        assert_eq!(OpenAiCompatible.parse_response(body).unwrap(), "Hello");
    }

    #[test]
    fn unexpected_response_shape_is_a_readable_error() {
        assert!(OpenAiCompatible.parse_response("{}").is_err());
        assert!(OpenAiCompatible.parse_response("not json").is_err());
    }
}
```

Create `src-tauri/src/ai/mod.rs` with declarations the tests need plus its own test module (add `mod ai;` to `lib.rs` after `mod audio;`):

```rust
pub mod openai_compatible;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn friendly_error_maps_auth_rate_limit_and_extracts_nested_message() {
        let auth = friendly_http_error(401, r#"{"error":{"message":"bad key"}}"#);
        assert!(auth.contains("Authentication failed") && auth.contains("bad key"), "got: {auth}");
        let rate = friendly_http_error(429, r#"{"message":"slow down"}"#);
        assert!(rate.contains("Rate limited") && rate.contains("slow down"), "got: {rate}");
        let other = friendly_http_error(500, "boom");
        assert!(other.contains("500") && other.contains("boom"), "got: {other}");
    }

    #[test]
    fn registry_resolves_known_ids_and_rejects_unknown() {
        assert_eq!(provider("openai_compatible").unwrap().id(), "openai_compatible");
        assert!(provider("skynet").is_err());
    }

    #[tokio::test]
    async fn send_round_trips_against_a_mock_openai_server() {
        let server = httpmock::MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(httpmock::Method::POST)
                .path("/v1/chat/completions")
                .header("authorization", "Bearer test-key");
            then.status(200).json_body(serde_json::json!({
                "choices": [{ "message": { "role": "assistant", "content": "OK" } }]
            }));
        });
        let cfg = crate::config::ProviderSettings {
            base_url: format!("{}/v1", server.base_url()),
            model: "m".into(),
        };
        let p = provider("openai_compatible").unwrap();
        let req = p.build_request(&cfg, Some("test-key"), "ping").unwrap();
        let body = send(req).await.unwrap();
        assert_eq!(p.parse_response(&body).unwrap(), "OK");
        mock.assert();
    }

    #[tokio::test]
    async fn send_surfaces_auth_failures_with_the_provider_message() {
        let server = httpmock::MockServer::start();
        server.mock(|when, then| {
            when.method(httpmock::Method::POST).path("/v1/chat/completions");
            then.status(401)
                .json_body(serde_json::json!({ "error": { "message": "invalid api key" } }));
        });
        let cfg = crate::config::ProviderSettings {
            base_url: format!("{}/v1", server.base_url()),
            model: "m".into(),
        };
        let p = provider("openai_compatible").unwrap();
        let req = p.build_request(&cfg, Some("bad"), "ping").unwrap();
        let err = send(req).await.unwrap_err();
        assert!(err.contains("Authentication failed") && err.contains("invalid api key"), "got: {err}");
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cd src-tauri; cargo test ai`
Expected: FAIL to compile — trait, `HttpRequest`, `provider`, `send`, `friendly_http_error` not found.

- [ ] **Step 4: Write the implementation**

Fill `src-tauri/src/ai/mod.rs` (above its test module):

```rust
use tauri::AppHandle;

/// One prompt round-trip must finish inside this (spec: timeout is a
/// reportable failure, not a hang).
pub const REQUEST_TIMEOUT_SECS: u64 = 60;

/// A fully built provider HTTP call — pure data, so request construction
/// is unit-testable without any network (spec testing requirement).
pub struct HttpRequest {
    pub url: String,
    pub headers: Vec<(&'static str, String)>,
    pub body: serde_json::Value,
}

/// Adding a provider = one new file implementing this + one registry line.
pub trait AiProvider: Sync {
    fn id(&self) -> &'static str;
    fn requires_api_key(&self) -> bool;
    fn build_request(
        &self,
        cfg: &crate::config::ProviderSettings,
        api_key: Option<&str>,
        prompt: &str,
    ) -> Result<HttpRequest, String>;
    /// Extract the completion text from a 2xx response body.
    fn parse_response(&self, body: &str) -> Result<String, String>;
}

pub fn provider(id: &str) -> Result<&'static dyn AiProvider, String> {
    match id {
        "openai_compatible" => Ok(&openai_compatible::OpenAiCompatible),
        // Task 4 adds: "ollama", "anthropic", "gemini"
        _ => Err(format!("Unknown AI provider \"{id}\"")),
    }
}

/// "" or whitespace in settings = use the provider's built-in default.
pub(crate) fn or_default<'a>(value: &'a str, default: &'a str) -> &'a str {
    let v = value.trim();
    if v.is_empty() { default } else { v }
}

/// Human-readable reason for a non-2xx response (spec: auth / rate-limit /
/// other, with the provider's own message when it sends JSON).
pub fn friendly_http_error(status: u16, body: &str) -> String {
    let detail = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v.pointer("/error/message")
                .and_then(|m| m.as_str().map(String::from))
                .or_else(|| v.get("error").and_then(|m| m.as_str().map(String::from)))
                .or_else(|| v.get("message").and_then(|m| m.as_str().map(String::from)))
        })
        .unwrap_or_else(|| body.chars().take(200).collect());
    match status {
        401 | 403 => format!("Authentication failed — check your API key ({detail})"),
        429 => format!("Rate limited by the provider ({detail})"),
        _ => format!("Provider returned HTTP {status}: {detail}"),
    }
}

/// Execute a built request; returns the raw success body.
pub async fn send(req: HttpRequest) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .build()
        .map_err(|e| e.to_string())?;
    let mut r = client.post(&req.url).json(&req.body);
    for (name, value) in &req.headers {
        r = r.header(*name, value);
    }
    let resp = r.send().await.map_err(|e| {
        if e.is_timeout() {
            format!("Request timed out after {REQUEST_TIMEOUT_SECS} s")
        } else if e.is_connect() {
            format!("Could not connect to {}", req.url)
        } else {
            e.to_string()
        }
    })?;
    let status = resp.status().as_u16();
    let body = resp.text().await.map_err(|e| e.to_string())?;
    if !(200..300).contains(&status) {
        return Err(friendly_http_error(status, &body));
    }
    Ok(body)
}

/// Full completion against a SPECIFIC provider (Task 4's `test_provider`
/// uses this directly).
pub async fn complete_with(app: &AppHandle, provider_id: &str, prompt: &str) -> Result<String, String> {
    let settings = crate::config::load(app)?;
    let p = provider(provider_id)?;
    let key = crate::secrets::get(provider_id)?;
    if p.requires_api_key() && key.is_none() {
        return Err(format!(
            "No API key configured for {provider_id} — add one on the Providers page"
        ));
    }
    let req = p.build_request(settings.ai.provider(provider_id)?, key.as_deref(), prompt)?;
    let body = send(req).await?;
    p.parse_response(&body)
}

/// Full completion against the ACTIVE provider from settings.
pub async fn complete(app: &AppHandle, prompt: &str) -> Result<String, String> {
    let id = crate::config::load(app)?.ai.active_provider;
    complete_with(app, &id, prompt).await
}
```

Fill `src-tauri/src/ai/openai_compatible.rs` (above its test module):

```rust
use super::{or_default, AiProvider, HttpRequest};
use crate::config::ProviderSettings;

pub const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";
pub const DEFAULT_MODEL: &str = "gpt-4o-mini";

/// OpenAI + anything speaking its API: LM Studio, llama.cpp server, Azure
/// OpenAI-compatible gateways, OpenRouter, ...
pub struct OpenAiCompatible;

impl AiProvider for OpenAiCompatible {
    fn id(&self) -> &'static str {
        "openai_compatible"
    }

    /// Local OpenAI-style servers run keyless — the key is optional here
    /// and simply omitted from the headers when absent.
    fn requires_api_key(&self) -> bool {
        false
    }

    fn build_request(
        &self,
        cfg: &ProviderSettings,
        api_key: Option<&str>,
        prompt: &str,
    ) -> Result<HttpRequest, String> {
        let base = or_default(&cfg.base_url, DEFAULT_BASE_URL).trim_end_matches('/');
        let mut headers: Vec<(&'static str, String)> = Vec::new();
        if let Some(key) = api_key {
            headers.push(("authorization", format!("Bearer {key}")));
        }
        Ok(HttpRequest {
            url: format!("{base}/chat/completions"),
            headers,
            body: serde_json::json!({
                "model": or_default(&cfg.model, DEFAULT_MODEL),
                "messages": [{ "role": "user", "content": prompt }],
            }),
        })
    }

    fn parse_response(&self, body: &str) -> Result<String, String> {
        let v: serde_json::Value =
            serde_json::from_str(body).map_err(|e| format!("Unexpected response: {e}"))?;
        v.pointer("/choices/0/message/content")
            .and_then(|c| c.as_str())
            .map(|s| s.trim().to_string())
            .ok_or_else(|| "Unexpected response shape (no choices[0].message.content)".into())
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd src-tauri; cargo test`
Expected: 5 `openai_compatible` + 4 `ai::tests` tests pass (the two `#[tokio::test]`s spin up a local httpmock server — no real network); all suites green.

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/ai src-tauri/src/lib.rs
git commit -m "feat: add AiProvider trait with executor and openai-compatible provider"
```

---

### Task 4: `ollama`, `anthropic`, `gemini` providers + `test_provider` command (TDD)

**Files:**
- Create: `src-tauri/src/ai/ollama.rs`, `src-tauri/src/ai/anthropic.rs`, `src-tauri/src/ai/gemini.rs`
- Modify: `src-tauri/src/ai/mod.rs` (registry lines, module decls, `test_provider` command)
- Modify: `src-tauri/src/lib.rs` (register `test_provider`)

**Interfaces:**
- Consumes: `AiProvider`, `HttpRequest`, `or_default`, `complete_with` (Task 3).
- Produces: registry entries for all four `config::PROVIDER_IDS`; async command `test_provider(provider_id: String) -> Result<String, String>` (sends "Reply with exactly: OK", returns the model's reply — Task 9's Test button).

- [ ] **Step 1: Write the failing tests**

Create the three files with only test modules, and add to `ai/mod.rs` top: `pub mod anthropic;`, `pub mod gemini;`, `pub mod ollama;` (alphabetical around the existing `pub mod openai_compatible;`).

`src-tauri/src/ai/ollama.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProviderSettings;

    #[test]
    fn builds_a_non_streaming_chat_request_without_auth() {
        let cfg = ProviderSettings { base_url: "http://box:11434/".into(), model: "phi3".into() };
        let req = Ollama.build_request(&cfg, None, "Hi").unwrap();
        assert_eq!(req.url, "http://box:11434/api/chat");
        assert!(req.headers.is_empty());
        assert_eq!(req.body["model"], "phi3");
        assert_eq!(req.body["stream"], false);
        assert_eq!(req.body["messages"][0]["content"], "Hi");
    }

    #[test]
    fn empty_config_falls_back_to_localhost_defaults() {
        let req = Ollama.build_request(&ProviderSettings::default(), None, "Hi").unwrap();
        assert_eq!(req.url, format!("{DEFAULT_BASE_URL}/api/chat"));
        assert_eq!(req.body["model"], DEFAULT_MODEL);
    }

    #[test]
    fn parses_message_content() {
        let body = r#"{"message":{"role":"assistant","content":" Hello "},"done":true}"#;
        assert_eq!(Ollama.parse_response(body).unwrap(), "Hello");
        assert!(Ollama.parse_response("{}").is_err());
    }
}
```

`src-tauri/src/ai/anthropic.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProviderSettings;

    #[test]
    fn builds_a_messages_request_with_version_header_and_max_tokens() {
        let cfg = ProviderSettings { base_url: String::new(), model: String::new() };
        let req = Anthropic.build_request(&cfg, Some("sk-ant"), "Hi").unwrap();
        assert_eq!(req.url, format!("{DEFAULT_BASE_URL}/v1/messages"));
        assert!(req.headers.contains(&("x-api-key", "sk-ant".into())));
        assert!(req.headers.contains(&("anthropic-version", "2023-06-01".into())));
        assert_eq!(req.body["model"], DEFAULT_MODEL);
        assert_eq!(req.body["max_tokens"], MAX_TOKENS);
        assert_eq!(req.body["messages"][0]["content"], "Hi");
    }

    #[test]
    fn missing_api_key_is_rejected_at_build_time() {
        let err = Anthropic
            .build_request(&ProviderSettings::default(), None, "Hi")
            .unwrap_err();
        assert!(err.to_lowercase().contains("api key"), "got: {err}");
    }

    #[test]
    fn parses_the_first_content_block() {
        let body = r#"{"content":[{"type":"text","text":" Hello "}],"role":"assistant"}"#;
        assert_eq!(Anthropic.parse_response(body).unwrap(), "Hello");
        assert!(Anthropic.parse_response(r#"{"content":[]}"#).is_err());
    }
}
```

`src-tauri/src/ai/gemini.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProviderSettings;

    #[test]
    fn builds_a_generate_content_request_with_goog_api_key_header() {
        let cfg = ProviderSettings { base_url: String::new(), model: "gemini-2.5-pro".into() };
        let req = Gemini.build_request(&cfg, Some("g-key"), "Hi").unwrap();
        assert_eq!(
            req.url,
            format!("{DEFAULT_BASE_URL}/v1beta/models/gemini-2.5-pro:generateContent")
        );
        assert!(req.headers.contains(&("x-goog-api-key", "g-key".into())));
        assert_eq!(req.body["contents"][0]["parts"][0]["text"], "Hi");
    }

    #[test]
    fn missing_api_key_is_rejected_at_build_time() {
        let err = Gemini.build_request(&ProviderSettings::default(), None, "Hi").unwrap_err();
        assert!(err.to_lowercase().contains("api key"), "got: {err}");
    }

    #[test]
    fn parses_the_first_candidate_part() {
        let body = r#"{"candidates":[{"content":{"parts":[{"text":" Hello "}],"role":"model"}}]}"#;
        assert_eq!(Gemini.parse_response(body).unwrap(), "Hello");
        assert!(Gemini.parse_response(r#"{"candidates":[]}"#).is_err());
    }
}
```

Add to `ai/mod.rs` tests:

```rust
    #[test]
    fn every_config_provider_id_resolves_in_the_registry() {
        for id in crate::config::PROVIDER_IDS {
            assert_eq!(provider(id).unwrap().id(), id, "registry mismatch for {id}");
        }
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri; cargo test ai`
Expected: FAIL to compile — `Ollama`, `Anthropic`, `Gemini` not found.

- [ ] **Step 3: Write the implementations**

`src-tauri/src/ai/ollama.rs` (above tests):

```rust
use super::{or_default, AiProvider, HttpRequest};
use crate::config::ProviderSettings;

pub const DEFAULT_BASE_URL: &str = "http://localhost:11434";
pub const DEFAULT_MODEL: &str = "llama3.2";

pub struct Ollama;

impl AiProvider for Ollama {
    fn id(&self) -> &'static str {
        "ollama"
    }

    fn requires_api_key(&self) -> bool {
        false
    }

    fn build_request(
        &self,
        cfg: &ProviderSettings,
        _api_key: Option<&str>,
        prompt: &str,
    ) -> Result<HttpRequest, String> {
        let base = or_default(&cfg.base_url, DEFAULT_BASE_URL).trim_end_matches('/');
        Ok(HttpRequest {
            url: format!("{base}/api/chat"),
            headers: Vec::new(),
            body: serde_json::json!({
                "model": or_default(&cfg.model, DEFAULT_MODEL),
                "messages": [{ "role": "user", "content": prompt }],
                // One-shot result to the clipboard — streaming buys nothing here.
                "stream": false,
            }),
        })
    }

    fn parse_response(&self, body: &str) -> Result<String, String> {
        let v: serde_json::Value =
            serde_json::from_str(body).map_err(|e| format!("Unexpected response: {e}"))?;
        v.pointer("/message/content")
            .and_then(|c| c.as_str())
            .map(|s| s.trim().to_string())
            .ok_or_else(|| "Unexpected response shape (no message.content)".into())
    }
}
```

`src-tauri/src/ai/anthropic.rs` (above tests):

```rust
use super::{or_default, AiProvider, HttpRequest};
use crate::config::ProviderSettings;

pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
pub const DEFAULT_MODEL: &str = "claude-sonnet-5";
/// The Messages API requires max_tokens; generous ceiling for prompt results.
pub const MAX_TOKENS: u32 = 4096;

pub struct Anthropic;

impl AiProvider for Anthropic {
    fn id(&self) -> &'static str {
        "anthropic"
    }

    fn requires_api_key(&self) -> bool {
        true
    }

    fn build_request(
        &self,
        cfg: &ProviderSettings,
        api_key: Option<&str>,
        prompt: &str,
    ) -> Result<HttpRequest, String> {
        let key = api_key.ok_or("Anthropic requires an API key")?;
        let base = or_default(&cfg.base_url, DEFAULT_BASE_URL).trim_end_matches('/');
        Ok(HttpRequest {
            url: format!("{base}/v1/messages"),
            headers: vec![
                ("x-api-key", key.to_string()),
                ("anthropic-version", "2023-06-01".to_string()),
            ],
            body: serde_json::json!({
                "model": or_default(&cfg.model, DEFAULT_MODEL),
                "max_tokens": MAX_TOKENS,
                "messages": [{ "role": "user", "content": prompt }],
            }),
        })
    }

    fn parse_response(&self, body: &str) -> Result<String, String> {
        let v: serde_json::Value =
            serde_json::from_str(body).map_err(|e| format!("Unexpected response: {e}"))?;
        v.pointer("/content/0/text")
            .and_then(|c| c.as_str())
            .map(|s| s.trim().to_string())
            .ok_or_else(|| "Unexpected response shape (no content[0].text)".into())
    }
}
```

`src-tauri/src/ai/gemini.rs` (above tests):

```rust
use super::{or_default, AiProvider, HttpRequest};
use crate::config::ProviderSettings;

pub const DEFAULT_BASE_URL: &str = "https://generativelanguage.googleapis.com";
pub const DEFAULT_MODEL: &str = "gemini-2.5-flash";

pub struct Gemini;

impl AiProvider for Gemini {
    fn id(&self) -> &'static str {
        "gemini"
    }

    fn requires_api_key(&self) -> bool {
        true
    }

    fn build_request(
        &self,
        cfg: &ProviderSettings,
        api_key: Option<&str>,
        prompt: &str,
    ) -> Result<HttpRequest, String> {
        let key = api_key.ok_or("Gemini requires an API key")?;
        let base = or_default(&cfg.base_url, DEFAULT_BASE_URL).trim_end_matches('/');
        let model = or_default(&cfg.model, DEFAULT_MODEL);
        Ok(HttpRequest {
            url: format!("{base}/v1beta/models/{model}:generateContent"),
            headers: vec![("x-goog-api-key", key.to_string())],
            body: serde_json::json!({
                "contents": [{ "parts": [{ "text": prompt }] }],
            }),
        })
    }

    fn parse_response(&self, body: &str) -> Result<String, String> {
        let v: serde_json::Value =
            serde_json::from_str(body).map_err(|e| format!("Unexpected response: {e}"))?;
        v.pointer("/candidates/0/content/parts/0/text")
            .and_then(|c| c.as_str())
            .map(|s| s.trim().to_string())
            .ok_or_else(|| "Unexpected response shape (no candidates[0].content.parts[0].text)".into())
    }
}
```

In `ai/mod.rs`, complete the registry and add the command at the bottom:

```rust
pub fn provider(id: &str) -> Result<&'static dyn AiProvider, String> {
    match id {
        "openai_compatible" => Ok(&openai_compatible::OpenAiCompatible),
        "ollama" => Ok(&ollama::Ollama),
        "anthropic" => Ok(&anthropic::Anthropic),
        "gemini" => Ok(&gemini::Gemini),
        _ => Err(format!("Unknown AI provider \"{id}\"")),
    }
}
```

```rust
/// Connection test for the Providers page: cheapest possible round trip
/// that proves endpoint + key + model all work.
#[tauri::command]
pub async fn test_provider(app: AppHandle, provider_id: String) -> Result<String, String> {
    complete_with(&app, &provider_id, "Reply with exactly: OK").await
}
```

Register in `lib.rs` `invoke_handler` (after the `secrets` commands): `ai::test_provider,`.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd src-tauri; cargo test`
Expected: 9 new provider tests + the registry test pass; all suites green.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/ai src-tauri/src/lib.rs
git commit -m "feat: add ollama, anthropic and gemini providers with connection test"
```

---

### Task 5: `prompts.rs` — prompt store, template engine, seed prompt (TDD)

**Files:**
- Modify: `src-tauri/Cargo.toml` (add `chrono`, `uuid`)
- Create: `src-tauri/src/prompts.rs`
- Modify: `src-tauri/src/lib.rs` (module decl + 3 commands)

**Interfaces:**
- Consumes: `tauri_plugin_store::StoreExt` (pattern from `config.rs`), `shortcuts::parse` (accelerator validation at the command boundary).
- Produces: `Prompt { id, name, template, shortcut, enabled }` (serde camelCase, `enabled` defaults true); `prompts::load(app) -> Result<Vec<Prompt>, String>` (seeds the default prompt on first run); `prompts::save_list(app, &[Prompt])`; pure `upsert(Vec<Prompt>, Prompt) -> Vec<Prompt>`, `remove(Vec<Prompt>, &str) -> Vec<Prompt>`, `render(&str, &TemplateVars) -> String`, `needs_selection(&str) -> bool`, `now_vars(selected_text, clipboard) -> TemplateVars`; commands `list_prompts`, `save_prompt(prompt) -> Prompt` (assigns a uuid when `id` is empty), `delete_prompt(id)`. Task 7 uses `load`/`render`/`needs_selection`/`now_vars`; Task 8 appends `sync_prompts` calls to the two mutating commands.

- [ ] **Step 1: Add the dependencies**

In `src-tauri/Cargo.toml` `[dependencies]` (after `keyring`):

```toml
chrono = "0.4"
uuid = { version = "1", features = ["v4"] }
```

- [ ] **Step 2: Write the failing tests**

Create `src-tauri/src/prompts.rs` with only the test module, and add `mod prompts;` to `lib.rs` (after `mod overlay;`):

```rust
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
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cd src-tauri; cargo test prompts`
Expected: FAIL to compile — `Prompt`, `TemplateVars`, `render`, `upsert`, ... not found.

- [ ] **Step 4: Write the implementation**

Prepend to `src-tauri/src/prompts.rs`:

```rust
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
```

Register in `lib.rs` `invoke_handler` (after `ai::test_provider,`):

```rust
            prompts::list_prompts,
            prompts::save_prompt,
            prompts::delete_prompt,
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd src-tauri; cargo test`
Expected: 8 new `prompts` tests pass; all suites green.

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/prompts.rs src-tauri/src/lib.rs
git commit -m "feat: add prompt store with template engine and seeded default prompt"
```

### Task 6: `notify.rs` + shared `send_ctrl_key` primitive (refactor)

Pure behavior-preserving refactor — no new tests; the existing suite plus a compile of every call site is the gate. It unblocks Task 7: `selection` needs Ctrl+**C** (today `inject` can only send Ctrl+V) and `prompt_flow` needs the notification helper currently private to `dictation`.

**Files:**
- Create: `src-tauri/src/notify.rs`
- Modify: `src-tauri/src/inject.rs` (generalize `send_paste` → `send_ctrl_key`)
- Modify: `src-tauri/src/dictation.rs` (drop private `notify`, call the module)
- Modify: `src-tauri/src/shortcuts.rs` (`init` uses `notify::send`)
- Modify: `src-tauri/src/lib.rs` (module decl)

**Interfaces:**
- Produces: `notify::send(app: &AppHandle, enabled: bool, body: &str)` (single notification choke point; `enabled` = `settings.notifications_enabled`, callers pass it so one settings load covers a whole flow); `inject::send_ctrl_key(c: char) -> Result<(), String>` (`pub(crate)`). Tasks 7–8 consume both.

- [ ] **Step 1: Create `src-tauri/src/notify.rs`** (and add `mod notify;` to `lib.rs` after `mod models;`):

```rust
use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

/// App-wide notification choke point. `enabled` is the caller's already-
/// loaded `settings.notifications_enabled` — pass `true` for failures that
/// happen BEFORE settings could be read.
pub fn send(app: &AppHandle, enabled: bool, body: &str) {
    if !enabled {
        return;
    }
    let _ = app
        .notification()
        .builder()
        .title("Claudy")
        .body(body)
        .show();
}
```

- [ ] **Step 2: Generalize the paste keystroke in `src-tauri/src/inject.rs`**

Replace the whole `send_paste` function with:

```rust
/// Send Ctrl+<c> via input simulation — 'v' pastes (dictation/auto-paste),
/// 'c' copies (selection probe).
pub(crate) fn send_ctrl_key(c: char) -> Result<(), String> {
    // Constructed per call: cheap on Windows, and enigo's default
    // release_keys_when_dropped(true) cleans up stuck keys on error.
    let mut enigo = Enigo::new(&EnigoSettings::default())
        .map_err(|e| format!("Input simulation unavailable: {e}"))?;
    enigo
        .key(Key::Control, Direction::Press)
        .map_err(|e| format!("Ctrl+{c} keystroke failed: {e}"))?;
    let click = enigo.key(Key::Unicode(c), Direction::Click);
    // Always attempt the release, even if the click failed.
    let release = enigo.key(Key::Control, Direction::Release);
    click.map_err(|e| format!("Ctrl+{c} keystroke failed: {e}"))?;
    release.map_err(|e| format!("Could not release Ctrl: {e}"))
}
```

and change its one call site in `insert_text`: `let paste_result = send_paste();` → `let paste_result = send_ctrl_key('v');`.

- [ ] **Step 3: Point `dictation.rs` at the module**

Delete the private `fn notify(...)` at the bottom of `dictation.rs` and replace its 5 call sites: `notify(&app, ...)` → `crate::notify::send(&app, ...)` (arguments unchanged — same signature). Remove the now-unused `use tauri_plugin_notification::NotificationExt;` import.

- [ ] **Step 4: Point `shortcuts::init` at the module**

Replace the notification block in `init` with:

```rust
    if let Err(e) = register_dictation(app, None, &settings.dictation_shortcut) {
        // Settings may be unreadable at this point: always show.
        crate::notify::send(app, true, &format!("Dictation shortcut unavailable: {e}"));
    }
```

(delete the inner `use tauri_plugin_notification::NotificationExt;` and builder chain).

- [ ] **Step 5: Verify unchanged behavior**

Run: `cd src-tauri; cargo test`
Expected: all existing suites green (no new tests — refactor only). Build warning-free (no unused imports).

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/notify.rs src-tauri/src/inject.rs src-tauri/src/dictation.rs src-tauri/src/shortcuts.rs src-tauri/src/lib.rs
git commit -m "refactor: extract notify module and shared ctrl-key primitive"
```

---

### Task 7: `selection.rs` + `prompt_flow.rs` — the prompt pipeline (TDD)

**Files:**
- Create: `src-tauri/src/selection.rs`
- Create: `src-tauri/src/prompt_flow.rs`
- Modify: `src-tauri/src/lib.rs` (2 module decls, `.manage(PromptFlowState)`, register `run_prompt`)

**Interfaces:**
- Consumes: `inject::send_ctrl_key` + `notify::send` (Task 6), `prompts::{load, render, needs_selection, now_vars}` (Task 5), `ai::complete` (Task 3), `inject::insert_text`, `config::load`.
- Produces: `selection::Selection { text: String, clipboard: String }` (`text == ""` = nothing selected; `clipboard` = user's original clipboard); `selection::read(app) -> Result<Selection, String>` (blocking ~300 ms — spawn_blocking only); pure `selection::interpret_capture(&str) -> String`; `prompt_flow::PromptFlowState` (managed); `prompt_flow::trigger(app, prompt_id)` (non-blocking — Task 8's shortcut handler calls this); pure `prompt_flow::find_enabled(&[Prompt], id) -> Result<Prompt, String>`; command `run_prompt(id)` (manual/E2E trigger without a shortcut; Phase 5's "run now" button).

- [ ] **Step 1: Write the failing tests**

Create `src-tauri/src/selection.rs` with only its test module, and add `mod selection;` to `lib.rs` (after `mod secrets;` — keep the list alphabetical):

```rust
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
```

Create `src-tauri/src/prompt_flow.rs` with only its test module, and add `mod prompt_flow;` to `lib.rs` (before `mod prompts;` — keep the list alphabetical):

```rust
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri; cargo test selection; cargo test prompt_flow`
Expected: FAIL to compile — `SENTINEL`, `interpret_capture`, `find_enabled` not found.

- [ ] **Step 3: Write `selection.rs`**

Prepend above its tests:

```rust
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
```

- [ ] **Step 4: Write `prompt_flow.rs`**

Prepend above its tests:

```rust
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
```

Wire `lib.rs`: `.manage(prompt_flow::PromptFlowState::default())` (after the `DictationState` manage line) and `prompt_flow::run_prompt,` in `invoke_handler` (after `prompts::delete_prompt,`).

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd src-tauri; cargo test`
Expected: 2 `selection` + 3 `prompt_flow` tests pass; all suites green.

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/selection.rs src-tauri/src/prompt_flow.rs src-tauri/src/lib.rs
git commit -m "feat: add selection probe and prompt flow orchestration"
```

---

### Task 8: `shortcuts.rs` — prompt-shortcut registry with live sync (TDD)

**Files:**
- Modify: `src-tauri/src/shortcuts.rs` (registry state, pure binding computation, `sync_prompts`, extend `init`)
- Modify: `src-tauri/src/prompts.rs` (`save_prompt`/`delete_prompt` re-sync after saving)
- Modify: `src-tauri/src/config.rs` (`update_settings` re-syncs — a dictation-shortcut change alters the conflict set)
- Modify: `src-tauri/src/lib.rs` (`.manage(PromptShortcuts)`)

**Interfaces:**
- Consumes: `prompts::{load, Prompt}` (Task 5), `prompt_flow::trigger` (Task 7), `notify::send` (Task 6), existing `parse`.
- Produces: `shortcuts::PromptShortcuts(Mutex<HashMap<String, String>>)` (managed; accel string → prompt id — the handler closure resolves the prompt id at FIRE time via this map, so renaming/re-pointing a prompt needs no re-registration); pure `desired_prompt_bindings(&[Prompt], dictation_accel) -> (Vec<(String, String)>, Vec<String>)` (bindings + human-readable warnings); `sync_prompts(app) -> Result<Vec<String>, String>` (reconcile registered shortcuts with the store, returns warnings). A skipped binding (conflict/invalid) is a warning notification, never a hard failure — the rest of the prompts still work.

- [ ] **Step 1: Write the failing tests**

Append inside the existing `mod tests` in `src-tauri/src/shortcuts.rs`:

```rust
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd src-tauri; cargo test shortcuts`
Expected: FAIL to compile — `desired_prompt_bindings` not found.

- [ ] **Step 3: Write the implementation**

In `src-tauri/src/shortcuts.rs`, add to the top:

```rust
use std::collections::HashMap;
use std::sync::Mutex;

use tauri::Manager;
```

and append after `register_dictation`:

```rust
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
```

Extend `init` (after the dictation registration block):

```rust
    match sync_prompts(app) {
        Ok(warnings) => notify_sync_warnings(app, &warnings),
        Err(e) => crate::notify::send(app, true, &format!("Prompt shortcuts unavailable: {e}")),
    }
```

- [ ] **Step 4: Wire the mutation paths**

In `src-tauri/src/prompts.rs`, before the final `Ok(prompt)` of `save_prompt` and as the tail of `delete_prompt`, re-sync (a failed sync is a real error — the user just changed bindings and must know they didn't take):

```rust
    // save_prompt: after `save_list(&app, &list)?;`
    crate::shortcuts::notify_sync_warnings(&app, &crate::shortcuts::sync_prompts(&app)?);
    Ok(prompt)
```

```rust
    // delete_prompt: replace the final `save_list(&app, &list)` line
    save_list(&app, &list)?;
    crate::shortcuts::notify_sync_warnings(&app, &crate::shortcuts::sync_prompts(&app)?);
    Ok(())
```

In `src-tauri/src/config.rs` `update_settings`, after the `register_dictation` block (the new dictation combo may collide with — or free up — a prompt binding):

```rust
    if old.dictation_shortcut != settings.dictation_shortcut {
        crate::shortcuts::register_dictation(
            &app,
            Some(&old.dictation_shortcut),
            &settings.dictation_shortcut,
        )?;
        save(&app, &settings)?;
        crate::shortcuts::notify_sync_warnings(&app, &crate::shortcuts::sync_prompts(&app)?);
        return Ok(());
    }
    save(&app, &settings)
```

(note the save happens BEFORE the sync — `sync_prompts` reads settings from the store, so it must see the new dictation shortcut.)

In `src-tauri/src/lib.rs`, add `.manage(shortcuts::PromptShortcuts::default())` after the `PromptFlowState` manage line.

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd src-tauri; cargo test`
Expected: 4 new `shortcuts` tests pass; all suites green.

- [ ] **Step 6: Commit**

```powershell
git add src-tauri/src/shortcuts.rs src-tauri/src/prompts.rs src-tauri/src/config.rs src-tauri/src/lib.rs
git commit -m "feat: register prompt global shortcuts with live sync"
```

---

### Task 9: Frontend — `ai-api.ts` + minimal Providers page

No frontend test runner exists in this repo; the gates are `npx tsc --noEmit` plus the Verification section's manual E2E.

**Files:**
- Create: `src/lib/ai-api.ts`
- Modify: `src/pages/ProvidersPage.tsx` (replace the placeholder)

**Interfaces:**
- Consumes: commands from Tasks 2/4/5/7 (`set_api_key`, `has_api_key`, `delete_api_key`, `test_provider`, `list_prompts`, `save_prompt`, `delete_prompt`, `run_prompt`), `useSettings` + `ProviderId`/`AiSettings` types (Task 1). Note Tauri converts command arg names to camelCase: `provider_id` → `providerId`.
- Produces: typed wrappers (Phase 5's prompt manager reuses the prompt ones) and a working Providers page: active-provider select, base URL/model fields, API-key entry (write-only), connection test.

- [ ] **Step 1: Create `src/lib/ai-api.ts`**

```typescript
import { invoke } from "@tauri-apps/api/core";
import type { ProviderId } from "@/lib/settings-store";

// --- API keys (Task 2) — the key is write-only; it never comes back. ---

export const setApiKey = (provider: ProviderId, key: string): Promise<void> =>
  invoke("set_api_key", { provider, key });

export const hasApiKey = (provider: ProviderId): Promise<boolean> =>
  invoke("has_api_key", { provider });

export const deleteApiKey = (provider: ProviderId): Promise<void> =>
  invoke("delete_api_key", { provider });

// --- Providers (Task 4) ---

/** Cheapest full round trip: endpoint + key + model. Resolves to the model's reply. */
export const testProvider = (providerId: ProviderId): Promise<string> =>
  invoke("test_provider", { providerId });

// --- Prompts (Tasks 5/7) — consumed by the Phase 5 prompt manager. ---

export interface Prompt {
  id: string;
  name: string;
  template: string;
  shortcut: string;
  enabled: boolean;
}

export const listPrompts = (): Promise<Prompt[]> => invoke("list_prompts");

/** Upsert; pass id: "" to create. Resolves to the stored prompt (id filled in). */
export const savePrompt = (prompt: Prompt): Promise<Prompt> =>
  invoke("save_prompt", { prompt });

export const deletePrompt = (id: string): Promise<void> =>
  invoke("delete_prompt", { id });

export const runPrompt = (id: string): Promise<void> => invoke("run_prompt", { id });
```

- [ ] **Step 2: Replace `src/pages/ProvidersPage.tsx`**

```tsx
import { useEffect, useState } from "react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { hasApiKey, setApiKey, testProvider } from "@/lib/ai-api";
import {
  useSettings,
  type AiSettings,
  type ProviderId,
} from "@/lib/settings-store";

interface ProviderMeta {
  id: ProviderId;
  settingsKey: keyof Omit<AiSettings, "activeProvider">;
  label: string;
  defaultBaseUrl: string;
  defaultModel: string;
  keyHint: string;
}

const PROVIDERS: ProviderMeta[] = [
  {
    id: "openai_compatible",
    settingsKey: "openaiCompatible",
    label: "OpenAI-compatible",
    defaultBaseUrl: "https://api.openai.com/v1",
    defaultModel: "gpt-4o-mini",
    keyHint: "Optional for local servers (LM Studio, llama.cpp)",
  },
  {
    id: "ollama",
    settingsKey: "ollama",
    label: "Ollama",
    defaultBaseUrl: "http://localhost:11434",
    defaultModel: "llama3.2",
    keyHint: "Not used by Ollama",
  },
  {
    id: "anthropic",
    settingsKey: "anthropic",
    label: "Anthropic",
    defaultBaseUrl: "https://api.anthropic.com",
    defaultModel: "claude-sonnet-5",
    keyHint: "Required",
  },
  {
    id: "gemini",
    settingsKey: "gemini",
    label: "Google Gemini",
    defaultBaseUrl: "https://generativelanguage.googleapis.com",
    defaultModel: "gemini-2.5-flash",
    keyHint: "Required",
  },
];

interface TestState {
  status: "idle" | "running" | "ok" | "error";
  message: string;
}

export default function ProvidersPage() {
  const settings = useSettings((s) => s.settings);
  const update = useSettings((s) => s.update);
  const [isKeyStored, setIsKeyStored] = useState(false);
  const [keyDraft, setKeyDraft] = useState("");
  const [keyError, setKeyError] = useState<string | null>(null);
  const [test, setTest] = useState<TestState>({ status: "idle", message: "" });

  const activeId = settings?.ai.activeProvider ?? "openai_compatible";
  const meta = PROVIDERS.find((p) => p.id === activeId) ?? PROVIDERS[0];

  useEffect(() => {
    setKeyDraft("");
    setKeyError(null);
    setTest({ status: "idle", message: "" });
    hasApiKey(meta.id)
      .then(setIsKeyStored)
      .catch((e: unknown) => setKeyError(String(e)));
  }, [meta.id]);

  if (!settings) return null;
  const cfg = settings.ai[meta.settingsKey];

  const patchProvider = (patch: Partial<{ baseUrl: string; model: string }>) =>
    update({ ai: { ...settings.ai, [meta.settingsKey]: { ...cfg, ...patch } } });

  const saveKey = async () => {
    setKeyError(null);
    try {
      await setApiKey(meta.id, keyDraft); // empty draft = remove the key
      setKeyDraft("");
      setIsKeyStored(await hasApiKey(meta.id));
    } catch (e: unknown) {
      setKeyError(String(e));
    }
  };

  const runTest = async () => {
    setTest({ status: "running", message: "" });
    try {
      const reply = await testProvider(meta.id);
      setTest({ status: "ok", message: reply });
    } catch (e: unknown) {
      setTest({ status: "error", message: String(e) });
    }
  };

  return (
    <div className="flex flex-col gap-6 p-6">
      <div>
        <h1 className="text-2xl font-semibold">Providers</h1>
        <p className="text-muted-foreground mt-1">
          AI provider for prompt shortcuts.
        </p>
      </div>

      <Card>
        <CardHeader>
          <CardTitle>Active provider</CardTitle>
          <CardDescription>Used by every prompt shortcut</CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-4">
          <div className="flex items-center gap-3">
            <Label className="w-28 shrink-0">Provider</Label>
            <Select
              value={activeId}
              onValueChange={(v) =>
                update({ ai: { ...settings.ai, activeProvider: v as ProviderId } })
              }
            >
              <SelectTrigger className="flex-1">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {PROVIDERS.map((p) => (
                  <SelectItem key={p.id} value={p.id}>
                    {p.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          {/* key remounts on provider switch so defaultValue re-seeds; commit
              on blur — per-keystroke updates would write settings.json each key */}
          <div className="flex items-center gap-3">
            <Label className="w-28 shrink-0">Base URL</Label>
            <Input
              key={`${meta.id}-baseUrl`}
              defaultValue={cfg.baseUrl}
              placeholder={meta.defaultBaseUrl}
              onBlur={(e) => patchProvider({ baseUrl: e.target.value.trim() })}
            />
          </div>
          <div className="flex items-center gap-3">
            <Label className="w-28 shrink-0">Model</Label>
            <Input
              key={`${meta.id}-model`}
              defaultValue={cfg.model}
              placeholder={meta.defaultModel}
              onBlur={(e) => patchProvider({ model: e.target.value.trim() })}
            />
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>API key</CardTitle>
          <CardDescription>
            Stored in the OS credential store, never in a file. {meta.keyHint}.
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          <div className="flex items-center gap-3">
            <Input
              type="password"
              value={keyDraft}
              placeholder={isKeyStored ? "•••••••• (stored)" : "Paste API key"}
              onChange={(e) => setKeyDraft(e.target.value)}
            />
            <Button
              variant="outline"
              size="sm"
              onClick={saveKey}
              disabled={!keyDraft && !isKeyStored}
            >
              {keyDraft || !isKeyStored ? "Save key" : "Remove key"}
            </Button>
            {isKeyStored && <Badge variant="secondary">Key stored</Badge>}
          </div>
          {keyError && <p className="text-destructive text-sm">{keyError}</p>}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Connection test</CardTitle>
          <CardDescription>
            Sends a one-word prompt through the full pipeline
          </CardDescription>
        </CardHeader>
        <CardContent className="flex flex-col gap-3">
          <div>
            <Button onClick={runTest} disabled={test.status === "running"}>
              {test.status === "running" ? "Testing…" : "Test connection"}
            </Button>
          </div>
          {test.status === "ok" && (
            <p className="text-sm text-green-600">Reply: {test.message}</p>
          )}
          {test.status === "error" && (
            <p className="text-destructive text-sm">{test.message}</p>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
```

- [ ] **Step 3: Type-check and build**

Run: `npx tsc --noEmit` → clean. `cd src-tauri; cargo test` → still green (no Rust changes; sanity only).

- [ ] **Step 4: Commit**

```powershell
git add src/lib/ai-api.ts src/pages/ProvidersPage.tsx
git commit -m "feat: add providers page with api key entry and connection test"
```

---

## Verification (end of Phase 4)

Automated gates first:

- [ ] `cd src-tauri; cargo test` — all suites green (~40 new tests across Tasks 1–8).
- [ ] `npx tsc --noEmit` — clean.
- [ ] `cd src-tauri; cargo build` — no warnings from the new modules.

Manual E2E (`npm run tauri dev`, Windows 11):

- [ ] **Configure a provider** — Providers page: pick a provider you can reach (Ollama locally, or paste a real key for a hosted one). "Test connection" shows the model's reply (expect "OK").
- [ ] **Keys never touch JSON** — after saving a key, `settings.json` and `prompts.json` (in `%APPDATA%\com.claudy.app`) contain no key material; Windows Credential Manager shows a `com.claudy.app` entry with user `provider:<id>`. "Remove key" deletes the entry.
- [ ] **The success criterion: prompt shortcut works end-to-end** — select a sentence with typos in Notepad → press `Ctrl+Shift+G` → "Running…" then "done — result copied to clipboard" notifications → Ctrl+V pastes the corrected text; the original selection in Notepad is untouched and the pre-existing clipboard content was only replaced by the RESULT (not by the probe).
- [ ] **Empty selection aborts loudly** — click into Notepad without selecting → `Ctrl+Shift+G` → "no text selected" notification, clipboard unchanged.
- [ ] **Provider failure is visible** — stop Ollama (or break the base URL) → trigger the prompt → failure notification with a readable reason; app stays healthy.
- [ ] **Auto-paste (opt-in)** — set `"autoPaste": true` in `settings.json` (no UI until Phase 5), restart → trigger the prompt on a selection → result replaces the selection in-place AND stays on the clipboard.
- [ ] **Shortcut conflict is a warning, not a crash** — set the seed prompt's shortcut equal to the dictation shortcut via `prompts.json`, restart → "Prompt shortcut skipped" notification; dictation still works.
- [ ] **Dictation regression check** — `Ctrl+Shift+D` dictation round-trip still works (Task 6 touched its notify/paste paths).

Wrap-up (same convention as Phase 3):

- [ ] Update `.superpowers/sdd/progress.md` — Phase 4 tasks ledger, mark READY.
- [ ] `git push` after your review.


