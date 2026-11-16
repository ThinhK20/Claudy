use tauri::{AppHandle, Emitter};

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

/// One image attached to a prompt. `data` is standard base64 (no data-URI
/// prefix); `media_type` is the MIME string (e.g. "image/png"). Deserialized
/// straight from the `ask_assistant` command payload, hence camelCase.
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageAttachment {
    pub media_type: String,
    pub data: String,
}

/// Who said an earlier message. Providers map this onto their own vocabulary —
/// everyone says "assistant" except Gemini, which says "model".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
}

impl Role {
    /// Wire value for the three OpenAI-shaped providers.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
        }
    }
}

/// One earlier message replayed for context. Text only, by design: prior
/// images are represented by a marker (see `assistant::conversation`), so a
/// long session can never balloon the payload with replayed base64.
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryTurn {
    pub role: Role,
    pub content: String,
}

/// Per-request switches that aren't part of the stored provider config.
/// Kept as a struct so new switches don't churn every call site (web search
/// in v1, the assistant's custom system prompt in Phase 8, images in Phase 9,
/// quick-chat history in Phase 10).
#[derive(Debug, Clone, Default)]
pub struct RequestOptions {
    pub web_search: bool,
    /// System instruction for the request; `None` = provider default shape
    /// (no system field at all). Set only by the quick-ask assistant path.
    pub system: Option<String>,
    /// Images to send alongside the prompt. Empty = text-only (every existing
    /// caller), so providers keep their bare-string content unchanged.
    pub images: Vec<ImageAttachment>,
    /// Earlier turns of the current quick-chat session, oldest first, already
    /// budgeted by `assistant::conversation`. Empty for every other caller,
    /// and providers then leave the body byte-identical.
    pub history: Vec<HistoryTurn>,
}

/// Prepend history to a `{"role","content"}` messages array — anthropic,
/// openai_compatible and ollama all speak that shape. A no-op on empty history,
/// which is what keeps those three bodies byte-identical without a session.
///
/// Must run AFTER any image rewrite (which addresses the current message at
/// index 0) and BEFORE the system insert (which must end up at index 0).
pub(crate) fn prepend_history(body: &mut serde_json::Value, history: &[HistoryTurn]) {
    if history.is_empty() {
        return;
    }
    let mut messages: Vec<serde_json::Value> = history
        .iter()
        .map(|t| serde_json::json!({ "role": t.role.as_str(), "content": t.content }))
        .collect();
    if let Some(current) = body["messages"].as_array() {
        messages.extend(current.iter().cloned());
    }
    body["messages"] = serde_json::Value::Array(messages);
}

/// Shared trim/filter so each provider injects the system prompt only when
/// it is set and non-empty.
pub(crate) fn non_empty_system(opts: &RequestOptions) -> Option<&str> {
    opts.system.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

/// What a provider reported about a completed request: token counts, plus the
/// model it says it actually served. Feeds the usage log — never the request
/// path, so a provider that reports nothing changes no behavior.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResponseUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    /// Provider-reported model id. `None` = fall back to `default_model()`.
    pub model: Option<String>,
}

/// The shape every `parse_usage` impl shares: parse the body, read two token
/// pointers and a model pointer. `None` when the body isn't JSON or the
/// provider reported neither token count — a renamed or dropped field costs the
/// stats entry, never the completion.
pub(crate) fn usage_from(
    body: &str,
    input_ptr: &str,
    output_ptr: &str,
    model_ptr: &str,
) -> Option<ResponseUsage> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    let input = v.pointer(input_ptr).and_then(serde_json::Value::as_u64);
    let output = v.pointer(output_ptr).and_then(serde_json::Value::as_u64);
    if input.is_none() && output.is_none() {
        return None;
    }
    Some(ResponseUsage {
        input_tokens: input.unwrap_or(0) as u32,
        output_tokens: output.unwrap_or(0) as u32,
        model: v
            .pointer(model_ptr)
            .and_then(|m| m.as_str())
            .map(str::to_string)
            .filter(|s| !s.is_empty()),
    })
}

/// Adding a provider = one new file implementing this + one registry line.
pub trait AiProvider: Sync {
    #[allow(dead_code)] // part of the provider contract; exercised by registry tests
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

