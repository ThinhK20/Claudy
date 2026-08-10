//! Local-only usage log: what ran, when, how many tokens, what it cost.
//!
//! **Metadata only.** No prompt text, no selected text, no AI response, and no
//! error *message* — only a coarse `ErrorKind`. Nothing here is ever sent
//! anywhere; it exists so the Usage page can answer "what am I actually using
//! and what is it costing me" without weakening the app's privacy posture.
//!
//! Recording is on by default and prunable — see `UsageSettings` in `config`.

use std::sync::Mutex;

use chrono::{DateTime, Duration, Local};
use serde::{Deserialize, Deserializer, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_store::StoreExt;

pub mod pricing;
pub mod summary;

pub use pricing::ModelPrice;
pub use summary::{Range, UsageSummary};

pub const STORE_FILE: &str = "usage.json";
const KEY: &str = "events";

/// Emitted after every write so an open Usage page refreshes without polling
/// (the `reminders-changed` pattern).
pub const CHANGED_EVENT: &str = "usage-changed";

/// Hard ceiling regardless of the retention setting. Each event is ~200 bytes,
/// so this bounds `usage.json` at a few megabytes even if someone sets
/// retention to "keep forever" and then scripts a thousand runs a day.
pub const MAX_EVENTS: usize = 20_000;

/// What produced a run. Reminder activity is split three ways because a
/// created reminder, a fired reminder, and the AI time-parse fallback are
/// genuinely different events — collapsing them would make the Reminders row
/// meaningless.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    Prompt,
    Assistant,
    /// Quick Chat compacting its own conversation memory. A real billed call
    /// the user didn't explicitly ask for, so it gets its own line rather than
    /// hiding inside `Assistant`.
    AssistantSummary,
    ReminderParse,
    ReminderCreated,
    ReminderFired,
    /// Sink for a value this build doesn't recognize — a hand-edited file or a
    /// log written by a newer version. Counted, but never miscounted as a real
    /// source.
    #[default]
    Other,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Prompt => "prompt",
            Self::Assistant => "assistant",
            Self::AssistantSummary => "assistantSummary",
            Self::ReminderParse => "reminderParse",
            Self::ReminderCreated => "reminderCreated",
            Self::ReminderFired => "reminderFired",
            Self::Other => "other",
        }
    }
}

// Hand-written like `reminders::Status`: usage.json is hand-editable, and one
// unknown source string must not fail the load of the whole log.
impl<'de> Deserialize<'de> for Source {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(match String::deserialize(d).unwrap_or_default().as_str() {
            "prompt" => Self::Prompt,
            "assistant" => Self::Assistant,
            "assistantSummary" => Self::AssistantSummary,
            "reminderParse" => Self::ReminderParse,
            "reminderCreated" => Self::ReminderCreated,
            "reminderFired" => Self::ReminderFired,
            _ => Self::Other,
        })
    }
}

/// Why a run failed — a *category*, never the provider's message. The message
/// can quote request content, so it is deliberately not persisted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorKind {
    #[default]
    None,
    Auth,
    RateLimit,
    Timeout,
    Other,
}

impl ErrorKind {
    /// The wire name serde emits — pinned by the round-trip test so a rename
    /// can't silently change what lands in `usage.json`.
    #[allow(dead_code)]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Auth => "auth",
            Self::RateLimit => "rateLimit",
            Self::Timeout => "timeout",
            Self::Other => "other",
        }
    }

    /// Bucket a provider error without keeping it. Matches the phrasing
    /// `ai::friendly_http_error` and `ai::send` produce.
    pub fn classify(message: &str) -> Self {
        let m = message.to_ascii_lowercase();
        if m.contains("authentication failed") || m.contains("api key") {
            Self::Auth
        } else if m.contains("rate limited") {
            Self::RateLimit
        } else if m.contains("timed out") {
            Self::Timeout
        } else {
            Self::Other
        }
    }
}

