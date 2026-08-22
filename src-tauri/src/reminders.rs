use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, TimeZone, Weekday};
use serde::{Deserialize, Deserializer, Serialize};
use tauri::{AppHandle, Emitter};
use tauri_plugin_store::StoreExt;

use crate::{ai, config, notify};

pub const STORE_FILE: &str = "reminders.json";
const KEY: &str = "reminders";

/// Emitted after every mutation and every fire, so an open Reminders page
/// refreshes without polling (same fan-out idea as `assistant-state`).
pub const CHANGED_EVENT: &str = "reminders-changed";

/// Distinct from the app-wide "Claudy" title so a reminder toast is
/// recognizable at a glance among prompt/dictation notifications.
const NOTIFICATION_TITLE: &str = "Claudy — Reminder";

/// Scheduler poll interval. Reminder granularity is minutes, so this is
/// precise enough and avoids re-arming a timer on every mutation.
const TICK_SECS: u64 = 15;

/// Below this, a late fire isn't worth calling "overdue" — a reminder due at
/// 16:32 firing at 16:32:14 (poll granularity) is simply on time.
const OVERDUE_GRACE_MINS: i64 = 2;

/// A day named without a clock time ("tomorrow", "next friday") means this.
const DEFAULT_HOUR: u32 = 9;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Scheduled, not yet fired.
    #[default]
    Pending,
    /// The notification went out; still waiting on the user.
    Fired,
    /// Dismissed by the user.
    Done,
}

