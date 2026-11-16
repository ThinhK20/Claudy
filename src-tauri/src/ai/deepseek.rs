use super::openai_compatible::{apply_chat_options, chat_request, parse_chat_response, parse_chat_usage};
use super::{AiProvider, HttpRequest};
use crate::config::ProviderSettings;

pub const DEFAULT_BASE_URL: &str = "https://api.deepseek.com";
/// The fastest DeepSeek model, and the one that accepts images.
pub const DEFAULT_MODEL: &str = "deepseek-flash";

/// DeepSeek's own API. It speaks the OpenAI chat-completions format, so the
/// wire logic is shared with `openai_compatible`; what differs is the
/// defaults, a required key, per-model image support, and thinking mode
/// being switched off on every request (it is on by default and only adds
/// latency to Claudy's short rewrites and answers).
pub struct DeepSeek;

impl AiProvider for DeepSeek {
    fn id(&self) -> &'static str {
        "deepseek"
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
        let mut req = chat_request(cfg, api_key, prompt, DEFAULT_BASE_URL, DEFAULT_MODEL);
        req.body["thinking"] = serde_json::json!({ "type": "disabled" });
        Ok(req)
    }

    fn build_request_with(
        &self,
        cfg: &ProviderSettings,
        api_key: Option<&str>,
        prompt: &str,
        opts: super::RequestOptions,
    ) -> Result<HttpRequest, String> {
        let mut req = self.build_request(cfg, api_key, prompt)?;
        apply_chat_options(&mut req, prompt, &opts);
        Ok(req)
    }

    fn parse_response(&self, body: &str) -> Result<String, String> {
        parse_chat_response(body)
    }

    fn default_model(&self) -> &'static str {
        DEFAULT_MODEL
    }

    fn parse_usage(&self, body: &str) -> Option<super::ResponseUsage> {
        parse_chat_usage(body)
    }

    /// Flash takes images; V4-Pro is text-only. Anything else (a blank model,
    /// a future Flash release) is allowed, so a new vision model works without
    /// an app update and only a known text-only model is blocked.
    fn supports_images(&self, model: &str) -> bool {
        !model.trim().to_ascii_lowercase().starts_with(TEXT_ONLY_PREFIX)
    }
}

/// DeepSeek models matching this prefix accept no image input.
const TEXT_ONLY_PREFIX: &str = "deepseek-v4-pro";

#[cfg(test)]
mod tests {
    use crate::config::ProviderSettings;

    fn cfg(base_url: &str, model: &str) -> ProviderSettings {
        ProviderSettings { base_url: base_url.into(), model: model.into(), ..Default::default() }
    }

    #[test]
    fn requests_flash_on_the_deepseek_api_with_thinking_disabled() {
        let p = super::super::provider("deepseek").unwrap();
        let req = p.build_request(&cfg("", ""), Some("sk-ds"), "Hi").unwrap();
        assert_eq!(req.url, "https://api.deepseek.com/chat/completions");
        assert!(req.headers.contains(&("authorization", "Bearer sk-ds".into())));
        assert_eq!(req.body["model"], "deepseek-flash");
        assert_eq!(req.body["thinking"]["type"], "disabled");
        assert_eq!(req.body["messages"][0]["content"], "Hi");
    }

    #[test]
    fn reads_token_counts_and_the_model_deepseek_reports() {
        let body = r#"{
            "model":"deepseek-flash",
            "choices":[{"message":{"role":"assistant","content":"  Hi  ","reasoning_content":null}}],
            "usage":{"prompt_tokens":12,"completion_tokens":3,"total_tokens":15}
        }"#;
        let p = super::super::provider("deepseek").unwrap();
        let u = p.parse_usage(body).unwrap();
        assert_eq!((u.input_tokens, u.output_tokens), (12, 3));
        assert_eq!(u.model.as_deref(), Some("deepseek-flash"));
        assert_eq!(p.parse_response(body).unwrap(), "Hi");
    }

    #[test]
    fn only_v4_pro_is_refused_images() {
        let p = super::super::provider("deepseek").unwrap();
        assert!(p.supports_images("deepseek-flash"));
        assert!(p.supports_images("deepseek-flash-next"), "unknown models are allowed");
        assert!(!p.supports_images("deepseek-v4-pro"));
        assert!(!p.supports_images("DeepSeek-V4-Pro-0813"));
    }

    #[test]
    fn assistant_turns_carry_system_history_and_images_with_thinking_still_off() {
        use super::super::{HistoryTurn, ImageAttachment, RequestOptions, Role};
        let opts = RequestOptions {
            system: Some("Be brief.".into()),
            history: vec![
                HistoryTurn { role: Role::User, content: "What is Tauri?".into() },
                HistoryTurn { role: Role::Assistant, content: "A desktop framework.".into() },
            ],
            images: vec![ImageAttachment { media_type: "image/png".into(), data: "X".into() }],
            ..Default::default()
        };
        let p = super::super::provider("deepseek").unwrap();
        let req = p
            .build_request_with(&cfg("https://proxy.example/ds/", "deepseek-v4-pro"), Some("k"), "And this?", opts)
            .unwrap();
        assert_eq!(req.url, "https://proxy.example/ds/chat/completions");
        assert_eq!(req.body["model"], "deepseek-v4-pro");
        assert_eq!(req.body["thinking"]["type"], "disabled");
        let msgs = req.body["messages"].as_array().unwrap();
        let roles: Vec<&str> = msgs.iter().map(|m| m["role"].as_str().unwrap()).collect();
        assert_eq!(roles, ["system", "user", "assistant", "user"]);
        assert_eq!(msgs[0]["content"], "Be brief.");
        assert_eq!(msgs[3]["content"][0]["text"], "And this?");
        assert_eq!(msgs[3]["content"][1]["image_url"]["url"], "data:image/png;base64,X");
    }

    #[tokio::test]
    async fn round_trips_against_a_mock_deepseek_server() {
        let server = httpmock::MockServer::start();
        let mock = server.mock(|when, then| {
            when.method(httpmock::Method::POST)
                .path("/chat/completions")
                .header("authorization", "Bearer sk-ds")
                .json_body_partial(r#"{"model":"deepseek-flash","thinking":{"type":"disabled"}}"#);
            then.status(200).json_body(serde_json::json!({
                "model": "deepseek-flash",
                "choices": [{ "message": { "role": "assistant", "content": "OK" } }],
                "usage": { "prompt_tokens": 4, "completion_tokens": 1 }
            }));
        });
        let p = super::super::provider("deepseek").unwrap();
        assert!(p.requires_api_key(), "the DeepSeek API never works keyless");
        assert!(!p.supports_web_search());
        let req = p.build_request(&cfg(&server.base_url(), ""), Some("sk-ds"), "ping").unwrap();
        let body = super::super::send(req).await.unwrap();
        assert_eq!(p.parse_response(&body).unwrap(), "OK");
        mock.assert();
    }
}