    /// The model this provider uses when `ProviderSettings.model` is "" — the
    /// same constant each impl already feeds to `or_default`. Lets the usage log
    /// record an effective model instead of an empty string.
    fn default_model(&self) -> &'static str;

    /// Token counts from a 2xx body. Defaults to `None` so a provider that
    /// reports no usage (or changes its field names) simply records no tokens.
    /// Must never fail the request.
    fn parse_usage(&self, _body: &str) -> Option<ResponseUsage> {
        None
    }

    /// Does this provider offer a native web-search tool? Overridden per
    /// provider in Phase 2; defaults to false so ollama/openai_compatible
    /// silently no-op.
    fn supports_web_search(&self) -> bool {
        false
    }

    /// Can this provider's configured `model` accept image input? Defaults to
    /// false; cloud providers override to true, ollama uses a model-name
    /// heuristic. Drives the assistant's warn-and-block guard for attachments.
    fn supports_images(&self, _model: &str) -> bool {
        false
    }

    /// Build a request honoring `RequestOptions`. Defaults to `build_request`
    /// (ignoring options) so providers opt in by overriding only this.
    fn build_request_with(
        &self,
        cfg: &crate::config::ProviderSettings,
        api_key: Option<&str>,
        prompt: &str,
        _opts: RequestOptions,
    ) -> Result<HttpRequest, String> {
        self.build_request(cfg, api_key, prompt)
    }
}

pub mod anthropic;
pub mod deepseek;
pub mod fallback;
pub mod gemini;
pub mod ollama;
pub mod openai_compatible;

pub fn provider(id: &str) -> Result<&'static dyn AiProvider, String> {
    match id {
        "openai_compatible" => Ok(&openai_compatible::OpenAiCompatible),
        "ollama" => Ok(&ollama::Ollama),
        "anthropic" => Ok(&anthropic::Anthropic),
        "gemini" => Ok(&gemini::Gemini),
        "deepseek" => Ok(&deepseek::DeepSeek),
        _ => Err(format!("Unknown AI provider \"{id}\"")),
    }
}

/// "" or whitespace in settings = use the provider's built-in default.
pub(crate) fn or_default<'a>(value: &'a str, default: &'a str) -> &'a str {
    let v = value.trim();
    if v.is_empty() { default } else { v }
}

/// The model a request actually ran against, for the usage log: what the
/// provider reported wins (gateways resolve aliases and dated ids), else the
/// configured model, else the provider default. Never empty — cost attribution
/// needs something to key on.
pub fn effective_model(p: &dyn AiProvider, cfg_model: &str, reported: Option<&str>) -> String {
    match reported.map(str::trim).filter(|m| !m.is_empty()) {
        Some(m) => m.to_string(),
        None => or_default(cfg_model, p.default_model()).to_string(),
    }
}

/// A failed provider call. `retryable` means the failure looks transient and
/// model-specific — the provider is saturated, not the account — so the same
/// request may still succeed against another model on the same provider.
#[derive(Debug, Clone)]
pub struct ProviderError {
    pub message: String,
    pub retryable: bool,
}

impl ProviderError {
    /// Nothing another model could fix: a bad key, a malformed request, or a
    /// local network problem.
    fn fatal(message: String) -> Self {
        Self { message, retryable: false }
    }
}

/// Statuses worth re-trying against a different model.
///
/// 429 is included because provider quotas are usually per-model, so a sibling
/// model often has headroom left. 401/403 deliberately are NOT: a bad key
/// fails identically on every model, and burning through the chain would just
/// multiply the failures (and the Usage-page noise) for nothing.
pub fn is_overloaded(status: u16) -> bool {
    matches!(status, 429 | 500 | 502 | 503 | 504 | 529)
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
        // Peak-hour saturation. The wording is load-bearing: `ErrorKind::classify`
        // matches "is overloaded" to bucket these on the Usage page.
        500 | 502 | 503 | 504 | 529 => {
            format!("Provider is overloaded (HTTP {status}: {detail})")
        }
        _ => format!("Provider returned HTTP {status}: {detail}"),
    }
}