// Hand-written like `DictationMode`: reminders.json is hand-editable, and one
// bad status string must not fail the load of the whole queue.
impl<'de> Deserialize<'de> for Status {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(match String::deserialize(d).unwrap_or_default().as_str() {
            "fired" => Self::Fired,
            "done" => Self::Done,
            _ => Self::Pending,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Reminder {
    pub id: String,
    /// Exactly what the user typed — never rewritten.
    pub text: String,
    /// `text` with the time expression stripped; the notification body.
    pub title: String,
    /// RFC3339 with offset, e.g. "2026-07-24T16:32:00+07:00". Stored as a
    /// string so the store JSON stays readable and the frontend can parse it
    /// directly; converted at the boundary via `parse_due`.
    pub due_at: String,
    pub created_at: String,
    /// RFC3339; "" until it fires.
    pub fired_at: String,
    pub status: Status,
}

impl Default for Reminder {
    fn default() -> Self {
        Self {
            id: String::new(),
            text: String::new(),
            title: String::new(),
            due_at: String::new(),
            created_at: String::new(),
            fired_at: String::new(),
            status: Status::Pending,
        }
    }
}

// --- persistence (same thin-wrapper shape as prompts.rs) ---

pub fn load(app: &AppHandle) -> Result<Vec<Reminder>, String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    match store.get(KEY) {
        // An empty queue is the correct first-run state — nothing to seed.
        Some(v) => serde_json::from_value(v).map_err(|e| e.to_string()),
        None => Ok(Vec::new()),
    }
}

pub fn save_list(app: &AppHandle, list: &[Reminder]) -> Result<(), String> {
    let store = app.store(STORE_FILE).map_err(|e| e.to_string())?;
    let value = serde_json::to_value(list).map_err(|e| e.to_string())?;
    store.set(KEY, value);
    store.save().map_err(|e| e.to_string())
}

/// Pure list ops — persistence stays a thin wrapper around them.
pub fn upsert(mut list: Vec<Reminder>, reminder: Reminder) -> Vec<Reminder> {
    match list.iter().position(|r| r.id == reminder.id) {
        Some(i) => list[i] = reminder,
        None => list.push(reminder),
    }
    list
}

pub fn remove(list: Vec<Reminder>, id: &str) -> Vec<Reminder> {
    list.into_iter().filter(|r| r.id != id).collect()
}

/// RFC3339 -> local time. `None` for a hand-edited/corrupt value.
pub fn parse_due(due_at: &str) -> Option<DateTime<Local>> {
    DateTime::parse_from_rfc3339(due_at)
        .ok()
        .map(|d| d.with_timezone(&Local))
}

/// Ids of pending reminders that have come due. A reminder whose `dueAt` is
/// unreadable counts as due: firing it once (status then flips to Fired) beats
/// leaving it invisible in the queue forever.
pub fn due_now(list: &[Reminder], now: DateTime<Local>) -> Vec<String> {
    list.iter()
        .filter(|r| r.status == Status::Pending)
        .filter(|r| parse_due(&r.due_at).map(|d| d <= now).unwrap_or(true))
        .map(|r| r.id.clone())
        .collect()
}

/// Pure: flip the given ids to Fired with a timestamp.
pub fn mark_fired(list: Vec<Reminder>, ids: &[String], stamp: &str) -> Vec<Reminder> {
    list.into_iter()
        .map(|mut r| {
            if ids.contains(&r.id) {
                r.status = Status::Fired;
                r.fired_at = stamp.to_string();
            }
            r
        })
        .collect()
}

/// "Today 16:32" / "Tomorrow 09:00" / "Wed 29 Jul 14:30". Rust-side labelling
/// for notification text only; the Reminders page formats its own rows.
pub fn format_due_label(due: DateTime<Local>, now: DateTime<Local>) -> String {
    let days = (due.date_naive() - now.date_naive()).num_days();
    let clock = due.format("%H:%M");
    match days {
        0 => format!("Today {clock}"),
        1 => format!("Tomorrow {clock}"),
        -1 => format!("Yesterday {clock}"),
        _ => format!("{} {clock}", due.format("%a %-d %b")),
    }
}

/// The notification body: the title, plus an overdue note when the fire is
/// meaningfully late (app was closed when it came due).
pub fn notification_body(r: &Reminder, now: DateTime<Local>) -> String {
    let title = if r.title.trim().is_empty() {
        r.text.trim()
    } else {
        r.title.trim()
    };
    match parse_due(&r.due_at) {
        Some(due) if now - due > Duration::minutes(OVERDUE_GRACE_MINS) => {
            format!("{title}\n(overdue — was due {})", format_due_label(due, now))
        }
        _ => title.to_string(),
    }
}

// --- local time parser ---

#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    pub due_at: DateTime<Local>,
    pub title: String,
}

/// One whitespace-separated word, lowercased and stripped of trailing
/// punctuation for matching, with byte offsets back into the original text so
/// the matched span can be cut out to form the title.
struct Token {
    lower: String,
    start: usize,
    end: usize,
}

fn tokenize(text: &str) -> Vec<Token> {
    text.split_whitespace()
        .map(|word| {
            // split_whitespace slices borrow from `text`, so pointer arithmetic
            // gives the byte offset without tracking it manually.
            let start = word.as_ptr() as usize - text.as_ptr() as usize;
            let trimmed = word.trim_end_matches([',', '.', '!', '?', ';', ':']);
            Token {
                lower: trimmed.to_lowercase(),
                start,
                end: start + word.len(),
            }
        })
        .collect()
}

fn word_number(s: &str) -> Option<i64> {
    match s {
        "a" | "an" | "one" => Some(1),
        "two" | "couple" => Some(2),
        "three" => Some(3),
        "four" => Some(4),
        "five" => Some(5),
        "six" => Some(6),
        "seven" => Some(7),
        "eight" => Some(8),
        "nine" => Some(9),
        "ten" => Some(10),
        "fifteen" => Some(15),
        "twenty" => Some(20),
        "thirty" => Some(30),
        "sixty" => Some(60),
        _ => s.parse::<i64>().ok(),
    }
}

fn unit_seconds(s: &str) -> Option<i64> {
    match s {
        "s" | "sec" | "secs" | "second" | "seconds" => Some(1),
        "m" | "min" | "mins" | "minute" | "minutes" => Some(60),
        "h" | "hr" | "hrs" | "hour" | "hours" => Some(3600),
        "d" | "day" | "days" => Some(86_400),
        "w" | "wk" | "wks" | "week" | "weeks" => Some(604_800),
        _ => None,
    }
}

/// "2h" -> (2, "h"), "30min" -> (30, "min"). `None` when there is no
/// digits-then-letters shape.
fn split_compact(s: &str) -> Option<(i64, &str)> {
    let idx = s.find(|c: char| !c.is_ascii_digit())?;
    if idx == 0 {
        return None;
    }
    Some((s[..idx].parse().ok()?, &s[idx..]))
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Clock {
    hour: u32,
    minute: u32,
    /// Did the user say am/pm (or noon/midnight)? Drives the bare-hour
    /// disambiguation for day-less times.
    explicit: bool,
    /// Was it written as H:MM? Someone who types "11:00" has named the hour
    /// exactly and does not mean 23:00 — only a bare "11" is ambiguous.
    precise: bool,
}

fn parse_clock(s: &str) -> Option<Clock> {
    match s {
        "noon" | "midday" => {
            return Some(Clock { hour: 12, minute: 0, explicit: true, precise: true })
        }
        "midnight" => return Some(Clock { hour: 0, minute: 0, explicit: true, precise: true }),
        _ => {}
    }
    let (body, pm) = match s.strip_suffix("pm").or_else(|| s.strip_suffix("p.m")) {
        Some(b) => (b, Some(true)),
        None => match s.strip_suffix("am").or_else(|| s.strip_suffix("a.m")) {
            Some(b) => (b, Some(false)),
            None => (s, None),
        },
    };
    let body = body.trim_end_matches('.');
    if body.is_empty() {
        return None;
    }
    let (h, m, precise) = match body.split_once(':') {
        Some((h, m)) => (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?, true),
        None => (body.parse::<u32>().ok()?, 0, false),
    };
    if m > 59 {
        return None;
    }
    let hour = match pm {
        Some(true) if h == 12 => 12,
        Some(true) if h < 12 => h + 12,
        Some(false) if h == 12 => 0,
        Some(false) if h < 12 => h,
        Some(_) => return None, // "13pm"
        None if h < 24 => h,
        None => return None,
    };
    Some(Clock { hour, minute: m, explicit: pm.is_some(), precise })
}

/// A clock at `i`, optionally spelled as two tokens ("5 pm"). Returns the
/// clock and the index of its LAST token.
fn match_clock(tokens: &[Token], i: usize) -> Option<(Clock, usize)> {
    let here = tokens.get(i)?;
    if let Some(next) = tokens.get(i + 1) {
        if next.lower == "am" || next.lower == "pm" {
            if let Some(c) = parse_clock(&format!("{}{}", here.lower, next.lower)) {
                return Some((c, i + 1));
            }
        }
    }
    parse_clock(&here.lower).map(|c| (c, i))
}

fn weekday(s: &str) -> Option<Weekday> {
    Some(match s {
        "monday" | "mon" => Weekday::Mon,
        "tuesday" | "tue" | "tues" => Weekday::Tue,
        "wednesday" | "wed" | "weds" => Weekday::Wed,
        "thursday" | "thu" | "thur" | "thurs" => Weekday::Thu,
        "friday" | "fri" => Weekday::Fri,
        "saturday" | "sat" => Weekday::Sat,
        "sunday" | "sun" => Weekday::Sun,
        _ => return None,
    })
}

/// A named part of the day, when no clock time is given.
fn day_part_hour(s: &str) -> Option<u32> {
    Some(match s {
        "morning" => 9,
        "noon" | "midday" => 12,
        "afternoon" => 14,
        "evening" => 18,
        "night" => 20,
        _ => return None,
    })
}

/// Local date+time -> instant. DST-safe: a gap (spring forward) has no exact
/// local instant, so fall back to the earliest valid one, then to the next
/// hour rather than dropping the match.
fn at_local(date: NaiveDate, hour: u32, minute: u32) -> Option<DateTime<Local>> {
    let naive = date.and_hms_opt(hour, minute, 0)?;
    let mapped = Local.from_local_datetime(&naive);
    mapped.single().or_else(|| mapped.earliest()).or_else(|| {
        let bumped = date.and_hms_opt((hour + 1).min(23), minute, 0)?;
        Local.from_local_datetime(&bumped).earliest()
    })
}

/// "in 2 hours", "in 30min", "in a week". Returns the instant and the index of
/// the expression's LAST token.
fn match_relative(
    tokens: &[Token],
    i: usize,
    now: DateTime<Local>,
) -> Option<(DateTime<Local>, usize)> {
    if tokens.get(i)?.lower != "in" {
        return None;
    }
    let next = tokens.get(i + 1)?;
    // Compact: "in 2h"
    if let Some((n, unit)) = split_compact(&next.lower) {
        if let Some(secs) = unit_seconds(unit) {
            if n >= 1 {
                return Some((shift_seconds(now, n, secs)?, i + 1));
            }
        }
    }
    // Spelled out: "in 2 hours"
    let n = word_number(&next.lower)?;
    let secs = unit_seconds(&tokens.get(i + 2)?.lower)?;
    if n < 1 {
        return None;
    }
    Some((shift_seconds(now, n, secs)?, i + 2))
}

/// `now` shifted forward by `n * secs` seconds, or `None` when that lands
/// outside the representable range.
///
/// Every factor here comes from user text: `word_number` falls through to
/// `parse::<i64>()`, so "in 999999999 days" arrives intact. `n * secs`,
/// `Duration::seconds` and `DateTime + TimeDelta` each panic on overflow, and
/// `preview_reminder` is a *sync* command run on every keystroke — an unwind
/// there goes straight into the IPC handler. A nonsense offset must read as
/// "no time found", which is what the rest of the parser already does with
/// input it can't use.
fn shift_seconds(now: DateTime<Local>, n: i64, secs: i64) -> Option<DateTime<Local>> {
    let total = n.checked_mul(secs)?;
    now.checked_add_signed(Duration::try_seconds(total)?)
}

/// The date named at `i`, that anchor's default hour, and the index of its
/// LAST token. "on"/"next"/"this" are consumed as an optional prefix.
fn match_day(tokens: &[Token], i: usize, now: DateTime<Local>) -> Option<(NaiveDate, u32, usize)> {
    let mut j = i;
    if matches!(tokens.get(i)?.lower.as_str(), "on" | "next" | "this") {
        j = i + 1;
    }
    let word = &tokens.get(j)?.lower;
    let today = now.date_naive();
    match word.as_str() {
        "today" => Some((today, DEFAULT_HOUR, j)),
        "tomorrow" | "tmr" | "tmrw" => Some((today.succ_opt()?, DEFAULT_HOUR, j)),
        // "tonight" is today, but nobody means 9 a.m. by it.
        "tonight" => Some((today, 20, j)),
        _ => {
            let target = weekday(word)?;
            // A bare weekday always means the NEXT one, never today.
            let ahead = match (target.num_days_from_monday() as i64
                - today.weekday().num_days_from_monday() as i64
                + 7)
                % 7
            {
                0 => 7,
                n => n,
            };
            Some((today + Duration::days(ahead), DEFAULT_HOUR, j))
        }
    }
}

/// Read a bare 1-11 hour in the half of the day its anchor names.
///
/// "tonight" carries an evening meaning that the clock following it must not
/// throw away: "tonight at 8" is 20:00, not 08:00. Taken literally it was
/// usually already past, and the day branch's own past-time guard then dropped
/// the entire parse to the AI fallback — so the failure was silent and offline
/// it failed outright. Only an ambiguous hour is moved: "tonight at 8am" says
/// what it means, and "at 10:30" already names the hour exactly.
fn hour_for_anchor(clock: Clock, default_hour: u32) -> u32 {
    let ambiguous = !clock.explicit && !clock.precise && (1..=11).contains(&clock.hour);
    if ambiguous && default_hour >= 12 {
        clock.hour + 12
    } else {
        clock.hour
    }
}

/// A time part following a day anchor: "at 9am", "at 17:00", a bare
/// meridiem/colon clock ("tomorrow 9am"), or "morning"/"evening"/…
/// `default_hour` is the anchor's own time of day — see `hour_for_anchor`.
fn match_day_time(tokens: &[Token], after: usize, default_hour: u32) -> Option<(u32, u32, usize)> {
    let tok = tokens.get(after)?;
    if tok.lower == "at" || tok.lower == "around" {
        if let Some((c, end)) = match_clock(tokens, after + 1) {
            return Some((hour_for_anchor(c, default_hour), c.minute, end));
        }
    }
    if let Some(hour) = day_part_hour(&tok.lower) {
        return Some((hour, 0, after));
    }
    // "tomorrow 9am" / "monday 17:00" — only when unambiguous, so a stray
    // number ("tomorrow 3 things") can't be read as a time.
    let unambiguous = tok.lower.contains(':')
        || tok.lower.ends_with("am")
        || tok.lower.ends_with("pm")
        || tokens
            .get(after + 1)
            .is_some_and(|n| n.lower == "am" || n.lower == "pm");
    if unambiguous {
        if let Some((c, end)) = match_clock(tokens, after) {
            return Some((hour_for_anchor(c, default_hour), c.minute, end));
        }
    }
    None
}

/// A clock stated *before* the day it belongs to: "dinner at 7pm tomorrow".
/// Returns the index the time expression starts at, plus the resolved hour and
/// minute.
///
/// Only a clock running right up to the day word counts, so an unrelated
/// earlier time ("book the 9am flight, dinner tomorrow") can't be stolen.
fn match_clock_before(
    tokens: &[Token],
    day_start: usize,
    default_hour: u32,
) -> Option<(usize, u32, u32)> {
    for j in (0..day_start).rev() {
        if !matches!(tokens[j].lower.as_str(), "at" | "around") {
            continue;
        }
        if let Some((c, end)) = match_clock(tokens, j + 1) {
            if end + 1 == day_start {
                return Some((j, hour_for_anchor(c, default_hour), c.minute));
            }
        }
    }
    None
}

/// "at 5pm" with no day named. Picks the soonest future instant: a bare hour
/// of 1–11 with neither am/pm nor H:MM notation also considers its afternoon
/// reading, so "at 9" typed at 14:30 means 21:00 tonight, not 09:00 tomorrow.
/// "at 11:00" is left alone — that spelling already names the hour exactly.
fn match_bare_clock(
    tokens: &[Token],
    i: usize,
    now: DateTime<Local>,
) -> Option<(DateTime<Local>, usize)> {
    if !matches!(tokens.get(i)?.lower.as_str(), "at" | "around") {
        return None;
    }
    let (clock, end) = match_clock(tokens, i + 1)?;
    let today = now.date_naive();
    let tomorrow = today.succ_opt()?;

    let mut candidates = vec![at_local(today, clock.hour, clock.minute)];
    if !clock.explicit && !clock.precise && (1..=11).contains(&clock.hour) {
        candidates.push(at_local(today, clock.hour + 12, clock.minute));
    }
    candidates.push(at_local(tomorrow, clock.hour, clock.minute));

    candidates
        .into_iter()
        .flatten()
        .find(|d| *d > now)
        .map(|d| (d, end))
}

/// Leading filler the user may keep typing even though Remind Mode already
/// says "remind". Stripped so the toast reads as the task itself.
const LEAD_INS: [&str; 8] = [
    "remind me to ",
    "remind me ",
    "reminder to ",
    "reminder ",
    "remember to ",
    "remember ",
    "please ",
    "to ",
];

fn strip_lead_ins(text: &str) -> String {
    let mut s = text.trim();
    loop {
        let lower = s.to_lowercase();
        match LEAD_INS.iter().find(|p| lower.starts_with(**p)) {
            Some(p) => s = s[p.len()..].trim_start(),
            None => break,
        }
    }
    s.to_string()
}

/// The title: the original text minus the matched time span, whitespace
/// collapsed, lead-ins and dangling connectors removed. Falls back to the full
/// text when stripping would leave nothing.
fn clean_title(text: &str, start: usize, end: usize) -> String {
    let joined = format!("{} {}", &text[..start], &text[end..]);
    let collapsed = joined.split_whitespace().collect::<Vec<_>>().join(" ");
    let stripped = strip_lead_ins(&collapsed);
    let trimmed = stripped.trim_end_matches([',', '.', '!', '?', ';', ':', ' ']).trim();
    // Spans like "... by tomorrow" leave the connector behind.
    let trimmed = match trimmed.rsplit_once(' ') {
        Some((head, last))
            if matches!(last.to_lowercase().as_str(), "at" | "in" | "on" | "by" | "around") =>
        {
            head.trim()
        }
        _ => trimmed,
    };
    if trimmed.is_empty() {
        strip_lead_ins(text)
    } else {
        trimmed.to_string()
    }
}

/// Find a time expression anywhere in `text` and resolve it against `now`.
/// `None` = no expression this parser knows; the caller decides whether to
/// fall back to the AI extractor.
pub fn parse_when(text: &str, now: DateTime<Local>) -> Option<Parsed> {
    let tokens = tokenize(text);
    let found = |i: usize, due_at: DateTime<Local>, end: usize| Parsed {
        due_at,
        title: clean_title(text, tokens[i].start, tokens[end].end),
    };

    // Most specific first: "in 2 hours" before a day, a day before a day-less
    // "at 5pm". Each matcher sweeps the whole string before the next one runs.
    // Trying all three at each index in turn instead would rank by *position*
    // rather than specificity, so a clock early in the sentence beat a day word
    // later in it — "dinner at 7pm tomorrow" landed today, with "tomorrow" left
    // behind in the title as evidence the day was never read.
    for i in 0..tokens.len() {
        if let Some((due_at, end)) = match_relative(&tokens, i, now) {
            return Some(found(i, due_at, end));
        }
    }
    for i in 0..tokens.len() {
        if let Some((date, default_hour, day_end)) = match_day(&tokens, i, now) {
            // A time part after the anchor ("tomorrow at 7pm"), or failing
            // that one stated just before it ("dinner at 7pm tomorrow") —
            // the same sentence to a person, and the more natural phrasing.
            // `start` moves back to the clock in that case so the title loses
            // the whole expression instead of keeping a dangling "at 7pm".
            let (start, hour, minute, end) =
                match match_day_time(&tokens, day_end + 1, default_hour) {
                    Some((h, m, e)) => (i, h, m, e),
                    None => match match_clock_before(&tokens, i, default_hour) {
                        Some((clock_at, h, m)) => (clock_at, h, m, day_end),
                        None => (i, default_hour, 0, day_end),
                    },
                };
            let due_at = at_local(date, hour, minute)?;
            // The user named a day outright. If it lands in the past ("today
            // at 9am" typed at 14:30) say we don't know rather than let the
            // day-less matcher quietly reschedule it for tomorrow.
            if due_at <= now {
                return None;
            }
            return Some(found(start, due_at, end));
        }
    }
    for i in 0..tokens.len() {
        if let Some((due_at, end)) = match_bare_clock(&tokens, i, now) {
            return Some(found(i, due_at, end));
        }
    }
    None
}

// --- AI fallback ---

/// Tolerant JSON extraction: models wrap objects in ``` fences or prose.
/// Returns the first balanced `{…}` that parses.
pub fn extract_json(reply: &str) -> Option<serde_json::Value> {
    let bytes = reply.as_bytes();
    let start = reply.find('{')?;
    let (mut depth, mut in_string, mut escaped) = (0usize, false, false);
    for (offset, &b) in bytes[start..].iter().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        match b {
            b'\\' if in_string => escaped = true,
            b'"' => in_string = !in_string,
            b'{' if !in_string => depth += 1,
            b'}' if !in_string => {
                depth -= 1;
                if depth == 0 {
                    return serde_json::from_str(&reply[start..=start + offset]).ok();
                }
            }
            _ => {}
        }
    }
    None
}

fn extract_prompt(now: DateTime<Local>) -> String {
    format!(
        "You convert a reminder request into JSON. The current local time is {}. \
         Reply with ONLY a JSON object, no prose and no code fences, shaped exactly like:\n\
         {{\"title\": \"<the task, without the time phrase>\", \"dueAt\": \"<RFC3339 timestamp with offset>\"}}\n\
         The timestamp must be in the future and use the same UTC offset as the current time. \
         If the request names no time at all, reply with {{}}.",
        now.to_rfc3339()
    )
}

/// One provider round-trip to resolve a time the local parser couldn't.
async fn extract_with_ai(
    app: &AppHandle,
    text: &str,
    now: DateTime<Local>,
) -> Result<Parsed, String> {
    let opts = ai::RequestOptions {
        system: Some(extract_prompt(now)),
        ..Default::default()
    };
    let reply = ai::complete_with_options(app, text, opts, ai::UsageSource::ReminderParse).await?;
    let value = extract_json(&reply).ok_or_else(when_error)?;
    let due_at = value
        .get("dueAt")
        .and_then(|v| v.as_str())
        .and_then(parse_due)
        .ok_or_else(when_error)?;
    if due_at <= now {
        return Err(when_error());
    }
    let title = value
        .get("title")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| strip_lead_ins(text));
    Ok(Parsed { due_at, title })
}

