//! Aggregation for the Usage page.
//!
//! Pure: `summarize` takes the event list, a range, the current time, and the
//! user's price overrides, and returns everything the page renders. No
//! `AppHandle`, no clock read — `now` is injected so day bucketing is
//! deterministic under test (the `reminder-format` convention).

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate};
use serde::{Deserialize, Serialize};

use super::pricing::{self, ModelPrice};
use super::{Source, UsageEvent};

/// How far back the page is looking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Range {
    Week,
    Month,
    Quarter,
    All,
}

impl Range {
    /// Number of day buckets to render, or `None` for "everything recorded".
    pub fn days(self) -> Option<i64> {
        match self {
            Self::Week => Some(7),
            Self::Month => Some(30),
            Self::Quarter => Some(90),
            Self::All => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Week => "7d",
            Self::Month => "30d",
            Self::Quarter => "90d",
            Self::All => "all",
        }
    }
}

impl std::str::FromStr for Range {
    type Err = std::convert::Infallible;

    /// Unknown input is the 7-day view, not an error — a stale frontend
    /// asking for a range we removed should still render something.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s.trim() {
            "30d" => Self::Month,
            "90d" => Self::Quarter,
            "all" => Self::All,
            _ => Self::Week,
        })
    }
}

/// One day of the bar chart. Days with no activity are present with zeroes so
/// the chart has no gaps and the x-axis stays evenly spaced.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct DayBucket {
    /// Local calendar date, `YYYY-MM-DD`.
    pub date: String,
    pub runs: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
}

/// "Which prompts do I actually use." Keyed by prompt id; `name` is the most
/// recent snapshot, so a renamed — or deleted — prompt still reads sensibly.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct PromptStat {
    pub prompt_id: String,
    pub name: String,
    pub runs: u32,
    pub failures: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
    /// RFC3339, the most recent run in range.
    pub last_used_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct SourceStat {
    pub source: Source,
    pub runs: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct ModelStat {
    pub provider: String,
    pub model: String,
    pub runs: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
    /// False when no price is known — the UI badges the row and the summary
    /// sets `costIsPartial`.
    pub priced: bool,
}

/// Everything the Usage page renders, computed in one pass.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct UsageSummary {
    /// Echo of the requested range, so the UI can confirm what it got.
    pub range: String,
    pub total_runs: u32,
    pub failure_count: u32,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub estimated_cost_usd: f64,
    /// At least one run used a model with no known price — the total is a
    /// floor, not an estimate. Never let the UI present it as complete.
    pub cost_is_partial: bool,
    pub prices_as_of: String,
    pub daily: Vec<DayBucket>,
    pub top_prompts: Vec<PromptStat>,
    pub by_source: Vec<SourceStat>,
    pub by_model: Vec<ModelStat>,
}

/// Running totals shared by every grouping.
#[derive(Default, Clone, Copy)]
struct Totals {
    runs: u32,
    failures: u32,
    input: u64,
    output: u64,
    cost: f64,
}

impl Totals {
    fn add(&mut self, e: &UsageEvent, cost: f64) {
        self.runs += 1;
        if !e.ok {
            self.failures += 1;
        }
        self.input += u64::from(e.input_tokens);
        self.output += u64::from(e.output_tokens);
        self.cost += cost;
    }
}

/// The window start, as a local calendar date. `None` = no lower bound.
fn window_start(range: Range, now: DateTime<Local>) -> Option<NaiveDate> {
    // `days = 7` means today plus the previous six, so the axis has exactly
    // seven columns and "today" is always the last one.
    range.days().map(|d| (now - Duration::days(d - 1)).date_naive())
}

/// Parse an event's timestamp into local time. A hand-edited or truncated
/// value costs that one row, never the whole page.
fn local_date(at: &str) -> Option<NaiveDate> {
    DateTime::parse_from_rfc3339(at.trim())
        .ok()
        .map(|dt| dt.with_timezone(&Local).date_naive())
}