/// Execute a built request; returns the raw success body.
///
/// Transport failures are never `retryable`: a timeout would spend another
/// full `REQUEST_TIMEOUT_SECS` on a guess, and a connect error is local — no
/// model on any provider can fix either.
pub async fn send(req: HttpRequest) -> Result<String, ProviderError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .build()
        .map_err(|e| ProviderError::fatal(e.to_string()))?;
    let mut r = client.post(&req.url).json(&req.body);
    for (name, value) in &req.headers {
        r = r.header(*name, value);
    }
    let resp = r.send().await.map_err(|e| {
        ProviderError::fatal(if e.is_timeout() {
            format!("Request timed out after {REQUEST_TIMEOUT_SECS} s")
        } else if e.is_connect() {
            format!("Could not connect to {}", req.url)
        } else {
            e.to_string()
        })
    })?;
    let status = resp.status().as_u16();
    let body = resp.text().await.map_err(|e| ProviderError::fatal(e.to_string()))?;
    if !(200..300).contains(&status) {
        return Err(ProviderError {
            message: friendly_http_error(status, &body),
            retryable: is_overloaded(status),
        });
    }
    Ok(body)
}

/// Which flow made this call, for the usage log. Deliberately NOT part of
/// `RequestOptions`: that struct is handed to `build_request_with`, and
/// providers have no business seeing attribution data.
#[derive(Debug, Clone)]
pub enum UsageSource {
    Prompt { id: String, name: String },
    Assistant,
    /// The quick-ask assistant folding its own conversation memory.
    AssistantSummary,
    ReminderParse,
    /// Connection tests and anything else that isn't the user doing work.
    NotRecorded,
}

/// Log one provider round-trip. Infallible by construction — a stats write
/// must never affect the completion it is measuring.
fn record_usage(
    app: &AppHandle,
    source: &UsageSource,
    provider_id: &str,
    model: &str,
    started: std::time::Instant,
    usage: Option<&ResponseUsage>,
    error: Option<&str>,
) {
    let kind = match source {
        UsageSource::NotRecorded => return,
        UsageSource::Prompt { .. } => crate::usage::Source::Prompt,
        UsageSource::Assistant => crate::usage::Source::Assistant,
        UsageSource::AssistantSummary => crate::usage::Source::AssistantSummary,
        UsageSource::ReminderParse => crate::usage::Source::ReminderParse,
    };
    let mut event = crate::usage::UsageEvent::now(kind);
    if let UsageSource::Prompt { id, name } = source {
        event.prompt_id = id.clone();
        event.prompt_name = name.clone();
    }
    event.provider = provider_id.to_string();
    event.model = model.to_string();
    event.duration_ms = started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
    if let Some(u) = usage {
        event.input_tokens = u.input_tokens;
        event.output_tokens = u.output_tokens;
    }
    if let Some(message) = error {
        event.ok = false;
        // The category only — the message can quote request content.
        event.error_kind = crate::usage::ErrorKind::classify(message);
    }
    crate::usage::record(app, event);
}

/// One round-trip against one model. `started` rides along so the usage log
/// times each attempt separately instead of billing the whole chain to the
/// model that happened to answer.
pub(crate) struct Attempt {
    pub model: String,
    pub started: std::time::Instant,
    pub usage: Option<ResponseUsage>,
    pub outcome: Result<String, ProviderError>,
}

/// Try each candidate model in order, stopping at the first success or the
/// first failure another model could not fix.
///
/// Takes no `AppHandle` on purpose: the whole fallback behaviour is then
/// testable against a mock server without a Tauri runtime.
pub(crate) async fn attempt_chain(
    p: &dyn AiProvider,
    cfg: &crate::config::ProviderSettings,
    api_key: Option<&str>,
    prompt: &str,
    opts: &RequestOptions,
    candidates: &[String],
) -> Vec<Attempt> {
    let mut attempts = Vec::with_capacity(candidates.len());
    for model in candidates {
        // Every provider reads the model off `cfg` — gemini into the URL, the
        // other three into the body — so swapping it here is all it takes.
        let cfg = crate::config::ProviderSettings { model: model.clone(), ..cfg.clone() };
        let started = std::time::Instant::now();

        // Usage survives a `parse_response` failure: the provider still billed
        // the tokens even when we couldn't read the answer.
        let mut usage: Option<ResponseUsage> = None;
        let outcome = match p.build_request_with(&cfg, api_key, prompt, opts.clone()) {
            Err(e) => Err(ProviderError::fatal(e)),
            Ok(req) => match send(req).await {
                Err(e) => Err(e),
                Ok(body) => {
                    usage = p.parse_usage(&body);
                    p.parse_response(&body).map_err(ProviderError::fatal)
                }
            },
        };

        let keep_going = outcome.as_ref().err().is_some_and(|e| e.retryable);
        attempts.push(Attempt { model: model.clone(), started, usage, outcome });
        if !keep_going {
            break;
        }
    }
    attempts
}

