use tauri::AppHandle;

/// One prompt round-trip must finish inside this (spec: timeout is a
/// reportable failure, not a hang).
pub const REQUEST_TIMEOUT_SECS: u64 = 60;

/// A fully built provider HTTP call — pure data, so request construction
/// is unit-testable without any network (spec testing requirement).
#[derive(Debug)]
pub struct HttpRequest {
    pub url: String,
    pub headers: Vec<(&'static str, String)>,
    pub body: serde_json::Value,
}

/// One image attached to a prompt. `data` is standard base64 (no data-URI
/// prefix); `media_type` is the MIME string (e.g. "image/png"). Deserialized
/// straight from the `ask_assistant` command payload, hence camelCase.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageAttachment {
    pub media_type: String,
    pub data: String,
}

/// Per-request switches that aren't part of the stored provider config.
/// Kept as a struct so new switches don't churn every call site (web search
/// in v1, the assistant's custom system prompt in Phase 8, images in Phase 9).
#[derive(Debug, Clone, Default)]
pub struct RequestOptions {
    pub web_search: bool,
    /// System instruction for the request; `None` = provider default shape
    /// (no system field at all). Set only by the quick-ask assistant path.
    pub system: Option<String>,
    /// Images to send alongside the prompt. Empty = text-only (every existing
    /// caller), so providers keep their bare-string content unchanged.
    pub images: Vec<ImageAttachment>,
}

