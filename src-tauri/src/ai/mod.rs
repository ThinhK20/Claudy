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