pub fn summarize(
    events: &[UsageEvent],
    range: Range,
    now: DateTime<Local>,
    overrides: &[ModelPrice],
) -> UsageSummary {
    let today = now.date_naive();
    let start = window_start(range, now);

    // One parse pass: drop unparseable rows, then keep only the window.
    let in_range: Vec<(NaiveDate, &UsageEvent)> = events
        .iter()
        .filter_map(|e| local_date(&e.at).map(|d| (d, e)))
        .filter(|(d, _)| start.is_none_or(|s| *d >= s) && *d <= today)
        .collect();

    let mut overall = Totals::default();
    let mut cost_is_partial = false;
    // Insertion-ordered accumulators — `Vec` keeps sorting explicit and the
    // cardinalities here are tiny (a handful of prompts, models, sources).
    let mut by_day: Vec<(NaiveDate, Totals)> = Vec::new();
    let mut by_prompt: Vec<(PromptStat, Totals)> = Vec::new();
    let mut by_source: Vec<(Source, Totals)> = Vec::new();
    let mut by_model: Vec<(String, String, Totals, bool)> = Vec::new();

    for (date, e) in &in_range {
        let price = pricing::resolve(&e.provider, &e.model, overrides);
        let cost = price.map_or(0.0, |p| pricing::cost_usd(p, e.input_tokens, e.output_tokens));
        // Only AI calls can be unpriced — a zero-token reminder event has no
        // cost to be wrong about, so it must not flag the whole total.
        let has_tokens = e.input_tokens > 0 || e.output_tokens > 0;
        if price.is_none() && has_tokens {
            cost_is_partial = true;
        }

        overall.add(e, cost);

        match by_day.iter_mut().find(|(d, _)| d == date) {
            Some((_, t)) => t.add(e, cost),
            None => {
                let mut t = Totals::default();
                t.add(e, cost);
                by_day.push((*date, t));
            }
        }

        match by_source.iter_mut().find(|(s, _)| *s == e.source) {
            Some((_, t)) => t.add(e, cost),
            None => {
                let mut t = Totals::default();
                t.add(e, cost);
                by_source.push((e.source, t));
            }
        }

        if !e.model.is_empty() {
            match by_model
                .iter_mut()
                .find(|(p, m, _, _)| *p == e.provider && *m == e.model)
            {
                Some((_, _, t, priced)) => {
                    t.add(e, cost);
                    *priced = *priced && price.is_some();
                }
                None => {
                    let mut t = Totals::default();
                    t.add(e, cost);
                    by_model.push((e.provider.clone(), e.model.clone(), t, price.is_some()));
                }
            }
        }

        if e.source == Source::Prompt {
            match by_prompt.iter_mut().find(|(s, _)| s.prompt_id == e.prompt_id) {
                Some((stat, t)) => {
                    t.add(e, cost);
                    // A rename must not split the row: keep the newest name
                    // and the newest timestamp.
                    if e.at > stat.last_used_at {
                        stat.last_used_at = e.at.clone();
                        stat.name = e.prompt_name.clone();
                    }
                }
                None => {
                    let mut t = Totals::default();
                    t.add(e, cost);
                    by_prompt.push((
                        PromptStat {
                            prompt_id: e.prompt_id.clone(),
                            name: e.prompt_name.clone(),
                            last_used_at: e.at.clone(),
                            ..Default::default()
                        },
                        t,
                    ));
                }
            }
        }
    }

    UsageSummary {
        range: range.as_str().to_string(),
        total_runs: overall.runs,
        failure_count: overall.failures,
        total_input_tokens: overall.input,
        total_output_tokens: overall.output,
        estimated_cost_usd: overall.cost,
        cost_is_partial,
        prices_as_of: pricing::PRICES_AS_OF.to_string(),
        daily: fill_days(&by_day, start, today),
        top_prompts: rank_prompts(by_prompt),
        by_source: rank_sources(by_source),
        by_model: rank_models(by_model),
    }
}

/// Zero-fill every day in the window so the chart has no gaps. For `all`, span
/// the recorded days only — zero-filling back to the first ever run would
/// otherwise stretch the axis over an arbitrary amount of empty history.
fn fill_days(
    by_day: &[(NaiveDate, Totals)],
    start: Option<NaiveDate>,
    today: NaiveDate,
) -> Vec<DayBucket> {
    let first = match start.or_else(|| by_day.iter().map(|(d, _)| *d).min()) {
        Some(d) => d,
        None => return Vec::new(),
    };
    let mut out = Vec::new();
    let mut day = first;
    while day <= today {
        let t = by_day
            .iter()
            .find(|(d, _)| *d == day)
            .map(|(_, t)| *t)
            .unwrap_or_default();
        out.push(DayBucket {
            date: format!("{:04}-{:02}-{:02}", day.year(), day.month(), day.day()),
            runs: t.runs,
            input_tokens: t.input,
            output_tokens: t.output,
            cost_usd: t.cost,
        });
        day += Duration::days(1);
    }
    out
}