fn when_error() -> String {
    "Couldn't work out when — try \"in 2 hours\" or \"tomorrow at 9am\".".to_string()
}

// --- scheduler ---

/// Poll for due reminders forever. Started once from `lib.rs` setup; the first
/// tick completes immediately, so anything that came due while Claudy was
/// closed fires right after launch.
pub fn start_scheduler(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(TICK_SECS));
        loop {
            ticker.tick().await;
            fire_due(&app);
        }
    });
}

fn fire_due(app: &AppHandle) {
    let now = Local::now();
    let list = match load(app) {
        Ok(l) => l,
        Err(e) => {
            // Silent here meant every reminder stopped firing forever while
            // the tray and the scheduler both looked healthy — the save
            // failure three lines down was already logged, so match it.
            eprintln!("reminders: could not read the queue: {e}");
            return;
        }
    };
    let due = due_now(&list, now);
    if due.is_empty() {
        return;
    }
    // Compose the messages from the snapshot, persist, and only then show
    // them. Showing a toast is a synchronous OS call that can sit for a long
    // time, and this list is written back wholesale — anything the user saved
    // or deleted while we waited inside it would be silently reverted.
    let bodies: Vec<String> = due
        .iter()
        .filter_map(|id| list.iter().find(|r| &r.id == id))
        .map(|r| notification_body(r, now))
        .collect();
    let updated = mark_fired(list, &due, &now.to_rfc3339());
    if let Err(e) = save_list(app, &updated) {
        eprintln!("reminders: could not persist fired state: {e}");
    }
    let enabled = config::load(app)
        .map(|s| s.notifications_enabled)
        .unwrap_or(true);
    for body in &bodies {
        notify::send_with_title(app, enabled, NOTIFICATION_TITLE, body);
    }
    // Metadata only: that a reminder fired, never which one.
    for _ in &due {
        crate::usage::record(app, crate::usage::UsageEvent::now(crate::usage::Source::ReminderFired));
    }
    let _ = app.emit(CHANGED_EVENT, ());
}

