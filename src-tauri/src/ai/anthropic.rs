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

    fn supports_web_search(&self) -> bool {
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
        if opts.web_search {
            // Server-side web search tool; Claude runs the searches itself and
            // folds results into the answer. `max_uses` bounds cost/latency.
            req.body["tools"] = serde_json::json!([{
                "type": WEB_SEARCH_TOOL,
                "name": "web_search",
                "max_uses": WEB_SEARCH_MAX_USES,
            }]);
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
                super::super::RequestOptions { web_search: true },
            )
            .unwrap();
        assert_eq!(on.body["tools"][0]["type"], WEB_SEARCH_TOOL);
        assert_eq!(on.body["tools"][0]["name"], "web_search");
        assert_eq!(on.body["tools"][0]["max_uses"], WEB_SEARCH_MAX_USES);
    }

    #[test]
    fn supports_web_search_is_advertised() {
        assert!(Anthropic.supports_web_search());
    }
}