impl<'de> Deserialize<'de> for ErrorKind {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(match String::deserialize(d).unwrap_or_default().as_str() {
            "auth" => Self::Auth,
            "rateLimit" => Self::RateLimit,
            "timeout" => Self::Timeout,
            "other" => Self::Other,
            _ => Self::None,
        })
    }
}

/// One recorded run. Every field is metadata — see the module docs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct UsageEvent {
    pub id: String,
    /// RFC3339 with offset, e.g. "2026-07-26T14:05:12+07:00" — the same
    /// convention as `Reminder.due_at`, so the file stays readable and the
    /// frontend can parse it directly.
    pub at: String,
    pub source: Source,
    /// "" unless `source == Prompt`.
    pub prompt_id: String,
    /// Snapshot at run time, so a deleted prompt still reads sensibly.
    pub prompt_name: String,
    /// "" for events that never touched a provider.
    pub provider: String,
    /// The model actually used — never "" for an AI call.
    pub model: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub duration_ms: u32,
    pub ok: bool,
    pub error_kind: ErrorKind,
}

impl Default for UsageEvent {
    fn default() -> Self {
        Self {
            id: String::new(),
            at: String::new(),
            source: Source::Other,
            prompt_id: String::new(),
            prompt_name: String::new(),
            provider: String::new(),
            model: String::new(),
            input_tokens: 0,
            output_tokens: 0,
            duration_ms: 0,
            // A row that predates a future `ok` field is far more likely to be
            // a success than a failure; don't invent failures on load.
            ok: true,
            error_kind: ErrorKind::None,
        }
    }
}

impl UsageEvent {
    /// A new event stamped with a fresh id and the current local time.
    pub fn now(source: Source) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            at: Local::now().to_rfc3339(),
            source,
            ..Self::default()
        }
    }
}

/// Serializes the read-modify-write in `record`. The assistant, prompt, and
/// reminder paths all run on the async runtime and can overlap; without this
/// two concurrent appends would lose one of the events.
#[derive(Default)]
pub struct UsageState {
    lock: Mutex<()>,
}

pub fn load(app: &AppHandle) -> Result<Vec<UsageEvent>, String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    match store.get(KEY) {
        Some(v) => serde_json::from_value(v).map_err(|e| e.to_string()),
        None => Ok(Vec::new()),
    }
}

pub fn save_list(app: &AppHandle, events: &[UsageEvent]) -> Result<(), String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    let value = serde_json::to_value(events).map_err(|e| e.to_string())?;
    store.set(KEY, value);
    store.save().map_err(|e| e.to_string())
}

/// Drop what retention no longer covers, then enforce the hard ceiling.
/// `retention_days == 0` means keep forever — the cap still applies.
///
/// A row whose timestamp can't be parsed is dropped either way: it can never
/// age out on its own and the summary can't chart it, so keeping it would just
/// leak entries past the retention the user asked for.
pub fn prune(events: &mut Vec<UsageEvent>, retention_days: u32, now: DateTime<Local>) {
    let cutoff = (retention_days > 0).then(|| now - Duration::days(i64::from(retention_days)));
    events.retain(|e| match DateTime::parse_from_rfc3339(e.at.trim()) {
        Ok(t) => cutoff.is_none_or(|c| t >= c),
        Err(_) => false,
    });
    if events.len() > MAX_EVENTS {
        events.drain(..events.len() - MAX_EVENTS);
    }
}

/// Append one event. Silent no-op when recording is off — this is called from
/// hot paths (every prompt run, every reminder) and must never surface an
/// error to the user for a stats write.
pub fn record(app: &AppHandle, event: UsageEvent) {
    let settings = match crate::config::load(app) {
        Ok(s) => s,
        Err(_) => return,
    };
    if !settings.usage.enabled {
        return;
    }
    // A poisoned lock means some other thread panicked mid-append; recover the
    // guard rather than losing usage recording for the rest of the session.
    let state = app.state::<UsageState>();
    let _guard = state.lock.lock().unwrap_or_else(|e| e.into_inner());

    let mut events = load(app).unwrap_or_default();
    events.push(event);
    prune(&mut events, settings.usage.retention_days, Local::now());
    if save_list(app, &events).is_ok() {
        let _ = app.emit(CHANGED_EVENT, ());
    }
}

