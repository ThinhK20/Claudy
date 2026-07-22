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

    fn supports_web_search(&self) -> bool {
        true
    }

    /// Gemini 1.5+ / 2.x models are natively multimodal.
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
            // Append one `inline_data` part per attachment after the text part.
            if let Some(parts) = req.body["contents"][0]["parts"].as_array_mut() {
                for img in &opts.images {
                    parts.push(serde_json::json!({
                        "inline_data": { "mime_type": img.media_type, "data": img.data },
                    }));
                }
            }
        }
        if opts.web_search {
            // Google Search grounding: Gemini decides when to search and
            // grounds its answer in the results.
            req.body["tools"] = serde_json::json!([{ "google_search": {} }]);
        }
        if let Some(system) = super::non_empty_system(&opts) {
            // v1beta generateContent takes camelCase `systemInstruction`
            // with the same parts shape as `contents`.
            req.body["systemInstruction"] =
                serde_json::json!({ "parts": [{ "text": system }] });
        }
        Ok(req)
    }

    /// Concatenate all text parts of the first candidate — grounded answers can
    /// split the response across multiple parts.
    fn parse_response(&self, body: &str) -> Result<String, String> {
        let v: serde_json::Value =
            serde_json::from_str(body).map_err(|e| format!("Unexpected response: {e}"))?;
        let parts = v
            .pointer("/candidates/0/content/parts")
            .and_then(|p| p.as_array())
            .ok_or("Unexpected response shape (no candidates[0].content.parts)")?;
        let text: String = parts
            .iter()
            .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
            .collect();
        let text = text.trim().to_string();
        if text.is_empty() {
            return Err("Unexpected response shape (no text parts)".into());
        }
        Ok(text)
    }
}

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
    fn parses_and_concatenates_candidate_parts() {
        let body = r#"{"candidates":[{"content":{"parts":[{"text":" Hello "}],"role":"model"}}]}"#;
        assert_eq!(Gemini.parse_response(body).unwrap(), "Hello");
        assert!(Gemini.parse_response(r#"{"candidates":[]}"#).is_err());

        let multi = r#"{"candidates":[{"content":{"parts":[
            {"text":"The score "},
            {"text":"was 3-1."}
        ]}}]}"#;
        assert_eq!(Gemini.parse_response(multi).unwrap(), "The score was 3-1.");
    }

    #[test]
    fn google_search_tool_added_only_when_requested() {
        let cfg = ProviderSettings::default();
        let off = Gemini
            .build_request_with(&cfg, Some("k"), "Hi", super::super::RequestOptions::default())
            .unwrap();
        assert!(off.body.get("tools").is_none());

        let on = Gemini
            .build_request_with(
                &cfg,
                Some("k"),
                "Hi",
                super::super::RequestOptions { web_search: true, ..Default::default() },
            )
            .unwrap();
        assert!(on.body["tools"][0].get("google_search").is_some());
        assert!(Gemini.supports_web_search());
    }

    #[test]
    fn system_instruction_added_only_when_set() {
        let cfg = ProviderSettings::default();
        let off = Gemini
            .build_request_with(&cfg, Some("k"), "Hi", super::super::RequestOptions::default())
            .unwrap();
        assert!(off.body.get("systemInstruction").is_none());

        let on = Gemini
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
        assert_eq!(
            on.body["systemInstruction"]["parts"][0]["text"],
            "Be brief.\nUse Markdown."
        );
        assert_eq!(on.body["contents"][0]["parts"][0]["text"], "Hi", "user prompt untouched");
    }

    #[test]
    fn supports_images_is_advertised() {
        assert!(Gemini.supports_images("gemini-2.5-flash"));
    }

    #[test]
    fn images_append_inline_data_parts_after_the_text() {
        let cfg = ProviderSettings::default();
        let opts = super::super::RequestOptions {
            images: vec![super::super::ImageAttachment {
                media_type: "image/webp".into(),
                data: "ZZZ".into(),
            }],
            ..Default::default()
        };
        let req = Gemini.build_request_with(&cfg, Some("k"), "Look", opts).unwrap();
        let parts = &req.body["contents"][0]["parts"];
        assert_eq!(parts[0]["text"], "Look");
        assert_eq!(parts[1]["inline_data"]["mime_type"], "image/webp");
        assert_eq!(parts[1]["inline_data"]["data"], "ZZZ");
    }
}
