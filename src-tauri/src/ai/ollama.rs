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

    /// Ollama vision support is per-model. The default `llama3.2` is text-only,
    /// so we allow images only when the model name carries a known vision tag.
    /// Conservative by design: the assistant blocks attachments otherwise.
    fn supports_images(&self, model: &str) -> bool {
        const VISION_TAGS: [&str; 9] = [
            "llava", "vision", "bakllava", "moondream", "minicpm-v", "llama3.2-vision",
            "qwen2-vl", "qwen2.5vl", "gemma3",
        ];
        let m = model.to_ascii_lowercase();
        VISION_TAGS.iter().any(|tag| m.contains(tag))
    }

    fn build_request_with(
        &self,
        cfg: &ProviderSettings,
        api_key: Option<&str>,
        prompt: &str,
        opts: super::RequestOptions,
    ) -> Result<HttpRequest, String> {
        let mut req = self.build_request(cfg, api_key, prompt)?;
        if !opts.images.is_empty() {
            // Ollama takes images as a message-level array of bare base64
            // strings (no data-URI prefix); the text content stays a string.
            let imgs: Vec<serde_json::Value> = opts
                .images
                .iter()
                .map(|i| serde_json::Value::String(i.data.clone()))
                .collect();
            req.body["messages"][0]["images"] = serde_json::Value::Array(imgs);
        }
        if let Some(system) = super::non_empty_system(&opts) {
            if let Some(messages) = req.body["messages"].as_array_mut() {
                messages.insert(0, serde_json::json!({ "role": "system", "content": system }));
            }
        }
        Ok(req)
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

    #[test]
    fn system_message_prepended_only_when_set() {
        let cfg = ProviderSettings::default();
        let off = Ollama
            .build_request_with(&cfg, None, "Hi", super::super::RequestOptions::default())
            .unwrap();
        let plain = Ollama.build_request(&cfg, None, "Hi").unwrap();
        assert_eq!(off.body, plain.body, "no system prompt must leave the body identical");

        let on = Ollama
            .build_request_with(
                &cfg,
                None,
                "Hi",
                super::super::RequestOptions {
                    system: Some("Be brief.".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(on.body["messages"][0]["role"], "system");
        assert_eq!(on.body["messages"][0]["content"], "Be brief.");
        assert_eq!(on.body["messages"][1]["role"], "user");
        assert_eq!(on.body["messages"][1]["content"], "Hi");
        assert_eq!(on.body["stream"], false, "stream flag untouched");
    }

    #[test]
    fn supports_images_only_for_known_vision_models() {
        assert!(!Ollama.supports_images("llama3.2"));
        assert!(!Ollama.supports_images(""));
        assert!(Ollama.supports_images("llava"));
        assert!(Ollama.supports_images("llama3.2-vision"));
        assert!(Ollama.supports_images("qwen2.5vl:7b"));
    }

    #[test]
    fn images_attach_as_bare_base64_on_the_user_message() {
        let cfg = ProviderSettings::default();
        let opts = super::super::RequestOptions {
            images: vec![super::super::ImageAttachment {
                media_type: "image/png".into(),
                data: "RAWB64".into(),
            }],
            ..Default::default()
        };
        let req = Ollama.build_request_with(&cfg, None, "Hi", opts).unwrap();
        assert_eq!(req.body["messages"][0]["content"], "Hi", "content stays a string");
        assert_eq!(req.body["messages"][0]["images"][0], "RAWB64");
    }
}
