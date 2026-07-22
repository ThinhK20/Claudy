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

    /// OpenAI chat models take images via `image_url` parts; the default
    /// `gpt-4o-mini` is vision-capable. This provider also fronts arbitrary
    /// OpenAI-compatible gateways, so we optimistically allow images and let a
    /// non-vision endpoint surface its own error.
    fn supports_images(&self, _model: &str) -> bool {
        true
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
            // Rewrite the user message content as a parts array: text, then one
            // `image_url` part per attachment using an inline data URL. Done
            // before the system insert so the user message stays at index 0.
            let mut content = vec![serde_json::json!({ "type": "text", "text": prompt })];
            for img in &opts.images {
                content.push(serde_json::json!({
                    "type": "image_url",
                    "image_url": { "url": format!("data:{};base64,{}", img.media_type, img.data) },
                }));
            }
            req.body["messages"][0]["content"] = serde_json::Value::Array(content);
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
        v.pointer("/choices/0/message/content")
            .and_then(|c| c.as_str())
            .map(|s| s.trim().to_string())
            .ok_or_else(|| "Unexpected response shape (no choices[0].message.content)".into())
    }
}

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

    #[test]
    fn system_message_prepended_only_when_set() {
        let c = cfg("", "");
        let off = OpenAiCompatible
            .build_request_with(&c, None, "Hi", super::super::RequestOptions::default())
            .unwrap();
        let plain = OpenAiCompatible.build_request(&c, None, "Hi").unwrap();
        assert_eq!(off.body, plain.body, "no system prompt must leave the body identical");

        let on = OpenAiCompatible
            .build_request_with(
                &c,
                None,
                "Hi",
                super::super::RequestOptions {
                    system: Some("Be brief.\nUse Markdown.".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(on.body["messages"][0]["role"], "system");
        assert_eq!(on.body["messages"][0]["content"], "Be brief.\nUse Markdown.");
        assert_eq!(on.body["messages"][1]["role"], "user");
        assert_eq!(on.body["messages"][1]["content"], "Hi");
    }

    #[test]
    fn supports_images_is_advertised() {
        assert!(OpenAiCompatible.supports_images("gpt-4o-mini"));
    }

    #[test]
    fn images_become_image_url_parts_with_a_data_url() {
        let opts = super::super::RequestOptions {
            images: vec![super::super::ImageAttachment {
                media_type: "image/jpeg".into(),
                data: "ABC123".into(),
            }],
            ..Default::default()
        };
        let req = OpenAiCompatible.build_request_with(&cfg("", ""), None, "Read this", opts).unwrap();
        let content = &req.body["messages"][0]["content"];
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "Read this");
        assert_eq!(content[1]["type"], "image_url");
        assert_eq!(content[1]["image_url"]["url"], "data:image/jpeg;base64,ABC123");
    }

    #[test]
    fn image_content_survives_a_prepended_system_message() {
        let opts = super::super::RequestOptions {
            system: Some("Be brief.".into()),
            images: vec![super::super::ImageAttachment {
                media_type: "image/png".into(),
                data: "X".into(),
            }],
            ..Default::default()
        };
        let req = OpenAiCompatible.build_request_with(&cfg("", ""), None, "Hi", opts).unwrap();
        assert_eq!(req.body["messages"][0]["role"], "system");
        assert_eq!(req.body["messages"][1]["role"], "user");
        assert_eq!(req.body["messages"][1]["content"][0]["text"], "Hi");
        assert_eq!(req.body["messages"][1]["content"][1]["type"], "image_url");
    }
}
