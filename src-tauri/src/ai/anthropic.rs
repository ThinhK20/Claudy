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