// --- commands ---

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReminderPreview {
    /// The task with the time phrase removed.
    pub title: String,
    /// RFC3339, or `None` when the local parser found no time. The frontend
    /// formats this — Rust labelling exists only for notification text.
    pub due_at: Option<String>,
}

/// Drives the live chip under the ask box. LOCAL PARSER ONLY — this runs on
/// every keystroke, so it must never touch the network.
#[tauri::command]
pub fn preview_reminder(text: String) -> ReminderPreview {
    match parse_when(&text, Local::now()) {
        Some(p) => ReminderPreview { title: p.title, due_at: Some(p.due_at.to_rfc3339()) },
        None => ReminderPreview { title: strip_lead_ins(&text), due_at: None },
    }
}

/// The Enter path in Remind Mode: local parser, AI fallback, save, notify.
#[tauri::command]
pub async fn create_reminder_from_text(app: AppHandle, text: String) -> Result<Reminder, String> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("Type what you want to be reminded about".into());
    }
    let now = Local::now();
    let parsed = match parse_when(&text, now) {
        Some(p) => p,
        None => extract_with_ai(&app, &text, now).await?,
    };

    let reminder = Reminder {
        id: uuid::Uuid::new_v4().to_string(),
        title: parsed.title,
        text,
        due_at: parsed.due_at.to_rfc3339(),
        created_at: now.to_rfc3339(),
        fired_at: String::new(),
        status: Status::Pending,
    };
    let list = upsert(load(&app)?, reminder.clone());
    save_list(&app, &list)?;
    crate::usage::record(&app, crate::usage::UsageEvent::now(crate::usage::Source::ReminderCreated));

    let enabled = config::load(&app)
        .map(|s| s.notifications_enabled)
        .unwrap_or(true);
    notify::send(
        &app,
        enabled,
        &format!("Reminder set for {}", format_due_label(parsed.due_at, now)),
    );
    let _ = app.emit(CHANGED_EVENT, ());
    Ok(reminder)
}