/// Emitted whenever a request was served by a model other than the configured
/// one, so an open window can explain the answer it is showing.
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FallbackEvent {
    pub provider: String,
    pub from: String,
    pub to: String,
}

pub const FALLBACK_EVENT: &str = "ai-fallback";

/// Tell the user their model was swapped out from under them.
///
/// The event fires on every fallback — a panel should always be able to name
/// the model that answered. The desktop notification fires only on the FIRST
/// failure of a cooldown window, or a peak-hour outage would produce one toast
/// per request. Both are fire-and-forget: warning about a completion must
/// never be able to fail it.
fn announce_fallback(
    app: &AppHandle,
    notifications_enabled: bool,
    provider_id: &str,
    from: &str,
    to: &str,
    first_failure: bool,
) {
    let _ = app.emit(
        FALLBACK_EVENT,
        FallbackEvent {
            provider: provider_id.to_string(),
            from: from.to_string(),
            to: to.to_string(),
        },
    );
    if first_failure {
        let minutes = fallback::COOLDOWN_SECS / 60;
        crate::notify::send(
            app,
            notifications_enabled,
            &format!("{from} is overloaded — using {to} for the next {minutes} minutes."),
        );
    }
}

/// Core completion against a SPECIFIC provider with explicit request options.
/// Uses `build_request_with`, so a provider that ignores options behaves
/// exactly as before.
///
/// When the configured model is overloaded this walks its fallback chain
/// rather than failing (see `fallback`). Every attempt past the config load is
/// recorded — including the failures, which is the whole point of the Failures
/// tile on the Usage page.
pub async fn complete_provider_with_options(
    app: &AppHandle,
    provider_id: &str,
    prompt: &str,
    opts: RequestOptions,
    source: UsageSource,
) -> Result<String, String> {
    complete_core(app, provider_id, prompt, opts, source, true).await
}

/// `complete_provider_with_options` with the fallback chain switchable off for
/// this one call, whatever the setting says. Only the connection test wants
/// that: it exists to prove the CONFIGURED model works, so being rescued by a
/// sibling would make it report a success the user does not have.
async fn complete_core(
    app: &AppHandle,
    provider_id: &str,
    prompt: &str,
    opts: RequestOptions,
    source: UsageSource,
    allow_fallback: bool,
) -> Result<String, String> {
    let settings = crate::config::load(app)?;
    let p = provider(provider_id)?;
    let cfg = settings.ai.provider(provider_id)?;
    let key = crate::secrets::get(provider_id)?;

    // A missing key fails identically on every model, so it must never spend
    // the chain. Recorded like any other failure.
    if p.requires_api_key() && key.is_none() {
        let e = format!("No API key configured for {provider_id} — add one on the Providers page");
        let model = effective_model(p, &cfg.model, None);
        record_usage(app, &source, provider_id, &model, std::time::Instant::now(), None, Some(&e));
        return Err(e);
    }

    // What the user actually picked — the yardstick for "did we substitute?",
    // independent of how `candidates` reorders around cooldowns.
    let configured = or_default(&cfg.model, p.default_model()).to_string();
    let candidates = if allow_fallback && settings.ai.model_fallback_enabled {
        fallback::candidates(provider_id, &cfg.model, &cfg.fallback_models, p.default_model())
    } else {
        vec![configured.clone()]
    };

    let attempts = attempt_chain(p, cfg, key.as_deref(), prompt, &opts, &candidates).await;

    // `true` once the chain took a model OUT of service, as opposed to finding
    // it already cooling from an earlier request. Gates the desktop toast.
    let mut first_failure = false;
    for a in &attempts {
        record_usage(
            app,
            &source,
            provider_id,
            &effective_model(p, &a.model, a.usage.as_ref().and_then(|u| u.model.as_deref())),
            a.started,
            a.usage.as_ref(),
            a.outcome.as_ref().err().map(|e| e.message.as_str()),
        );
        match &a.outcome {
            // It answered, so it is healthy — end any cooldown early.
            Ok(_) => fallback::clear(provider_id, &a.model),
            Err(e) if e.retryable => {
                first_failure |= fallback::mark_overloaded(provider_id, &a.model);
            }
            Err(_) => {}
        }
    }

    // `candidates` is never empty, so neither is `attempts`; the guard is here
    // so a future caller can't turn that invariant into a panic.
    let Some(last) = attempts.last() else {
        return Err(format!("No model configured for {provider_id}"));
    };
    match &last.outcome {
        Ok(answer) => {
            if last.model != configured {
                announce_fallback(
                    app,
                    settings.notifications_enabled,
                    provider_id,
                    &configured,
                    &last.model,
                    first_failure,
                );
            }
            Ok(answer.clone())
        }
        Err(e) => Err(e.message.clone()),
    }
}