/// A model the pricing editor can show: what it costs now, where that price
/// came from, and whether it has actually been used.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelPriceRow {
    pub model: String,
    /// "" for a builtin-table entry that has never run here.
    pub provider: String,
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
    /// A user override is in effect for this model.
    pub overridden: bool,
    /// False = tokens are counted but no cost can be shown.
    pub priced: bool,
    /// Appears in the usage log — the rows worth editing first.
    pub in_use: bool,
}

/// Pure: build the editor's rows from the log, the overrides, and the table.
/// Models actually used come first, so the row you want is at the top.
fn price_rows(events: &[UsageEvent], overrides: &[ModelPrice]) -> Vec<ModelPriceRow> {
    let mut rows: Vec<ModelPriceRow> = Vec::new();
    let mut push = |model: &str, provider: &str, in_use: bool| {
        if model.is_empty() || rows.iter().any(|r| r.model.eq_ignore_ascii_case(model)) {
            return;
        }
        let price = pricing::resolve(provider, model, overrides);
        rows.push(ModelPriceRow {
            model: model.to_string(),
            provider: provider.to_string(),
            input_per_mtok: price.map_or(0.0, |p| p.input_per_mtok),
            output_per_mtok: price.map_or(0.0, |p| p.output_per_mtok),
            overridden: overrides
                .iter()
                .any(|o| pricing::is_usable(o) && o.model.trim().eq_ignore_ascii_case(model)),
            priced: price.is_some(),
            in_use,
        });
    };

    for e in events {
        push(&e.model, &e.provider, true);
    }
    for o in overrides.iter().filter(|o| pricing::is_usable(o)) {
        push(o.model.trim(), "", false);
    }
    for b in pricing::builtin_models() {
        push(&b.model, "", false);
    }
    rows
}

#[tauri::command]
pub fn usage_summary(app: AppHandle, range: String) -> Result<UsageSummary, String> {
    let settings = crate::config::load(&app)?;
    let events = load(&app)?;
    Ok(summary::summarize(
        &events,
        range.parse().unwrap_or(Range::Week),
        Local::now(),
        &settings.usage.model_prices,
    ))
}

/// Wipe the log. Behind a confirmation dialog in the UI — there is no undo.
#[tauri::command]
pub fn clear_usage(app: AppHandle) -> Result<(), String> {
    let state = app.state::<UsageState>();
    let _guard = state.lock.lock().unwrap_or_else(|e| e.into_inner());
    save_list(&app, &[])?;
    let _ = app.emit(CHANGED_EVENT, ());
    Ok(())
}