/// Most-used first. Ties break on tokens then name so the order is stable
/// across reloads rather than following insertion order.
fn rank_prompts(mut rows: Vec<(PromptStat, Totals)>) -> Vec<PromptStat> {
    rows.sort_by(|a, b| {
        b.1.runs
            .cmp(&a.1.runs)
            .then((b.1.input + b.1.output).cmp(&(a.1.input + a.1.output)))
            .then(a.0.name.cmp(&b.0.name))
    });
    rows.into_iter()
        .map(|(stat, t)| PromptStat {
            runs: t.runs,
            failures: t.failures,
            input_tokens: t.input,
            output_tokens: t.output,
            cost_usd: t.cost,
            ..stat
        })
        .collect()
}

fn rank_sources(mut rows: Vec<(Source, Totals)>) -> Vec<SourceStat> {
    rows.sort_by(|a, b| b.1.runs.cmp(&a.1.runs).then(a.0.as_str().cmp(b.0.as_str())));
    rows.into_iter()
        .map(|(source, t)| SourceStat {
            source,
            runs: t.runs,
            input_tokens: t.input,
            output_tokens: t.output,
            cost_usd: t.cost,
        })
        .collect()
}

/// Costliest first — the point of the table is "where is the money going",
/// and an unpriced model sorts by tokens among the other zero-cost rows.
fn rank_models(mut rows: Vec<(String, String, Totals, bool)>) -> Vec<ModelStat> {
    rows.sort_by(|a, b| {
        b.2.cost
            .partial_cmp(&a.2.cost)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then((b.2.input + b.2.output).cmp(&(a.2.input + a.2.output)))
            .then(a.1.cmp(&b.1))
    });
    rows.into_iter()
        .map(|(provider, model, t, priced)| ModelStat {
            provider,
            model,
            runs: t.runs,
            input_tokens: t.input,
            output_tokens: t.output,
            cost_usd: t.cost,
            priced,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usage::ErrorKind;
    use chrono::TimeZone;

    /// Same reference point the reminder tests use, so the two suites tell a
    /// consistent story about "today".
    fn now() -> DateTime<Local> {
        Local.with_ymd_and_hms(2026, 7, 24, 14, 30, 0).unwrap()
    }

    /// An event at a local date/hour, expressed in the machine's own offset so
    /// the test is timezone-independent.
    fn at(y: i32, m: u32, d: u32, h: u32) -> String {
        Local.with_ymd_and_hms(y, m, d, h, 0, 0).unwrap().to_rfc3339()
    }

    fn prompt_run(id: &str, name: &str, when: String, input: u32, output: u32) -> UsageEvent {
        UsageEvent {
            at: when,
            source: Source::Prompt,
            prompt_id: id.into(),
            prompt_name: name.into(),
            provider: "anthropic".into(),
            model: "claude-sonnet-5".into(),
            input_tokens: input,
            output_tokens: output,
            ok: true,
            ..UsageEvent::default()
        }
    }

    #[test]
    fn totals_add_up_across_every_grouping() {
        let events = vec![
            prompt_run("a", "Fix grammar", at(2026, 7, 24, 9), 100, 20),
            prompt_run("a", "Fix grammar", at(2026, 7, 24, 10), 200, 40),
            prompt_run("b", "Summarize", at(2026, 7, 23, 11), 300, 60),
        ];
        let s = summarize(&events, Range::Week, now(), &[]);
        assert_eq!(s.total_runs, 3);
        assert_eq!(s.total_input_tokens, 600);
        assert_eq!(s.total_output_tokens, 120);
        assert_eq!(s.failure_count, 0);
        assert!(!s.cost_is_partial);
        // $3/Mtok in + $15/Mtok out
        let expected = (600.0 * 3.0 + 120.0 * 15.0) / 1_000_000.0;
        assert!((s.estimated_cost_usd - expected).abs() < 1e-12);
        // The same totals must appear in the per-day and per-model views.
        let day_runs: u32 = s.daily.iter().map(|d| d.runs).sum();
        assert_eq!(day_runs, 3);
        assert_eq!(s.by_model.len(), 1);
        assert_eq!(s.by_model[0].runs, 3);
    }

    #[test]
    fn seven_day_range_is_zero_filled_and_ends_today() {
        let events = vec![prompt_run("a", "A", at(2026, 7, 24, 9), 10, 5)];
        let s = summarize(&events, Range::Week, now(), &[]);
        assert_eq!(s.daily.len(), 7, "today plus the previous six");
        assert_eq!(s.daily.first().unwrap().date, "2026-07-18");
        assert_eq!(s.daily.last().unwrap().date, "2026-07-24");
        // Only the last day has activity; the rest are present with zeroes.
        assert_eq!(s.daily.last().unwrap().runs, 1);
        assert!(s.daily[..6].iter().all(|d| d.runs == 0 && d.cost_usd == 0.0));
    }

    #[test]
    fn events_are_bucketed_by_local_day_across_midnight() {
        // 23:00 and 00:00 are an hour apart but different calendar days.
        let events = vec![
            prompt_run("a", "A", at(2026, 7, 23, 23), 10, 1),
            prompt_run("a", "A", at(2026, 7, 24, 0), 10, 1),
        ];
        let s = summarize(&events, Range::Week, now(), &[]);
        let d23 = s.daily.iter().find(|d| d.date == "2026-07-23").unwrap();
        let d24 = s.daily.iter().find(|d| d.date == "2026-07-24").unwrap();
        assert_eq!((d23.runs, d24.runs), (1, 1));
    }

    #[test]
    fn events_outside_the_range_are_excluded_entirely() {
        let events = vec![
            prompt_run("a", "A", at(2026, 7, 24, 9), 10, 1),
            prompt_run("a", "A", at(2026, 5, 1, 9), 999, 999), // long past
        ];
        let week = summarize(&events, Range::Week, now(), &[]);
        assert_eq!(week.total_runs, 1);
        assert_eq!(week.total_input_tokens, 10);

        let all = summarize(&events, Range::All, now(), &[]);
        assert_eq!(all.total_runs, 2, "`all` must include the old run");
        assert_eq!(
            all.daily.first().unwrap().date,
            "2026-05-01",
            "`all` spans the recorded days only"
        );
    }

    #[test]
    fn a_corrupt_timestamp_costs_one_row_not_the_whole_page() {
        let bad = prompt_run("a", "Broken", "not-a-date".into(), 500, 500);
        let events = vec![prompt_run("a", "A", at(2026, 7, 24, 9), 10, 1), bad];
        let s = summarize(&events, Range::Week, now(), &[]);
        assert_eq!(s.total_runs, 1);
        assert_eq!(s.total_input_tokens, 10);
    }

    #[test]
    fn top_prompts_rank_by_runs_and_keep_the_newest_name() {
        let events = vec![
            prompt_run("a", "Old name", at(2026, 7, 22, 9), 10, 1),
            prompt_run("a", "New name", at(2026, 7, 24, 9), 10, 1),
            prompt_run("b", "Once", at(2026, 7, 23, 9), 10, 1),
        ];
        let s = summarize(&events, Range::Week, now(), &[]);
        assert_eq!(s.top_prompts.len(), 2);
        assert_eq!(s.top_prompts[0].prompt_id, "a");
        assert_eq!(s.top_prompts[0].runs, 2);
        assert_eq!(s.top_prompts[0].name, "New name", "a rename must not split the row");
        assert_eq!(s.top_prompts[0].last_used_at, at(2026, 7, 24, 9));
        assert_eq!(s.top_prompts[1].prompt_id, "b");
    }

    #[test]
    fn equal_run_counts_break_ties_by_tokens_then_name() {
        // Deterministic order matters: the table must not reshuffle on reload.
        let events = vec![
            prompt_run("b", "Beta", at(2026, 7, 24, 9), 10, 0),
            prompt_run("a", "Alpha", at(2026, 7, 24, 9), 10, 0),
            prompt_run("c", "Gamma", at(2026, 7, 24, 9), 99, 0),
        ];
        let names: Vec<String> = summarize(&events, Range::Week, now(), &[])
            .top_prompts
            .into_iter()
            .map(|p| p.name)
            .collect();
        assert_eq!(names, vec!["Gamma", "Alpha", "Beta"]);
    }

    #[test]
    fn failures_are_counted_and_their_tokens_still_billed() {
        // A provider can charge for a request whose response we failed to
        // parse — dropping those tokens would under-report real spend.
        let mut failed = prompt_run("a", "A", at(2026, 7, 24, 9), 50, 0);
        failed.ok = false;
        failed.error_kind = ErrorKind::Auth;
        let events = vec![prompt_run("a", "A", at(2026, 7, 24, 10), 10, 5), failed];
        let s = summarize(&events, Range::Week, now(), &[]);
        assert_eq!(s.total_runs, 2);
        assert_eq!(s.failure_count, 1);
        assert_eq!(s.total_input_tokens, 60);
        assert_eq!(s.top_prompts[0].failures, 1);
    }

    #[test]
    fn an_unpriced_model_counts_tokens_and_flags_the_total_as_partial() {
        let mut mystery = prompt_run("a", "A", at(2026, 7, 24, 9), 1000, 500);
        mystery.provider = "openai_compatible".into();
        mystery.model = "mystery-model-9000".into();
        let s = summarize(&[mystery], Range::Week, now(), &[]);
        assert_eq!(s.total_input_tokens, 1000, "tokens are still counted");
        assert_eq!(s.estimated_cost_usd, 0.0, "no price means no guess");
        assert!(s.cost_is_partial, "the UI must be told the total is a floor");
        assert!(!s.by_model[0].priced);
    }

    #[test]
    fn zero_token_events_never_flag_the_total_as_partial() {
        // Reminder events carry no model and no tokens; there is no cost to
        // be uncertain about, so they must not taint the estimate.
        let e = UsageEvent {
            at: at(2026, 7, 24, 9),
            source: Source::ReminderCreated,
            ok: true,
            ..UsageEvent::default()
        };
        let s = summarize(&[e], Range::Week, now(), &[]);
        assert_eq!(s.total_runs, 1);
        assert!(!s.cost_is_partial);
        assert!(s.by_model.is_empty(), "an event with no model is not a model row");
        assert_eq!(s.by_source[0].source, Source::ReminderCreated);
    }

    #[test]
    fn ollama_runs_are_free_but_still_counted() {
        let mut local = prompt_run("a", "A", at(2026, 7, 24, 9), 5000, 2000);
        local.provider = "ollama".into();
        local.model = "llama3.2".into();
        let s = summarize(&[local], Range::Week, now(), &[]);
        assert_eq!(s.total_output_tokens, 2000);
        assert_eq!(s.estimated_cost_usd, 0.0);
        assert!(!s.cost_is_partial);
        assert!(s.by_model[0].priced);
    }

    #[test]
    fn a_price_override_changes_the_reported_cost() {
        let events = vec![prompt_run("a", "A", at(2026, 7, 24, 9), 1_000_000, 0)];
        let plain = summarize(&events, Range::Week, now(), &[]);
        assert!((plain.estimated_cost_usd - 3.0).abs() < 1e-9);

        let overrides = vec![ModelPrice {
            model: "claude-sonnet-5".into(),
            input_per_mtok: 1.0,
            output_per_mtok: 1.0,
        }];
        let tuned = summarize(&events, Range::Week, now(), &overrides);
        assert!((tuned.estimated_cost_usd - 1.0).abs() < 1e-9);
    }

    #[test]
    fn models_are_grouped_per_provider_and_ranked_by_cost() {
        let mut cheap = prompt_run("a", "A", at(2026, 7, 24, 9), 1000, 0);
        cheap.provider = "openai_compatible".into();
        cheap.model = "gpt-4o-mini".into();
        let pricey = prompt_run("a", "A", at(2026, 7, 24, 9), 1000, 1000);
        let s = summarize(&[cheap, pricey], Range::Week, now(), &[]);
        assert_eq!(s.by_model.len(), 2);
        assert_eq!(s.by_model[0].model, "claude-sonnet-5", "costliest first");
        assert!(s.by_model[0].cost_usd > s.by_model[1].cost_usd);
    }

    #[test]
    fn an_empty_log_produces_a_zeroed_but_renderable_summary() {
        let s = summarize(&[], Range::Week, now(), &[]);
        assert_eq!(s.total_runs, 0);
        assert_eq!(s.daily.len(), 7, "the chart still needs its axis");
        assert!(s.top_prompts.is_empty() && s.by_model.is_empty() && s.by_source.is_empty());
        assert!(!s.cost_is_partial);
        assert_eq!(s.prices_as_of, pricing::PRICES_AS_OF);
        assert_eq!(s.range, "7d");
        // `all` with no events has nothing to span, so no axis at all.
        assert!(summarize(&[], Range::All, now(), &[]).daily.is_empty());
    }

    #[test]
    fn ranges_parse_leniently_and_round_trip_their_labels() {
        use std::str::FromStr;
        for (input, expected) in [
            ("7d", Range::Week),
            ("30d", Range::Month),
            ("90d", Range::Quarter),
            ("all", Range::All),
            (" 30d ", Range::Month),
        ] {
            assert_eq!(Range::from_str(input).unwrap(), expected, "for {input:?}");
        }
        // A range the frontend no longer knows about must not fail the call.
        assert_eq!(Range::from_str("decade").unwrap(), Range::Week);
        assert_eq!(Range::Quarter.as_str(), "90d");
        assert_eq!(Range::Quarter.days(), Some(90));
        assert_eq!(Range::All.days(), None);
    }
}