/// Full completion against a SPECIFIC provider. No request options — plain
/// behavior, fallback chain included.
pub async fn complete_with(
    app: &AppHandle,
    provider_id: &str,
    prompt: &str,
    source: UsageSource,
) -> Result<String, String> {
    complete_provider_with_options(app, provider_id, prompt, RequestOptions::default(), source)
        .await
}

/// Full completion against the ACTIVE provider from settings.
pub async fn complete(
    app: &AppHandle,
    prompt: &str,
    source: UsageSource,
) -> Result<String, String> {
    let id = crate::config::load(app)?.ai.active_provider;
    complete_with(app, &id, prompt, source).await
}

/// Full completion against the ACTIVE provider honoring request options
/// (used by the quick-ask assistant for provider-native web search).
pub async fn complete_with_options(
    app: &AppHandle,
    prompt: &str,
    opts: RequestOptions,
    source: UsageSource,
) -> Result<String, String> {
    let id = crate::config::load(app)?.ai.active_provider;
    complete_provider_with_options(app, &id, prompt, opts, source).await
}

/// Whether the ACTIVE provider offers native web search (assistant uses this
/// to decide whether to request it).
pub fn active_provider_supports_web_search(app: &AppHandle) -> Result<bool, String> {
    let id = crate::config::load(app)?.ai.active_provider;
    Ok(provider(&id)?.supports_web_search())
}

/// Whether the ACTIVE provider + its configured model can accept images. The
/// quick-ask box calls this to gate image attachments (warn-and-block) and the
/// `ask` flow uses it as a backstop before sending. Empty model = provider
/// default, which each `supports_images` impl resolves on its own.
#[tauri::command]
pub fn active_provider_supports_images(app: AppHandle) -> Result<bool, String> {
    let settings = crate::config::load(&app)?;
    let id = &settings.ai.active_provider;
    let model = settings.ai.provider(id)?.model.clone();
    Ok(provider(id)?.supports_images(&model))
}