#[tauri::command]
pub fn list_model_prices(app: AppHandle) -> Result<Vec<ModelPriceRow>, String> {
    let settings = crate::config::load(&app)?;
    Ok(price_rows(&load(&app)?, &settings.usage.model_prices))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn now() -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 7, 24, 14, 30, 0).unwrap()
    }

    fn aged(days: i64) -> UsageEvent {
        UsageEvent {
            at: (now() - Duration::days(days)).to_rfc3339(),
            source: Source::Prompt,
            ..UsageEvent::default()
        }
    }

    #[test]
    fn serializes_to_camel_case() {
        let v = serde_json::to_value(UsageEvent::default()).unwrap();
        assert!(v.get("promptId").is_some());
        assert!(v.get("prompt_id").is_none());
        assert!(v.get("inputTokens").is_some());
        assert!(v.get("errorKind").is_some());
        assert!(v.get("durationMs").is_some());
    }

    #[test]
    fn deserializes_camel_case_and_fills_missing_fields_with_defaults() {
        let json = serde_json::json!({
            "at": "2026-07-26T14:05:12+07:00",
            "source": "prompt",
            "promptName": "Fix grammar & spelling",
            "inputTokens": 412,
        });
        let e: UsageEvent = serde_json::from_value(json).unwrap();
        assert_eq!(e.source, Source::Prompt);
        assert_eq!(e.input_tokens, 412);
        assert_eq!(e.output_tokens, 0, "missing field -> default");
        assert_eq!(e.error_kind, ErrorKind::None);
        assert!(e.ok, "a row with no `ok` field is not a failure");
    }

    #[test]
    fn a_full_event_round_trips_unchanged() {
        let e = UsageEvent {
            id: "3f2a1b6c-0000-4000-8000-000000000001".into(),
            at: "2026-07-26T14:05:12+07:00".into(),
            source: Source::Assistant,
            prompt_id: String::new(),
            prompt_name: String::new(),
            provider: "anthropic".into(),
            model: "claude-sonnet-5".into(),
            input_tokens: 412,
            output_tokens: 96,
            duration_ms: 1840,
            ok: false,
            error_kind: ErrorKind::RateLimit,
        };
        let back: UsageEvent = serde_json::from_value(serde_json::to_value(&e).unwrap()).unwrap();
        assert_eq!(back, e);
    }

    #[test]
    fn one_bad_enum_string_does_not_fail_the_whole_load() {
        // usage.json is hand-editable; a typo must cost the field, not the file.
        let json = serde_json::json!([
            { "at": "2026-07-26T14:05:12+07:00", "source": "prmopt", "errorKind": "kaboom",
              "model": "claude-sonnet-5" },
            { "at": "2026-07-26T15:05:12+07:00", "source": "assistant" },
        ]);
        let list: Vec<UsageEvent> = serde_json::from_value(json).unwrap();
        assert_eq!(list.len(), 2, "the good row must survive");
        assert_eq!(list[0].source, Source::Other, "unknown source is not miscounted");
        assert_eq!(list[0].error_kind, ErrorKind::None);
        assert_eq!(list[0].model, "claude-sonnet-5", "the rest of the row survives");
        assert_eq!(list[1].source, Source::Assistant);
    }

    #[test]
    fn a_log_written_before_a_field_existed_still_loads() {
        // No migration: absent fields simply take their defaults.
        let json = serde_json::json!([{ "at": "2026-07-26T14:05:12+07:00" }]);
        let list: Vec<UsageEvent> = serde_json::from_value(json).unwrap();
        assert_eq!(list[0].source, Source::Other);
        assert_eq!(list[0].duration_ms, 0);
    }

    #[test]
    fn every_source_and_error_kind_round_trips_its_wire_name() {
        for s in [
            Source::Prompt,
            Source::Assistant,
            Source::ReminderParse,
            Source::ReminderCreated,
            Source::ReminderFired,
            Source::Other,
        ] {
            let wire = serde_json::to_value(s).unwrap();
            assert_eq!(wire, serde_json::json!(s.as_str()));
            assert_eq!(serde_json::from_value::<Source>(wire).unwrap(), s);
        }
        for k in [
            ErrorKind::None,
            ErrorKind::Auth,
            ErrorKind::RateLimit,
            ErrorKind::Timeout,
            ErrorKind::Other,
        ] {
            let wire = serde_json::to_value(k).unwrap();
            assert_eq!(wire, serde_json::json!(k.as_str()));
            assert_eq!(serde_json::from_value::<ErrorKind>(wire).unwrap(), k);
        }
    }

    #[test]
    fn provider_errors_are_bucketed_without_keeping_the_message() {
        // The exact strings `ai::friendly_http_error` and `ai::send` produce.
        assert_eq!(
            ErrorKind::classify("Authentication failed — check your API key (bad key)"),
            ErrorKind::Auth
        );
        assert_eq!(
            ErrorKind::classify("No API key configured for anthropic — add one on the Providers page"),
            ErrorKind::Auth
        );
        assert_eq!(
            ErrorKind::classify("Rate limited by the provider (slow down)"),
            ErrorKind::RateLimit
        );
        assert_eq!(ErrorKind::classify("Request timed out after 60 s"), ErrorKind::Timeout);
        assert_eq!(ErrorKind::classify("Provider returned HTTP 500: boom"), ErrorKind::Other);
    }

    #[test]
    fn retention_drops_events_older_than_the_window() {
        let mut events = vec![aged(100), aged(91), aged(89), aged(0)];
        prune(&mut events, 90, now());
        assert_eq!(events.len(), 2, "only the last 90 days survive");
        assert_eq!(events[0].at, aged(89).at);
    }

    #[test]
    fn zero_retention_keeps_everything() {
        let mut events = vec![aged(5000), aged(0)];
        prune(&mut events, 0, now());
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn the_hard_cap_applies_even_with_retention_off() {
        // Oldest-first drop, so the newest MAX_EVENTS are the ones kept.
        let mut events: Vec<UsageEvent> = (0..MAX_EVENTS + 10).map(|_| aged(0)).collect();
        events[0].prompt_id = "oldest".into();
        events[MAX_EVENTS + 9].prompt_id = "newest".into();
        prune(&mut events, 0, now());
        assert_eq!(events.len(), MAX_EVENTS);
        assert_eq!(events.last().unwrap().prompt_id, "newest");
        assert!(events.iter().all(|e| e.prompt_id != "oldest"));
    }

    #[test]
    fn an_unparseable_timestamp_is_pruned_rather_than_kept_forever() {
        // It can never age out and the summary can't chart it, so keeping it
        // would leak rows past the retention the user asked for.
        let mut events =
            vec![UsageEvent { at: "whenever".into(), ..UsageEvent::default() }, aged(0)];
        prune(&mut events, 90, now());
        assert_eq!(events.len(), 1);
        prune(&mut events, 0, now());
        assert_eq!(events.len(), 1, "dropped under keep-forever too");
    }

    #[test]
    fn a_new_event_is_stamped_and_defaults_to_success() {
        let e = UsageEvent::now(Source::Assistant);
        assert_eq!(e.source, Source::Assistant);
        assert!(e.ok && e.error_kind == ErrorKind::None);
        assert!(!e.id.is_empty());
        assert!(DateTime::parse_from_rfc3339(&e.at).is_ok(), "at must be RFC3339: {}", e.at);
    }

    #[test]
    fn price_rows_put_models_in_use_first_and_flag_unpriced_ones() {
        let events = vec![
            UsageEvent {
                provider: "openai_compatible".into(),
                model: "mystery-model-9000".into(),
                ..UsageEvent::default()
            },
            UsageEvent {
                provider: "anthropic".into(),
                model: "claude-sonnet-5".into(),
                ..UsageEvent::default()
            },
        ];
        let rows = price_rows(&events, &[]);
        assert_eq!(rows[0].model, "mystery-model-9000");
        assert!(rows[0].in_use && !rows[0].priced, "unpriced in-use model comes first");
        assert!(rows[1].in_use && rows[1].priced);
        assert_eq!(rows[1].input_per_mtok, 3.00);
        // The builtin table fills out the rest, without duplicating a used model.
        assert_eq!(
            rows.iter().filter(|r| r.model == "claude-sonnet-5").count(),
            1,
            "a model must not appear twice"
        );
        assert!(rows.iter().any(|r| r.model == "gpt-4o-mini" && !r.in_use));
    }

    #[test]
    fn price_rows_report_an_override_as_overridden() {
        let overrides = vec![ModelPrice {
            model: "claude-sonnet-5".into(),
            input_per_mtok: 1.0,
            output_per_mtok: 2.0,
        }];
        let rows = price_rows(&[], &overrides);
        let row = rows.iter().find(|r| r.model == "claude-sonnet-5").unwrap();
        assert!(row.overridden && row.priced);
        assert_eq!((row.input_per_mtok, row.output_per_mtok), (1.0, 2.0));
        // A model only the user knows about still gets a row to edit.
        let custom = price_rows(
            &[],
            &[ModelPrice {
                model: "my-deployment".into(),
                input_per_mtok: 7.0,
                output_per_mtok: 9.0,
            }],
        );
        assert!(custom.iter().any(|r| r.model == "my-deployment" && r.overridden));
    }
}