#[tauri::command]
pub fn list_reminders(app: AppHandle) -> Result<Vec<Reminder>, String> {
    load(&app)
}

/// Upsert. Empty id = create (uuid assigned); returns the stored reminder.
/// Backs both manual add and edit on the Reminders page.
#[tauri::command]
pub fn save_reminder(app: AppHandle, mut reminder: Reminder) -> Result<Reminder, String> {
    if reminder.text.trim().is_empty() {
        return Err("Reminder text must not be empty".into());
    }
    let due = parse_due(&reminder.due_at).ok_or("Reminder needs a valid date and time")?;
    // This command backs both add and edit; only an add is a new reminder.
    let is_new = reminder.id.trim().is_empty();
    if is_new {
        reminder.id = uuid::Uuid::new_v4().to_string();
    }
    if reminder.created_at.trim().is_empty() {
        reminder.created_at = Local::now().to_rfc3339();
    }
    if reminder.title.trim().is_empty() {
        reminder.title = strip_lead_ins(&reminder.text);
    }
    // Normalize so ordering and comparisons never depend on input formatting.
    reminder.due_at = due.to_rfc3339();

    let list = upsert(load(&app)?, reminder.clone());
    save_list(&app, &list)?;
    if is_new {
        crate::usage::record(
            &app,
            crate::usage::UsageEvent::now(crate::usage::Source::ReminderCreated),
        );
    }
    let _ = app.emit(CHANGED_EVENT, ());
    Ok(reminder)
}

