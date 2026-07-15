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