/// Connection test for the Providers page: cheapest possible round trip
/// that proves endpoint + key + model all work. Deliberately does NOT fall
/// back — a green result has to mean the model you configured answered.
#[tauri::command]
pub async fn test_provider(app: AppHandle, provider_id: String) -> Result<String, String> {
    // NotRecorded: a connection test is plumbing, not usage — counting it
    // would inflate the run count every time you visit the Providers page.
    complete_core(
        &app,
        &provider_id,
        "Reply with exactly: OK",
        RequestOptions::default(),
        UsageSource::NotRecorded,
        false,
    )
    .await
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

    #[test]
    fn providers_without_web_search_ignore_options_and_stay_identical() {
        // Providers without web search must ignore the switch entirely:
        // build_request_with == build_request when only web_search is set.
        let cfg = crate::config::ProviderSettings {
            base_url: "http://x/v1".into(),
            model: "m".into(),
            ..Default::default()
        };
        for id in ["ollama", "openai_compatible", "deepseek"] {
            let p = provider(id).unwrap();
            assert!(!p.supports_web_search(), "{id} should not support web search");
            let plain = p.build_request(&cfg, Some("k"), "Hi").unwrap();
            let with = p
                .build_request_with(
                    &cfg,
                    Some("k"),
                    "Hi",
                    RequestOptions { web_search: true, ..Default::default() },
                )
                .unwrap();
            assert_eq!(plain.url, with.url, "{id} url changed");
            assert_eq!(plain.body, with.body, "{id} body changed despite no web search");
        }
    }

    #[test]
    fn default_options_have_no_system_prompt_so_dictation_flow_is_unchanged() {
        // `complete()` / `complete_with()` (used by prompt_flow) build
        // RequestOptions::default() — pin that this means "no system prompt"
        // and that every provider's request body stays identical to the plain
        // build_request shape.
        assert!(RequestOptions::default().system.is_none());
        assert!(RequestOptions::default().images.is_empty());
        // The third thing the default shape must not change: quick-chat
        // history. Every non-assistant caller must keep sending single-turn
        // requests byte-for-byte.
        assert!(RequestOptions::default().history.is_empty());
        let cfg = crate::config::ProviderSettings {
            base_url: "http://x/v1".into(),
            model: "m".into(),
            ..Default::default()
        };
        for id in crate::config::PROVIDER_IDS {
            let p = provider(id).unwrap();
            let plain = p.build_request(&cfg, Some("k"), "Hi").unwrap();
            let with = p
                .build_request_with(&cfg, Some("k"), "Hi", RequestOptions::default())
                .unwrap();
            assert_eq!(plain.body, with.body, "{id} body changed with default options");
        }
    }

    /// Two prior turns, the shape `assistant::conversation` always produces:
    /// complete user/assistant pairs, oldest first.
    fn sample_history() -> Vec<HistoryTurn> {
        vec![
            HistoryTurn { role: Role::User, content: "What is Tauri?".into() },
            HistoryTurn { role: Role::Assistant, content: "A desktop framework.".into() },
        ]
    }

    #[test]
    fn prepend_history_is_a_no_op_on_empty_history() {
        let mut body = serde_json::json!({ "messages": [{ "role": "user", "content": "Hi" }] });
        let before = body.clone();
        prepend_history(&mut body, &[]);
        assert_eq!(body, before, "an empty session must not touch the body");
    }

    #[test]
    fn history_never_displaces_the_current_prompt_for_any_provider() {
        // Whatever each provider does with history, the question the user just
        // asked must stay LAST — it is the thing being answered.
        let cfg = crate::config::ProviderSettings {
            base_url: "http://x/v1".into(),
            model: "m".into(),
            ..Default::default()
        };
        let opts = RequestOptions { history: sample_history(), ..Default::default() };
        for id in crate::config::PROVIDER_IDS {
            let req = provider(id)
                .unwrap()
                .build_request_with(&cfg, Some("k"), "Hi", opts.clone())
                .unwrap();
            // Gemini keeps turns under `contents`; everyone else under `messages`.
            let turns = req.body["messages"]
                .as_array()
                .or_else(|| req.body["contents"].as_array())
                .unwrap_or_else(|| panic!("{id} has neither messages nor contents"));
            let last = turns.last().unwrap();
            let text = last["content"]
                .as_str()
                .or_else(|| last["parts"][0]["text"].as_str())
                .unwrap_or_else(|| panic!("{id} last turn has no text"));
            assert_eq!(text, "Hi", "{id} displaced the current question");
            assert_eq!(turns.len(), 3, "{id} should carry 2 history turns + the current one");
        }
    }

    #[test]
    fn history_roles_use_each_providers_vocabulary() {
        assert_eq!(Role::User.as_str(), "user");
        assert_eq!(Role::Assistant.as_str(), "assistant");

        // Gemini is the exception: the assistant side is called "model".
        let cfg = crate::config::ProviderSettings::default();
        let req = provider("gemini")
            .unwrap()
            .build_request_with(
                &cfg,
                Some("k"),
                "Hi",
                RequestOptions { history: sample_history(), ..Default::default() },
            )
            .unwrap();
        assert_eq!(req.body["contents"][0]["role"], "user");
        assert_eq!(req.body["contents"][1]["role"], "model");
    }

    #[test]
    fn non_empty_system_filters_none_empty_and_whitespace() {
        let none = RequestOptions::default();
        assert_eq!(non_empty_system(&none), None);
        let empty = RequestOptions { system: Some("   ".into()), ..Default::default() };
        assert_eq!(non_empty_system(&empty), None);
        let set = RequestOptions { system: Some(" Be brief.\nUse lists. ".into()), ..Default::default() };
        assert_eq!(non_empty_system(&set), Some("Be brief.\nUse lists."));
    }

    #[test]
    fn usage_from_reads_both_counts_and_the_model() {
        let u = usage_from(
            r#"{"usage":{"input_tokens":412,"output_tokens":96},"model":"claude-sonnet-5"}"#,
            "/usage/input_tokens",
            "/usage/output_tokens",
            "/model",
        )
        .unwrap();
        assert_eq!(u.input_tokens, 412);
        assert_eq!(u.output_tokens, 96);
        assert_eq!(u.model.as_deref(), Some("claude-sonnet-5"));
    }

    #[test]
    fn usage_from_is_none_when_the_provider_reports_nothing() {
        // No usage block at all (many local OpenAI-compatible servers), a body
        // that isn't JSON, and an empty model string must all degrade quietly.
        assert!(usage_from(r#"{"choices":[]}"#, "/usage/a", "/usage/b", "/model").is_none());
        assert!(usage_from("not json", "/usage/a", "/usage/b", "/model").is_none());
        let no_model = usage_from(r#"{"usage":{"a":1},"model":""}"#, "/usage/a", "/usage/b", "/model")
            .unwrap();
        assert_eq!(no_model.model, None, "an empty model must not be recorded as one");
        assert_eq!(no_model.output_tokens, 0, "a missing count is zero, not a failure");
    }

    #[test]
    fn every_provider_reports_a_default_model_and_ignores_garbage_bodies() {
        for id in crate::config::PROVIDER_IDS {
            let p = provider(id).unwrap();
            assert!(!p.default_model().is_empty(), "{id} has no default model");
            assert!(p.parse_usage("").is_none(), "{id} choked on an empty body");
            assert!(p.parse_usage("<html>502</html>").is_none(), "{id} choked on HTML");
            assert!(p.parse_usage("{}").is_none(), "{id} invented usage from an empty object");
        }
    }

    #[test]
    fn effective_model_prefers_what_the_provider_reported() {
        let p = provider("anthropic").unwrap();
        // A gateway resolving an alias to a dated id — that's what gets billed.
        assert_eq!(effective_model(p, "claude-sonnet-5", Some("claude-sonnet-5-20260514")), "claude-sonnet-5-20260514");
        // Nothing reported: fall back to the configured model...
        assert_eq!(effective_model(p, "claude-opus-5", None), "claude-opus-5");
        // ...and with no configured model, the provider's own default.
        assert_eq!(effective_model(p, "", None), anthropic::DEFAULT_MODEL);
        assert_eq!(effective_model(p, "  ", Some("  ")), anthropic::DEFAULT_MODEL);
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
            ..Default::default()
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
            ..Default::default()
        };
        let p = provider("openai_compatible").unwrap();
        let req = p.build_request(&cfg, Some("bad"), "ping").unwrap();
        let err = send(req).await.unwrap_err();
        assert!(
            err.message.contains("Authentication failed") && err.message.contains("invalid api key"),
            "got: {}",
            err.message
        );
        assert!(!err.retryable, "a bad key fails on every model — never walk the chain");
    }

    #[test]
    fn only_saturation_statuses_are_worth_another_model() {
        // Overloaded, gateway-level, and Anthropic's 529. 429 counts because
        // provider quotas are usually per-model.
        for status in [429, 500, 502, 503, 504, 529] {
            assert!(is_overloaded(status), "{status} should fall back");
        }
        // A bad key, a malformed body, or a model that does not exist: every
        // model on the chain would fail exactly the same way.
        for status in [400, 401, 403, 404, 413, 422] {
            assert!(!is_overloaded(status), "{status} must not fall back");
        }
    }

    #[test]
    fn overload_errors_are_phrased_so_the_usage_log_can_bucket_them() {
        let e = friendly_http_error(503, r#"{"error":{"message":"The model is overloaded."}}"#);
        assert!(e.contains("is overloaded"), "got: {e}");
        assert!(e.contains("The model is overloaded."), "the provider's own words must survive");
        assert_eq!(crate::usage::ErrorKind::classify(&e), crate::usage::ErrorKind::Overloaded);
        // Anthropic's non-standard code gets the same treatment.
        assert!(friendly_http_error(529, "{}").contains("is overloaded"));
    }

    /// An OpenAI-shaped mock that fails one model and serves another, which is
    /// exactly the peak-hour shape this feature exists for.
    fn chain_server(bad_model: &str, bad_status: u16) -> httpmock::MockServer {
        let server = httpmock::MockServer::start();
        let bad = bad_model.to_string();
        server.mock(|when, then| {
            when.method(httpmock::Method::POST)
                .path("/v1/chat/completions")
                .json_body_partial(format!(r#"{{"model":"{bad}"}}"#));
            then.status(bad_status)
                .json_body(serde_json::json!({ "error": { "message": "overloaded" } }));
        });
        server.mock(|when, then| {
            when.method(httpmock::Method::POST)
                .path("/v1/chat/completions")
                .json_body_partial(r#"{"model":"spare"}"#);
            then.status(200).json_body(serde_json::json!({
                "choices": [{ "message": { "role": "assistant", "content": "OK" } }]
            }));
        });
        server
    }

    fn chain_cfg(server: &httpmock::MockServer) -> crate::config::ProviderSettings {
        crate::config::ProviderSettings {
            base_url: format!("{}/v1", server.base_url()),
            model: "busy".into(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn an_overloaded_model_is_answered_by_the_next_one_in_the_chain() {
        let server = chain_server("busy", 503);
        let cfg = chain_cfg(&server);
        let p = provider("openai_compatible").unwrap();
        let candidates = vec!["busy".to_string(), "spare".to_string()];
        let attempts =
            attempt_chain(p, &cfg, Some("k"), "ping", &RequestOptions::default(), &candidates).await;

        assert_eq!(attempts.len(), 2, "the chain should have moved on after the 503");
        assert_eq!(attempts[0].model, "busy");
        assert!(attempts[0].outcome.as_ref().unwrap_err().retryable);
        assert_eq!(attempts[1].model, "spare");
        assert_eq!(attempts[1].outcome.as_ref().unwrap(), "OK", "the spare model's answer");
    }

    #[tokio::test]
    async fn a_rate_limited_model_also_falls_through_to_the_next() {
        // 429 is the other peak-hour face of the same problem.
        let server = chain_server("busy", 429);
        let cfg = chain_cfg(&server);
        let p = provider("openai_compatible").unwrap();
        let candidates = vec!["busy".to_string(), "spare".to_string()];
        let attempts =
            attempt_chain(p, &cfg, Some("k"), "ping", &RequestOptions::default(), &candidates).await;
        assert_eq!(attempts.len(), 2);
        assert!(attempts[1].outcome.is_ok());
    }

    #[tokio::test]
    async fn an_auth_failure_stops_the_chain_after_one_attempt() {
        // Walking the chain on a bad key would multiply one failure into four
        // — same error, four times the latency and four Usage-page rows.
        let server = chain_server("busy", 401);
        let cfg = chain_cfg(&server);
        let p = provider("openai_compatible").unwrap();
        let candidates = vec!["busy".to_string(), "spare".to_string()];
        let attempts =
            attempt_chain(p, &cfg, Some("k"), "ping", &RequestOptions::default(), &candidates).await;

        assert_eq!(attempts.len(), 1, "an auth failure must not burn the chain");
        assert!(!attempts[0].outcome.as_ref().unwrap_err().retryable);
    }

    #[tokio::test]
    async fn a_single_candidate_behaves_exactly_like_the_old_single_shot() {
        let server = chain_server("busy", 503);
        let cfg = chain_cfg(&server);
        let p = provider("openai_compatible").unwrap();
        let attempts = attempt_chain(
            p,
            &cfg,
            Some("k"),
            "ping",
            &RequestOptions::default(),
            &["busy".to_string()],
        )
        .await;
        assert_eq!(attempts.len(), 1);
        let err = attempts[0].outcome.as_ref().unwrap_err();
        assert!(err.message.contains("is overloaded"), "got: {}", err.message);
    }
}