#[tauri::command]
pub fn delete_reminder(app: AppHandle, id: String) -> Result<(), String> {
    let list = remove(load(&app)?, &id);
    save_list(&app, &list)?;
    let _ = app.emit(CHANGED_EVENT, ());
    Ok(())
}

/// Push a reminder out and re-arm it. Works from any status, so a dismissed
/// one can be brought back.
#[tauri::command]
pub fn snooze_reminder(app: AppHandle, id: String, minutes: i64) -> Result<Reminder, String> {
    if minutes < 1 {
        return Err("Snooze must be at least a minute".into());
    }
    let list = load(&app)?;
    let mut reminder = list
        .iter()
        .find(|r| r.id == id)
        .cloned()
        .ok_or("That reminder no longer exists")?;
    // Same overflow hazard as the parser: `minutes` crosses the IPC boundary
    // as an unvalidated i64 and both `Duration::minutes` and the add panic.
    let due = shift_seconds(Local::now(), minutes, 60)
        .ok_or("That snooze is too far in the future")?;
    reminder.due_at = due.to_rfc3339();
    reminder.status = Status::Pending;
    reminder.fired_at = String::new();

    let list = upsert(list, reminder.clone());
    save_list(&app, &list)?;
    let _ = app.emit(CHANGED_EVENT, ());
    Ok(reminder)
}

