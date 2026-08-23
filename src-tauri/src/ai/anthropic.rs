use super::{or_default, AiProvider, HttpRequest};
use crate::config::ProviderSettings;

pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
pub const DEFAULT_MODEL: &str = "claude-sonnet-5";
/// The Messages API requires max_tokens; generous ceiling for prompt results.
pub const MAX_TOKENS: u32 = 4096;
/// Server-side web search tool (verified against the Messages API docs).
pub const WEB_SEARCH_TOOL: &str = "web_search_20250305";
/// Bound the number of searches per request for cost/latency.
pub const WEB_SEARCH_MAX_USES: u32 = 3;

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

    fn default_model(&self) -> &'static str {
        DEFAULT_MODEL
    }

    /// Messages API reports `usage.input_tokens` / `usage.output_tokens`
    /// alongside the model it served. Web-search requests bill the extra
    /// searches separately; those aren't token counts and aren't tracked.
    fn parse_usage(&self, body: &str) -> Option<super::ResponseUsage> {
        super::usage_from(body, "/usage/input_tokens", "/usage/output_tokens", "/model")
    }

    fn supports_web_search(&self) -> bool {
        true
    }

    /// Every current Claude model accepts image input.
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
            // Promote the bare-string user content to a content-block array:
            // the text first, then one base64 `image` block per attachment.
            let mut content = vec![serde_json::json!({ "type": "text", "text": prompt })];
            for img in &opts.images {
                content.push(serde_json::json!({
                    "type": "image",
                    "source": {
                        "type": "base64",
                        "media_type": img.media_type,
                        "data": img.data,
                    },
                }));
            }
            req.body["messages"][0]["content"] = serde_json::Value::Array(content);
        }
        // After the image rewrite (which addresses the current message at
        // index 0) and before the system field, which is top-level here.
        super::prepend_history(&mut req.body, &opts.history);
        if opts.web_search {
            // Server-side web search tool; Claude runs the searches itself and
            // folds results into the answer. `max_uses` bounds cost/latency.
            req.body["tools"] = serde_json::json!([{
                "type": WEB_SEARCH_TOOL,
                "name": "web_search",
                "max_uses": WEB_SEARCH_MAX_USES,
            }]);
        }
        if let Some(system) = super::non_empty_system(&opts) {
            // Messages API takes the system prompt as a top-level field,
            // not a message role.
            req.body["system"] = serde_json::json!(system);
        }
        Ok(req)
    }

    /// Concatenate every `text` content block. Web-search responses interleave
    /// `server_tool_use` / `web_search_tool_result` / `text` blocks, so reading
    /// only `content[0]` would drop the answer; citations are ignored in v1.
    fn parse_response(&self, body: &str) -> Result<String, String> {
        let v: serde_json::Value =
            serde_json::from_str(body).map_err(|e| format!("Unexpected response: {e}"))?;
        let blocks = v
            .get("content")
            .and_then(|c| c.as_array())
            .ok_or("Unexpected response shape (no content array)")?;
        let text: String = blocks
            .iter()
            .filter(|b| b.get("type").and_then(|t| t.as_str()) == Some("text"))
            .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
            .collect();
        let text = text.trim().to_string();
        if text.is_empty() {
            return Err("Unexpected response shape (no text content)".into());
        }
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ProviderSettings;

    #[test]
    fn builds_a_messages_request_with_version_header_and_max_tokens() {
        let cfg = ProviderSettings { base_url: String::new(), model: String::new(), ..Default::default() };
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
    fn parses_and_concatenates_text_blocks_ignoring_tool_blocks() {
        let body = r#"{"content":[{"type":"text","text":" Hello "}],"role":"assistant"}"#;
        assert_eq!(Anthropic.parse_response(body).unwrap(), "Hello");
        assert!(Anthropic.parse_response(r#"{"content":[]}"#).is_err());

        // A web-search response interleaves server-tool blocks with text blocks.
        let web = r#"{"content":[
            {"type":"server_tool_use","id":"a","name":"web_search","input":{}},
            {"type":"web_search_tool_result","tool_use_id":"a","content":[]},
            {"type":"text","text":"The score "},
            {"type":"text","text":"was 3-1."}
        ]}"#;
        assert_eq!(Anthropic.parse_response(web).unwrap(), "The score was 3-1.");
    }

    #[test]
    fn parses_usage_from_a_messages_response() {
        let body = r#"{
            "model":"claude-sonnet-5-20260514",
            "content":[{"type":"text","text":"Hello"}],
            "usage":{"input_tokens":412,"output_tokens":96}
        }"#;
        let u = Anthropic.parse_usage(body).unwrap();
        assert_eq!(u.input_tokens, 412);
        assert_eq!(u.output_tokens, 96);
        assert_eq!(u.model.as_deref(), Some("claude-sonnet-5-20260514"));
        // The text parse must be unaffected by reading usage.
        assert_eq!(Anthropic.parse_response(body).unwrap(), "Hello");
    }

    #[test]
    fn missing_usage_block_yields_no_usage_but_still_parses_the_answer() {
        let body = r#"{"content":[{"type":"text","text":"Hi"}]}"#;
        assert!(Anthropic.parse_usage(body).is_none());
        assert_eq!(Anthropic.parse_response(body).unwrap(), "Hi");
    }

    #[test]
    fn web_search_tool_added_only_when_requested() {
        let cfg = ProviderSettings::default();
        let off = Anthropic
            .build_request_with(&cfg, Some("k"), "Hi", super::super::RequestOptions::default())
            .unwrap();
        assert!(off.body.get("tools").is_none());

        let on = Anthropic
            .build_request_with(
                &cfg,
                Some("k"),
                "Hi",
                super::super::RequestOptions { web_search: true, ..Default::default() },
            )
            .unwrap();
        assert_eq!(on.body["tools"][0]["type"], WEB_SEARCH_TOOL);
        assert_eq!(on.body["tools"][0]["name"], "web_search");
        assert_eq!(on.body["tools"][0]["max_uses"], WEB_SEARCH_MAX_USES);
    }

    #[test]
    fn system_prompt_becomes_a_top_level_field_only_when_set() {
        let cfg = ProviderSettings::default();
        let off = Anthropic
            .build_request_with(&cfg, Some("k"), "Hi", super::super::RequestOptions::default())
            .unwrap();
        assert!(off.body.get("system").is_none());

        let empty = Anthropic
            .build_request_with(
                &cfg,
                Some("k"),
                "Hi",
                super::super::RequestOptions { system: Some("  ".into()), ..Default::default() },
            )
            .unwrap();
        assert!(empty.body.get("system").is_none(), "whitespace-only must be ignored");

        let on = Anthropic
            .build_request_with(
                &cfg,
                Some("k"),
                "Hi",
                super::super::RequestOptions {
                    system: Some("Be brief.\nUse Markdown.".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(on.body["system"], "Be brief.\nUse Markdown.");
        assert_eq!(on.body["messages"][0]["content"], "Hi", "user prompt untouched");
    }

    #[test]
    fn supports_web_search_is_advertised() {
        assert!(Anthropic.supports_web_search());
    }

    #[test]
    fn supports_images_for_any_model() {
        assert!(Anthropic.supports_images("claude-sonnet-5"));
        assert!(Anthropic.supports_images(""));
    }

    #[test]
    fn images_become_content_blocks_after_the_text() {
        let cfg = ProviderSettings::default();
        let opts = super::super::RequestOptions {
            images: vec![super::super::ImageAttachment {
                media_type: "image/png".into(),
                data: "BASE64DATA".into(),
            }],
            ..Default::default()
        };
        let req = Anthropic.build_request_with(&cfg, Some("k"), "What is this?", opts).unwrap();
        let content = &req.body["messages"][0]["content"];
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "What is this?");
        assert_eq!(content[1]["type"], "image");
        assert_eq!(content[1]["source"]["type"], "base64");
        assert_eq!(content[1]["source"]["media_type"], "image/png");
        assert_eq!(content[1]["source"]["data"], "BASE64DATA");
    }

    /// Two prior turns in the shape `assistant::conversation` produces.
    fn history() -> Vec<super::super::HistoryTurn> {
        use super::super::{HistoryTurn, Role};
        vec![
            HistoryTurn { role: Role::User, content: "What is Tauri?".into() },
            HistoryTurn { role: Role::Assistant, content: "A desktop framework.".into() },
        ]
    }

    #[test]
    fn history_is_prepended_before_the_current_user_message() {
        let cfg = ProviderSettings::default();
        let req = Anthropic
            .build_request_with(
                &cfg,
                Some("k"),
                "How does it compare?",
                super::super::RequestOptions { history: history(), ..Default::default() },
            )
            .unwrap();
        let msgs = req.body["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0]["role"], "user");
        assert_eq!(msgs[0]["content"], "What is Tauri?");
        assert_eq!(msgs[1]["role"], "assistant");
        assert_eq!(msgs[1]["content"], "A desktop framework.");
        assert_eq!(msgs[2]["role"], "user");
        assert_eq!(msgs[2]["content"], "How does it compare?");
    }

    #[test]
    fn history_keeps_image_blocks_on_the_current_message() {
        // The regression the images-then-history ordering exists for: attaching
        // the picture to the OLDEST turn instead of the one being asked.
        let cfg = ProviderSettings::default();
        let opts = super::super::RequestOptions {
            history: history(),
            images: vec![super::super::ImageAttachment {
                media_type: "image/png".into(),
                data: "BASE64DATA".into(),
            }],
            ..Default::default()
        };
        let req = Anthropic.build_request_with(&cfg, Some("k"), "What is this?", opts).unwrap();
        let msgs = req.body["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 3);
        assert!(msgs[0]["content"].is_string(), "history must stay plain text");
        let current = &msgs[2]["content"];
        assert_eq!(current[0]["text"], "What is this?");
        assert_eq!(current[1]["source"]["data"], "BASE64DATA");
    }

    #[test]
    fn history_and_system_coexist() {
        let cfg = ProviderSettings::default();
        let req = Anthropic
            .build_request_with(
                &cfg,
                Some("k"),
                "Hi",
                super::super::RequestOptions {
                    history: history(),
                    system: Some("Be brief.".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        // The system prompt is a top-level field here, so it never competes
        // with history for a message slot.
        assert_eq!(req.body["system"], "Be brief.");
        assert_eq!(req.body["messages"].as_array().unwrap().len(), 3);
    }

    #[test]
    fn empty_history_leaves_the_body_unchanged() {
        let cfg = ProviderSettings::default();
        let plain = Anthropic.build_request(&cfg, Some("k"), "Hi").unwrap();
        let with = Anthropic
            .build_request_with(&cfg, Some("k"), "Hi", super::super::RequestOptions::default())
            .unwrap();
        assert_eq!(plain.body, with.body);
    }

    #[test]
    fn no_images_keeps_the_bare_string_content() {
        let cfg = ProviderSettings::default();
        let with = Anthropic
            .build_request_with(&cfg, Some("k"), "Hi", super::super::RequestOptions::default())
            .unwrap();
        assert_eq!(with.body["messages"][0]["content"], "Hi");
    }
}