/// Shared trim/filter so each provider injects the system prompt only when
/// it is set and non-empty.
pub(crate) fn non_empty_system(opts: &RequestOptions) -> Option<&str> {
    opts.system.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

/// Adding a provider = one new file implementing this + one registry line.
pub trait AiProvider: Sync {
    #[allow(dead_code)] // part of the provider contract; exercised by registry tests
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

    /// Does this provider offer a native web-search tool? Overridden per
    /// provider in Phase 2; defaults to false so ollama/openai_compatible
    /// silently no-op.
    fn supports_web_search(&self) -> bool {
        false
    }

    /// Can this provider's configured `model` accept image input? Defaults to
    /// false; cloud providers override to true, ollama uses a model-name
    /// heuristic. Drives the assistant's warn-and-block guard for attachments.
    fn supports_images(&self, _model: &str) -> bool {
        false
    }

    /// Build a request honoring `RequestOptions`. Defaults to `build_request`
    /// (ignoring options) so providers opt in by overriding only this.
    fn build_request_with(
        &self,
        cfg: &crate::config::ProviderSettings,
        api_key: Option<&str>,
        prompt: &str,
        _opts: RequestOptions,
    ) -> Result<HttpRequest, String> {
        self.build_request(cfg, api_key, prompt)
    }
}

pub mod anthropic;
pub mod gemini;
pub mod ollama;
pub mod openai_compatible;

pub fn provider(id: &str) -> Result<&'static dyn AiProvider, String> {
    match id {
        "openai_compatible" => Ok(&openai_compatible::OpenAiCompatible),
        "ollama" => Ok(&ollama::Ollama),
        "anthropic" => Ok(&anthropic::Anthropic),
        "gemini" => Ok(&gemini::Gemini),
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

/// Core completion against a SPECIFIC provider with explicit request options.
/// Uses `build_request_with`, so a provider that ignores options behaves
/// exactly as before.
pub async fn complete_provider_with_options(
    app: &AppHandle,
    provider_id: &str,
    prompt: &str,
    opts: RequestOptions,
) -> Result<String, String> {
    let settings = crate::config::load(app)?;
    let p = provider(provider_id)?;
    let key = crate::secrets::get(provider_id)?;
    if p.requires_api_key() && key.is_none() {
        return Err(format!(
            "No API key configured for {provider_id} — add one on the Providers page"
        ));
    }
    let req = p.build_request_with(
        settings.ai.provider(provider_id)?,
        key.as_deref(),
        prompt,
        opts,
    )?;
    let body = send(req).await?;
    p.parse_response(&body)
}

/// Full completion against a SPECIFIC provider (Task 4's `test_provider`
/// uses this directly). No request options — plain behavior.
pub async fn complete_with(app: &AppHandle, provider_id: &str, prompt: &str) -> Result<String, String> {
    complete_provider_with_options(app, provider_id, prompt, RequestOptions::default()).await
}

/// Full completion against the ACTIVE provider from settings.
pub async fn complete(app: &AppHandle, prompt: &str) -> Result<String, String> {
    let id = crate::config::load(app)?.ai.active_provider;
    complete_with(app, &id, prompt).await
}

/// Full completion against the ACTIVE provider honoring request options
/// (used by the quick-ask assistant for provider-native web search).
pub async fn complete_with_options(
    app: &AppHandle,
    prompt: &str,
    opts: RequestOptions,
) -> Result<String, String> {
    let id = crate::config::load(app)?.ai.active_provider;
    complete_provider_with_options(app, &id, prompt, opts).await
}

/// Whether the ACTIVE provider offers native web search (assistant uses this
/// to decide whether to request it).
pub fn active_provider_supports_web_search(app: &AppHandle) -> Result<bool, String> {
    let id = crate::config::load(app)?.ai.active_provider;
    Ok(provider(&id)?.supports_web_search())
}

/// Whether the ACTIVE provider + its configured model can accept images. The
/// quick-ask box calls this to gate image attachments (warn-and-block) and the
/// `ask` flow uses it as a backstop before sending. Empty model = provider
/// default, which each `supports_images` impl resolves on its own.
#[tauri::command]
pub fn active_provider_supports_images(app: AppHandle) -> Result<bool, String> {
    let settings = crate::config::load(&app)?;
    let id = &settings.ai.active_provider;
    let model = settings.ai.provider(id)?.model.clone();
    Ok(provider(id)?.supports_images(&model))
}

/// Connection test for the Providers page: cheapest possible round trip
/// that proves endpoint + key + model all work.
#[tauri::command]
pub async fn test_provider(app: AppHandle, provider_id: String) -> Result<String, String> {
    complete_with(&app, &provider_id, "Reply with exactly: OK").await
}

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

    #[test]
    fn every_config_provider_id_resolves_in_the_registry() {
        for id in crate::config::PROVIDER_IDS {
            assert_eq!(provider(id).unwrap().id(), id, "registry mismatch for {id}");
        }
    }

    #[test]
    fn providers_without_web_search_ignore_options_and_stay_identical() {
        // ollama & openai_compatible don't override the defaults: no web
        // search, and build_request_with == build_request regardless of opts.
        let cfg = crate::config::ProviderSettings {
            base_url: "http://x/v1".into(),
            model: "m".into(),
        };
        for id in ["ollama", "openai_compatible"] {
            let p = provider(id).unwrap();
            assert!(!p.supports_web_search(), "{id} should not support web search");
            let plain = p.build_request(&cfg, Some("k"), "Hi").unwrap();
            let with = p
                .build_request_with(
                    &cfg,
                    Some("k"),
                    "Hi",
                    RequestOptions { web_search: true, ..Default::default() },
                )
                .unwrap();
            assert_eq!(plain.url, with.url, "{id} url changed");
            assert_eq!(plain.body, with.body, "{id} body changed despite no web search");
        }
    }

    #[test]
    fn default_options_have_no_system_prompt_so_dictation_flow_is_unchanged() {
        // `complete()` / `complete_with()` (used by prompt_flow) build
        // RequestOptions::default() — pin that this means "no system prompt"
        // and that every provider's request body stays identical to the plain
        // build_request shape.
        assert!(RequestOptions::default().system.is_none());
        assert!(RequestOptions::default().images.is_empty());
        let cfg = crate::config::ProviderSettings {
            base_url: "http://x/v1".into(),
            model: "m".into(),
        };
        for id in crate::config::PROVIDER_IDS {
            let p = provider(id).unwrap();
            let plain = p.build_request(&cfg, Some("k"), "Hi").unwrap();
            let with = p
                .build_request_with(&cfg, Some("k"), "Hi", RequestOptions::default())
                .unwrap();
            assert_eq!(plain.body, with.body, "{id} body changed with default options");
        }
    }

    #[test]
    fn non_empty_system_filters_none_empty_and_whitespace() {
        let none = RequestOptions::default();
        assert_eq!(non_empty_system(&none), None);
        let empty = RequestOptions { system: Some("   ".into()), ..Default::default() };
        assert_eq!(non_empty_system(&empty), None);
        let set = RequestOptions { system: Some(" Be brief.\nUse lists. ".into()), ..Default::default() };
        assert_eq!(non_empty_system(&set), Some("Be brief.\nUse lists."));
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