#[tauri::command]
pub fn complete_reminder(app: AppHandle, id: String) -> Result<(), String> {
    let mut list = load(&app)?;
    let target = list
        .iter_mut()
        .find(|r| r.id == id)
        .ok_or("That reminder no longer exists")?;
    target.status = Status::Done;
    save_list(&app, &list)?;
    let _ = app.emit(CHANGED_EVENT, ());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_day_named_after_the_clock_still_sets_the_day() {
        // The matcher priority is meant to run over the whole string ("a day
        // before a day-less 'at 5pm'"), so a clock earlier in the sentence
        // must not win over a day word later in it.
        let now = Local.with_ymd_and_hms(2026, 7, 24, 14, 30, 0).unwrap();
        let p = parse_when("dinner at 7pm tomorrow", now).expect("should parse");
        assert_eq!(p.due_at, Local.with_ymd_and_hms(2026, 7, 25, 19, 0, 0).unwrap());
        assert!(!p.title.contains("tomorrow"), "the day token must be consumed: {}", p.title);

        let p = parse_when("standup at 9am on monday", now).expect("should parse");
        assert_eq!(p.due_at, Local.with_ymd_and_hms(2026, 7, 27, 9, 0, 0).unwrap());
        assert!(!p.title.contains("monday"), "got {}", p.title);
    }

    #[test]
    fn tonight_keeps_its_evening_reading_when_a_bare_hour_follows() {
        // "tonight" means the evening; a following bare 1-11 hour with no
        // am/pm must not be taken literally as the morning.
        for hour_now in [7, 11, 19] {
            let now = Local.with_ymd_and_hms(2026, 7, 24, hour_now, 0, 0).unwrap();
            let p = parse_when("drinks tonight at 8", now)
                .unwrap_or_else(|| panic!("should parse at {hour_now}:00"));
            assert_eq!(
                p.due_at,
                Local.with_ymd_and_hms(2026, 7, 24, 20, 0, 0).unwrap(),
                "at {hour_now}:00"
            );
        }
    }

    #[test]
    fn an_absurd_offset_is_rejected_rather_than_panicking() {
        // `preview_reminder` is a sync command run on every keystroke, so a
        // panic here unwinds straight into the IPC handler and takes the
        // window with it. Any i64 reaches this via `word_number`.
        let now = Local.with_ymd_and_hms(2026, 7, 24, 14, 30, 0).unwrap();
        for text in [
            "remind me in 999999999 days",
            "check in 99999999999999999s",
            "ping in 9223372036854775807 weeks",
            "in 9223372036854775807 minutes",
        ] {
            assert!(parse_when(text, now).is_none(), "{text} must not parse");
        }
    }


    /// Fixed reference point for every parser test: Fri 24 Jul 2026, 14:30.
    fn now() -> DateTime<Local> {
        Local
            .with_ymd_and_hms(2026, 7, 24, 14, 30, 0)
            .single()
            .expect("unambiguous test instant")
    }

    fn parse(text: &str) -> Parsed {
        parse_when(text, now()).unwrap_or_else(|| panic!("no time found in {text:?}"))
    }

    fn due(text: &str) -> String {
        parse(text).due_at.format("%Y-%m-%d %H:%M").to_string()
    }

    fn reminder(id: &str, due_at: &str, status: Status) -> Reminder {
        Reminder {
            id: id.into(),
            text: "do the thing".into(),
            title: "do the thing".into(),
            due_at: due_at.into(),
            status,
            ..Reminder::default()
        }
    }

    // --- relative offsets ---

    #[test]
    fn parses_spelled_out_relative_offsets() {
        assert_eq!(due("follow up in 2 hours"), "2026-07-24 16:30");
        assert_eq!(due("stand up in 30 minutes"), "2026-07-24 15:00");
        assert_eq!(due("ping me in 3 days"), "2026-07-27 14:30");
        assert_eq!(due("review it in a week"), "2026-07-31 14:30");
    }

    #[test]
    fn parses_compact_and_abbreviated_offsets() {
        assert_eq!(due("check in 2h"), "2026-07-24 16:30");
        assert_eq!(due("check in 90m"), "2026-07-24 16:00");
        assert_eq!(due("check in 45 min"), "2026-07-24 15:15");
        assert_eq!(due("check in 1d"), "2026-07-25 14:30");
    }

    #[test]
    fn rejects_a_zero_length_offset() {
        // "in 0 minutes" resolves to now, which is not a usable reminder.
        assert!(parse_when("do it in 0 minutes", now()).is_none());
    }

    // --- clock times without a day ---

    #[test]
    fn a_bare_clock_time_later_today_stays_today() {
        assert_eq!(due("call the bank at 5pm"), "2026-07-24 17:00");
        assert_eq!(due("call the bank at 17:30"), "2026-07-24 17:30");
        assert_eq!(due("call the bank at 5:45 pm"), "2026-07-24 17:45");
    }

    #[test]
    fn a_clock_time_already_past_today_rolls_to_tomorrow() {
        // 14:30 "now": 9am is gone, and an explicit meridiem must not be
        // reinterpreted as 21:00.
        assert_eq!(due("call the bank at 9am"), "2026-07-25 09:00");
        assert_eq!(due("call the bank at 11:00"), "2026-07-25 11:00");
    }

    #[test]
    fn a_bare_hour_without_meridiem_prefers_the_soonest_reading() {
        // "at 9" at 14:30 means tonight, not tomorrow morning.
        assert_eq!(due("call the bank at 9"), "2026-07-24 21:00");
        // 02:00 today is gone and 14:00 is too, so this lands tomorrow.
        assert_eq!(due("call the bank at 2"), "2026-07-25 02:00");
    }

    #[test]
    fn understands_noon_and_midnight() {
        assert_eq!(due("eat at noon"), "2026-07-25 12:00"); // 12:00 already past
        assert_eq!(due("wrap up at midnight"), "2026-07-25 00:00");
    }

    // --- day anchors ---

    #[test]
    fn a_day_without_a_clock_time_defaults_to_nine_am() {
        assert_eq!(due("email them tomorrow"), "2026-07-25 09:00");
        assert_eq!(due("email them next friday"), "2026-07-31 09:00");
    }

    #[test]
    fn parses_a_day_with_an_explicit_time() {
        assert_eq!(due("email them tomorrow at 9am"), "2026-07-25 09:00");
        assert_eq!(due("email them tomorrow at 16:45"), "2026-07-25 16:45");
        assert_eq!(due("email them on monday at 9am"), "2026-07-27 09:00");
        assert_eq!(due("email them tomorrow 9am"), "2026-07-25 09:00");
    }

    #[test]
    fn a_bare_weekday_never_means_today() {
        // "now" is a Friday; "friday" must mean the NEXT one.
        assert_eq!(due("ship it friday"), "2026-07-31 09:00");
        assert_eq!(due("ship it on saturday"), "2026-07-25 09:00");
    }

    #[test]
    fn understands_named_parts_of_the_day() {
        assert_eq!(due("call them tomorrow morning"), "2026-07-25 09:00");
        assert_eq!(due("call them tomorrow evening"), "2026-07-25 18:00");
        assert_eq!(due("call them tonight"), "2026-07-24 20:00");
    }

    #[test]
    fn today_with_a_time_that_already_passed_is_not_a_match() {
        // Nothing else in the string resolves, so the parser declines rather
        // than silently scheduling a reminder in the past.
        assert!(parse_when("call them today at 9am", now()).is_none());
    }

    // --- title stripping ---

    #[test]
    fn the_title_is_the_text_minus_the_time_expression() {
        assert_eq!(parse("follow up on this email in 2 hours").title, "follow up on this email");
        assert_eq!(parse("in 2 hours follow up on this email").title, "follow up on this email");
        assert_eq!(parse("call the bank at 5pm").title, "call the bank");
    }

    #[test]
    fn the_title_drops_lead_ins_and_dangling_connectors() {
        assert_eq!(parse("remind me to buy milk in 2 hours").title, "buy milk");
        assert_eq!(parse("Remember to renew the domain tomorrow").title, "renew the domain");
        assert_eq!(parse("send the invoice by tomorrow").title, "send the invoice");
    }

    #[test]
    fn a_text_that_is_only_a_time_keeps_the_original_as_its_title() {
        assert_eq!(parse("in 2 hours").title, "in 2 hours");
    }

    #[test]
    fn text_with_no_time_expression_does_not_parse() {
        assert!(parse_when("follow up on this email", now()).is_none());
        assert!(parse_when("", now()).is_none());
        assert!(parse_when("buy 3 apples and 2 pears", now()).is_none());
    }

    // --- due selection & firing ---

    #[test]
    fn due_now_returns_only_pending_reminders_that_are_past_due() {
        let past = (now() - Duration::hours(1)).to_rfc3339();
        let future = (now() + Duration::hours(1)).to_rfc3339();
        let list = vec![
            reminder("a", &past, Status::Pending),
            reminder("b", &future, Status::Pending),
            reminder("c", &past, Status::Fired),
            reminder("d", &past, Status::Done),
        ];
        assert_eq!(due_now(&list, now()), vec!["a".to_string()]);
    }

    #[test]
    fn a_reminder_due_exactly_now_fires() {
        let list = vec![reminder("a", &now().to_rfc3339(), Status::Pending)];
        assert_eq!(due_now(&list, now()).len(), 1);
    }

    #[test]
    fn an_unreadable_due_time_fires_once_instead_of_hiding_forever() {
        let list = vec![reminder("a", "not-a-timestamp", Status::Pending)];
        assert_eq!(due_now(&list, now()), vec!["a".to_string()]);
    }

    #[test]
    fn mark_fired_stamps_only_the_given_ids() {
        let future = (now() + Duration::hours(1)).to_rfc3339();
        let list = vec![
            reminder("a", &future, Status::Pending),
            reminder("b", &future, Status::Pending),
        ];
        let out = mark_fired(list, &["a".to_string()], "STAMP");
        assert_eq!(out[0].status, Status::Fired);
        assert_eq!(out[0].fired_at, "STAMP");
        assert_eq!(out[1].status, Status::Pending);
        assert_eq!(out[1].fired_at, "");
    }

    // --- notification text ---

    #[test]
    fn notification_body_is_just_the_title_when_on_time() {
        let r = reminder("a", &now().to_rfc3339(), Status::Pending);
        assert_eq!(notification_body(&r, now()), "do the thing");
    }

    #[test]
    fn notification_body_flags_a_meaningfully_late_fire_as_overdue() {
        let r = reminder("a", &(now() - Duration::hours(3)).to_rfc3339(), Status::Pending);
        let body = notification_body(&r, now());
        assert!(body.starts_with("do the thing"), "got: {body}");
        assert!(body.contains("overdue") && body.contains("Today 11:30"), "got: {body}");
    }

    #[test]
    fn a_fire_inside_the_poll_window_is_not_called_overdue() {
        let r = reminder("a", &(now() - Duration::seconds(20)).to_rfc3339(), Status::Pending);
        assert!(!notification_body(&r, now()).contains("overdue"));
    }

    #[test]
    fn notification_body_falls_back_to_the_raw_text_when_the_title_is_empty() {
        let mut r = reminder("a", &now().to_rfc3339(), Status::Pending);
        r.title = String::new();
        assert_eq!(notification_body(&r, now()), "do the thing");
    }

    #[test]
    fn format_due_label_names_nearby_days_and_dates_further_out() {
        let n = now();
        assert_eq!(format_due_label(n, n), "Today 14:30");
        assert_eq!(format_due_label(n + Duration::days(1), n), "Tomorrow 14:30");
        assert_eq!(format_due_label(n + Duration::days(5), n), "Wed 29 Jul 14:30");
    }

    // --- AI fallback plumbing ---

    #[test]
    fn extract_json_reads_a_bare_object() {
        let v = extract_json(r#"{"title":"a","dueAt":"2026-07-24T16:00:00+07:00"}"#).unwrap();
        assert_eq!(v["title"], "a");
    }

    #[test]
    fn extract_json_survives_code_fences_and_surrounding_prose() {
        let fenced = "Sure!\n```json\n{\"title\":\"a\",\"dueAt\":\"x\"}\n```\nHope that helps.";
        assert_eq!(extract_json(fenced).unwrap()["title"], "a");
    }

    #[test]
    fn extract_json_handles_nested_objects_and_braces_inside_strings() {
        let reply = r#"{"title":"use {braces}","meta":{"n":1},"dueAt":"x"}"#;
        let v = extract_json(reply).unwrap();
        assert_eq!(v["title"], "use {braces}");
        assert_eq!(v["meta"]["n"], 1);
    }

    #[test]
    fn extract_json_returns_none_for_prose_or_broken_json() {
        assert!(extract_json("I'm not sure when you mean.").is_none());
        assert!(extract_json(r#"{"title": }"#).is_none());
        assert!(extract_json(r#"{"title":"unclosed"#).is_none());
    }

    #[test]
    fn the_extract_prompt_pins_the_current_time_and_the_output_shape() {
        let p = extract_prompt(now());
        assert!(p.contains("2026-07-24T14:30:00"), "must anchor 'now': {p}");
        assert!(p.contains("dueAt") && p.contains("title"));
    }

    // --- list ops & serde ---

    #[test]
    fn upsert_replaces_by_id_or_appends() {
        let list = vec![reminder("a", "x", Status::Pending), reminder("b", "x", Status::Pending)];
        let updated = upsert(list.clone(), reminder("a", "CHANGED", Status::Done));
        assert_eq!(updated.len(), 2);
        assert_eq!(updated[0].due_at, "CHANGED");
        assert_eq!(upsert(list, reminder("c", "x", Status::Pending)).len(), 3);
    }

    #[test]
    fn remove_drops_only_the_matching_id() {
        let list = vec![reminder("a", "x", Status::Pending), reminder("b", "x", Status::Pending)];
        let out = remove(list, "a");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, "b");
    }

    #[test]
    fn reminder_serde_is_camel_case_with_every_field_present() {
        let v = serde_json::to_value(Reminder::default()).unwrap();
        for field in ["id", "text", "title", "dueAt", "createdAt", "firedAt", "status"] {
            assert!(v.get(field).is_some(), "missing field {field}");
        }
        assert_eq!(v["status"], "pending");
    }

    #[test]
    fn missing_fields_deserialize_to_defaults() {
        let r: Reminder = serde_json::from_value(serde_json::json!({ "text": "do it" })).unwrap();
        assert_eq!(r.text, "do it");
        assert_eq!(r.status, Status::Pending);
        assert_eq!(r.fired_at, "");
    }

    #[test]
    fn an_unusable_status_does_not_fail_the_whole_queue_load() {
        // reminders.json is hand-editable; one typo must cost the status only.
        for bad in [serde_json::json!("firedd"), serde_json::json!(7)] {
            let json = serde_json::json!([{ "id": "a", "text": "keep me", "status": bad }]);
            let list: Vec<Reminder> = serde_json::from_value(json)
                .unwrap_or_else(|e| panic!("status {bad} broke the load: {e}"));
            assert_eq!(list[0].status, Status::Pending);
            assert_eq!(list[0].text, "keep me", "the rest of the entry must survive");
        }
    }

    #[test]
    fn preview_returns_no_due_time_when_the_local_parser_finds_none() {
        let p = preview_reminder("remind me to follow up on this email".into());
        assert_eq!(p.due_at, None);
        assert_eq!(p.title, "follow up on this email");
    }

    #[test]
    fn preview_serializes_camel_case() {
        let v = serde_json::to_value(preview_reminder("x in 2 hours".into())).unwrap();
        assert!(v.get("dueAt").is_some());
        assert!(v.get("title").is_some());
    }
}
