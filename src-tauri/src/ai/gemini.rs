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

    fn parse_response(&self, body: &str) -> Result<String, String> {
        let v: serde_json::Value =
            serde_json::from_str(body).map_err(|e| format!("Unexpected response: {e}"))?;
        v.pointer("/candidates/0/content/parts/0/text")
            .and_then(|c| c.as_str())
            .map(|s| s.trim().to_string())
            .ok_or_else(|| "Unexpected response shape (no candidates[0].content.parts[0].text)".into())
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
    fn parses_the_first_candidate_part() {
        let body = r#"{"candidates":[{"content":{"parts":[{"text":" Hello "}],"role":"model"}}]}"#;
        assert_eq!(Gemini.parse_response(body).unwrap(), "Hello");
        assert!(Gemini.parse_response(r#"{"candidates":[]}"#).is_err());
    }
}
