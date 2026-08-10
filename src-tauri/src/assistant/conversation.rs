//! Temporary conversation memory for the quick-ask assistant.
//!
//! Pure by construction: no `AppHandle`, no clock read (the caller injects
//! `now`), no network, no disk. `assistant/mod.rs` owns the state and the AI
//! calls; this file owns exactly one question — what does the model get to see,
//! and what gets folded away when that stops fitting.
//!
//! Memory-only is a privacy guarantee, not an oversight: nothing here can
//! persist, and `assistant::close` drops the whole struct.

use chrono::{DateTime, Local};

use crate::ai::{HistoryTurn, Role};

/// Verbatim-history budget, in estimated tokens. Sized for the *smallest*
/// target rather than the largest: a local Ollama model runs at a 2k–4k context
/// by default and silently truncates rather than erroring, and the current
/// question plus its answer still have to fit after we spend this. Cloud
/// providers never notice the ceiling.
pub const HISTORY_TOKEN_BUDGET: usize = 2_000;

/// Hard cap on replayed exchanges regardless of size. Keeps the payload
/// predictable and stops a long chatty session paying per-message overhead for
/// turns nobody will refer back to.
pub const MAX_VERBATIM_TURNS: usize = 6;

/// The immediately previous exchange is always replayed, even when it alone
/// blows the budget — it is what "it", "that" and "they" point at. Over budget
/// its answer is truncated, never dropped.
pub const MIN_VERBATIM_TURNS: usize = 1;

/// Fold once this many unfolded exchanges have piled up. One below
/// `MAX_VERBATIM_TURNS`, so the summary is ready *before* selection has to
/// start dropping turns rather than after.
pub const FOLD_AFTER_TURNS: usize = 5;

/// Exchanges kept verbatim when a fold runs; everything older is folded.
pub const KEEP_VERBATIM_ON_FOLD: usize = 2;

/// Ceiling on the stored summary, enforced on the model's reply — a summarizer
/// that ignores the word cap must not be able to grow the system prompt
/// without bound.
pub const MAX_SUMMARY_CHARS: usize = 1_600;

/// Per-answer clamp when building the fold prompt, so summarizing a long
/// session can't overflow the summarizer's own context.
pub const MAX_FOLD_ANSWER_CHARS: usize = 1_200;

/// Stands in for prior attachments. Base64 is never replayed: the model already
/// described the image in its answer, and the bytes would dominate every later
/// request.
pub const IMAGE_MARKER: &str = "[image attached]";

pub const SUMMARY_HEADING: &str = "Earlier in this conversation (summary):";

/// Appended when the session already contains a web-searched answer. Without
/// it, models happily answer "and tomorrow?" out of the stale grounded block
/// sitting in their context instead of searching again.
pub const FRESHNESS_NOTE: &str = "Some earlier answers came from a web search. \
Search again for anything time-sensitive instead of reusing them.";

pub const TRUNCATION_MARKER: &str = " …[trimmed]";

/// Rough per-message overhead (role, delimiters) the providers add around each
/// message. Counted so a long run of tiny turns isn't budgeted as free.
const ENVELOPE_TOKENS_PER_MESSAGE: usize = 4;

/// Rough token count. No tokenizer ships with the app, and adding one would
/// mean a per-provider vocabulary and a multi-MB dependency for a heuristic
/// decision. ASCII averages ~4 chars per token; CJK and emoji average closer to
/// one token per char, and a naive `len / 4` would under-count them 4x — which
/// is the failure mode that actually truncates a request. Deliberately biased
/// to over-estimate: over-estimating costs a dropped old turn, under-estimating
/// costs a broken prompt.
pub fn estimate_tokens(text: &str) -> usize {
    let mut ascii = 0usize;
    let mut wide = 0usize;
    for c in text.chars() {
        if c.is_ascii() {
            ascii += 1;
        } else {
            wide += 1;
        }
    }
    ascii.div_ceil(4) + wide
}

/// Cut `text` down to `budget` tokens on a char boundary, with a visible
/// marker. The spec forbids silently removing context, so a trim always
/// announces itself.
pub fn truncate_to_tokens(text: &str, budget: usize) -> String {
    if estimate_tokens(text) <= budget {
        return text.to_string();
    }
    let allowance = budget.saturating_sub(estimate_tokens(TRUNCATION_MARKER));
    let mut ascii = 0usize;
    let mut wide = 0usize;
    let mut end = 0usize;
    for (i, c) in text.char_indices() {
        let (a, w) = if c.is_ascii() { (ascii + 1, wide) } else { (ascii, wide + 1) };
        if a.div_ceil(4) + w > allowance {
            break;
        }
        ascii = a;
        wide = w;
        end = i + c.len_utf8();
    }
    let mut out = text[..end].trim_end().to_string();
    out.push_str(TRUNCATION_MARKER);
    out
}

/// Cut to a character count on a char boundary. Used on model replies, the one
/// input here whose length we don't control.
fn clamp_chars(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        None => text.to_string(),
        Some((i, _)) => text[..i].trim_end().to_string(),
    }
}

/// One completed exchange. Stored verbatim; the flattening for the wire happens
/// in `select_history`.
#[derive(Debug, Clone, PartialEq)]
pub struct Turn {
    pub question: String,
    pub answer: String,
    /// The answer was grounded in a provider-native web search. There are no
    /// client-visible tool results to store — Anthropic's parser keeps only
    /// text blocks and Gemini concatenates text parts — so the answer text *is*
    /// the preserved search result, and this flag records its provenance.
    pub web_search: bool,
    /// The question carried images. Replayed as `IMAGE_MARKER`, never bytes.
    pub had_images: bool,
    /// Part of the session record the feature asks for; kept for diagnostics
    /// rather than read on the request path.
    #[allow(dead_code)]
    pub at: String,
}

impl Turn {
    /// The question as a later request should see it: prior attachments become
    /// a marker. An image-only ask (legal today) becomes just the marker.
    fn question_text(&self) -> String {
        if !self.had_images {
            return self.question.clone();
        }
        if self.question.trim().is_empty() {
            IMAGE_MARKER.to_string()
        } else {
            format!("{} {IMAGE_MARKER}", self.question)
        }
    }

    /// What replaying this exchange costs: both messages plus their envelopes.
    fn cost(&self) -> usize {
        estimate_tokens(&self.question_text())
            + estimate_tokens(&self.answer)
            + 2 * ENVELOPE_TOKENS_PER_MESSAGE
    }
}

/// One quick-chat session. Every part the feature asks for lives here: id,
/// system prompt, the user/assistant messages, search provenance, the folded
/// summary, and both timestamps.
#[derive(Debug, Clone)]
pub struct Conversation {
    pub id: String,
    /// The user's configured system prompt as of the last question. Refreshed
    /// per ask so a Settings edit still applies mid-session.
    pub system: Option<String>,
    /// Oldest first. Never reordered or removed; `folded` moves instead.
    pub turns: Vec<Turn>,
    pub summary: Option<String>,
    /// How many leading turns `summary` already covers. Monotonic — the only
    /// thing that makes the async fold race-safe.
    pub folded: usize,
    /// A fold is in flight; suppresses a second one.
    pub folding: bool,
    /// Part of the session record the feature asks for; kept for diagnostics
    /// rather than read on the request path.
    #[allow(dead_code)]
    pub created_at: String,
    #[allow(dead_code)]
    pub last_active_at: String,
}

/// What a background fold needs: the prompt to send, and the boundary the
/// resulting summary will cover.
#[derive(Debug, Clone, PartialEq)]
pub struct FoldRequest {
    pub prompt: String,
    pub boundary: usize,
}

impl Conversation {
    /// A brand new session. `now` is injected so timestamps are deterministic
    /// under test — the `reminders` / `usage` convention.
    pub fn new(now: DateTime<Local>) -> Self {
        let at = now.to_rfc3339();
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            system: None,
            turns: Vec::new(),
            summary: None,
            folded: 0,
            folding: false,
            created_at: at.clone(),
            last_active_at: at,
        }
    }

    /// Exchanges the user can still refer back to — what drives the follow-up
    /// chip. Counts folded turns too: they are represented by the summary, not
    /// forgotten.
    pub fn turn_count(&self) -> usize {
        self.turns.len()
    }

    /// Append a completed exchange and touch `last_active_at`. Only called
    /// after a *successful* answer: a failed request leaves the session exactly
    /// as it was, so Retry re-asks against the same context.
    pub fn record_turn(
        &mut self,
        question: String,
        answer: String,
        web_search: bool,
        had_images: bool,
        now: DateTime<Local>,
    ) {
        let at = now.to_rfc3339();
        self.turns.push(Turn { question, answer, web_search, had_images, at: at.clone() });
        self.last_active_at = at;
    }

    /// The budgeted replay, oldest first. Excludes the current question, which
    /// the provider adds as the final message.
    pub fn history(&self) -> Vec<HistoryTurn> {
        let start = self.folded.min(self.turns.len());
        select_history(&self.turns[start..], HISTORY_TOKEN_BUDGET, MAX_VERBATIM_TURNS)
    }

    /// System instruction for the next request. `None` when there is nothing to
    /// say — the invariant that keeps a session's first question producing
    /// today's exact request body.
    pub fn request_system(&self, web_search: bool) -> Option<String> {
        compose_system(
            self.system.as_deref(),
            self.summary.as_deref(),
            web_search && self.has_searched(),
        )
    }

    /// The fold to run, or `None` when the session is small enough, one is
    /// already in flight, or there is nothing new to fold. Sets `folding`.
    pub fn take_fold_request(&mut self) -> Option<FoldRequest> {
        if self.folding {
            return None;
        }
        if self.turns.len().saturating_sub(self.folded) <= FOLD_AFTER_TURNS {
            return None;
        }
        let boundary = self.turns.len().saturating_sub(KEEP_VERBATIM_ON_FOLD);
        if boundary <= self.folded {
            return None;
        }
        let prompt = fold_prompt(self.summary.as_deref(), &self.turns[self.folded..boundary]);
        self.folding = true;
        Some(FoldRequest { prompt, boundary })
    }

    /// Install a completed fold. Ignores a stale boundary — the session kept
    /// growing while the summarizer ran, and a later fold may already have
    /// landed. Always clears `folding`.
    pub fn apply_fold(&mut self, boundary: usize, summary: String) -> bool {
        self.folding = false;
        let summary = summary.trim();
        if summary.is_empty() || boundary <= self.folded || boundary > self.turns.len() {
            return false;
        }
        self.summary = Some(clamp_chars(summary, MAX_SUMMARY_CHARS));
        self.folded = boundary;
        true
    }

    /// Clear `folding` after a failed fold, leaving the session usable. The
    /// next answer simply tries again, and selection hard-trims meanwhile.
    pub fn fold_failed(&mut self) {
        self.folding = false;
    }

    /// Any exchange in this session was answered with a web search.
    fn has_searched(&self) -> bool {
        self.turns.iter().any(|t| t.web_search)
    }
}

/// Select which exchanges to replay, walking newest-first and returning them
/// chronologically.
///
/// The rules, in the feature's priority order:
/// 1. The newest exchange is unconditional (`MIN_VERBATIM_TURNS`) — a follow-up
///    with zero context is useless. Over budget, its *answer* is truncated:
///    answers are the long half and the question carries the referent.
/// 2. Older exchanges are included only while they fit and the recency window
///    has room.
/// 3. **Stop at the first exchange that does not fit — never skip and
///    continue.** A hole in the middle ("as I said earlier" pointing at
///    nothing) changes meaning far more than a clean truncation at the head,
///    and the head is exactly what the summary already covers.
/// 4. Whole pairs only: a lone assistant message breaks Anthropic's alternation
///    requirement and makes pronoun resolution ambiguous.
pub fn select_history(turns: &[Turn], budget: usize, max_turns: usize) -> Vec<HistoryTurn> {
    if turns.is_empty() || max_turns == 0 {
        return Vec::new();
    }
    let mut chosen: Vec<&Turn> = Vec::new();
    let mut used = 0usize;
    for turn in turns.iter().rev() {
        let mandatory = chosen.len() < MIN_VERBATIM_TURNS;
        let cost = turn.cost();
        if !mandatory && (chosen.len() >= max_turns || used + cost > budget) {
            break;
        }
        used += cost;
        chosen.push(turn);
    }
    chosen.reverse();

    let mut out = Vec::with_capacity(chosen.len() * 2);
    for (i, turn) in chosen.iter().enumerate() {
        let question = turn.question_text();
        // Only a mandatory turn can leave us over budget, and mandatory means
        // the newest ones — the tail of `chosen`.
        let over_budget = used > budget && i + MIN_VERBATIM_TURNS >= chosen.len();
        let answer = if over_budget {
            let room = budget
                .saturating_sub(estimate_tokens(&question) + 2 * ENVELOPE_TOKENS_PER_MESSAGE);
            truncate_to_tokens(&turn.answer, room)
        } else {
            turn.answer.clone()
        };
        out.push(HistoryTurn { role: Role::User, content: question });
        out.push(HistoryTurn { role: Role::Assistant, content: answer });
    }
    out
}

/// Assemble the system field from the user's standing prompt, the conversation
/// summary, and the freshness note.
///
/// Returns `None` when all three are absent. That is the single most important
/// invariant here: it is what keeps `RequestOptions::default()` and a session's
/// first question producing the exact request body they produced before this
/// feature existed.
pub fn compose_system(
    base: Option<&str>,
    summary: Option<&str>,
    stale_search: bool,
) -> Option<String> {
    let base = base.map(str::trim).filter(|s| !s.is_empty());
    let summary = summary.map(str::trim).filter(|s| !s.is_empty());

    let mut parts: Vec<String> = Vec::new();
    if let Some(b) = base {
        parts.push(b.to_string());
    }
    if let Some(s) = summary {
        // Under a heading, so the model reads it as context to draw on rather
        // than as instructions to follow.
        parts.push(format!("{SUMMARY_HEADING}\n{s}"));
    }
    if stale_search {
        parts.push(FRESHNESS_NOTE.to_string());
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n\n"))
    }
}

/// The one-shot prompt that folds older exchanges into a compact summary.
///
/// Sent with `RequestOptions::default()` — no system prompt, no history, no
/// images, no search — so a user prompt like "answer as a pirate" cannot
/// corrupt the summary. The previous summary is fed back in so nothing is lost
/// across repeated folds.
pub fn fold_prompt(previous: Option<&str>, turns: &[Turn]) -> String {
    let mut out = String::from(
        "You are compacting the memory of an ongoing assistant conversation.\n\
         Rewrite the notes below into a summary of at most 120 words.\n\n\
         Keep, in this order of priority:\n\
         - facts, numbers and decisions the user may refer back to\n\
         - named entities (people, places, products, files, URLs) and what was said about them\n\
         - anything the user asked for that is still unresolved\n\
         - what pronouns like \"it\", \"that\" or \"they\" currently refer to\n\n\
         Mark any fact that came from a web search as \"(from a web search)\" so it can be\n\
         re-checked later. Drop pleasantries, restated questions, and anything a later\n\
         turn superseded. Write plain prose in the third person. Do not answer anything,\n\
         do not add information that is not in the notes, and do not use Markdown.\n\n\
         NOTES\n",
    );
    if let Some(p) = previous.map(str::trim).filter(|s| !s.is_empty()) {
        out.push_str("Summary so far: ");
        out.push_str(p);
        out.push('\n');
    }
    for turn in turns {
        out.push_str("Q: ");
        out.push_str(turn.question_text().trim());
        out.push_str("\nA: ");
        out.push_str(clamp_chars(turn.answer.trim(), MAX_FOLD_ANSWER_CHARS).trim());
        if turn.web_search {
            out.push_str(" (from a web search)");
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(offset: i64) -> DateTime<Local> {
        Local.timestamp_opt(1_700_000_000 + offset, 0).unwrap()
    }

    fn turn(question: &str, answer: &str) -> Turn {
        Turn {
            question: question.into(),
            answer: answer.into(),
            web_search: false,
            had_images: false,
            at: at(0).to_rfc3339(),
        }
    }

    /// A session with `n` small exchanges, numbered so ordering is checkable.
    fn session_with(n: usize) -> Conversation {
        let mut c = Conversation::new(at(0));
        for i in 1..=n {
            c.record_turn(format!("Q{i}"), format!("A{i}"), false, false, at(i as i64));
        }
        c
    }

    fn contents(history: &[HistoryTurn]) -> Vec<String> {
        history.iter().map(|h| h.content.clone()).collect()
    }

    fn questions(history: &[HistoryTurn]) -> Vec<String> {
        history
            .iter()
            .filter(|h| h.role == Role::User)
            .map(|h| h.content.clone())
            .collect()
    }

    #[test]
    fn estimate_tokens_counts_wide_chars_individually() {
        // ASCII follows the ~4-chars-per-token rule of thumb.
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("a"), 1);
        assert_eq!(estimate_tokens("abcdefgh"), 2);
        // CJK would be under-counted 4x by a naive len/4; it must not be.
        let cjk = "这是一个很长的中文句子";
        assert_eq!(estimate_tokens(cjk), cjk.chars().count());
        assert!(estimate_tokens(cjk) > cjk.chars().count() / 4);
    }

    #[test]
    fn truncate_to_tokens_cuts_on_a_char_boundary_and_marks_it() {
        let long = "日本語のテキストがとても長い場合の切り詰め".repeat(4);
        let cut = truncate_to_tokens(&long, 20);
        assert!(cut.ends_with(TRUNCATION_MARKER), "a trim must announce itself: {cut}");
        assert!(cut.chars().count() < long.chars().count());
        assert!(estimate_tokens(&cut) <= 20, "got {} tokens", estimate_tokens(&cut));
        // Short input comes back untouched.
        assert_eq!(truncate_to_tokens("short", 100), "short");
    }

    #[test]
    fn a_fresh_session_sends_no_history_and_no_system() {
        // The byte-identical-first-question guarantee: a brand new session must
        // build exactly the request this app built before the feature existed.
        let c = Conversation::new(at(0));
        assert!(c.history().is_empty());
        assert_eq!(c.request_system(false), None);
        assert_eq!(c.request_system(true), None);
        assert_eq!(c.turn_count(), 0);
    }

    #[test]
    fn record_turn_appends_and_touches_last_activity() {
        let mut c = Conversation::new(at(0));
        let created = c.created_at.clone();
        c.record_turn("Q1".into(), "A1".into(), false, false, at(60));
        assert_eq!(c.turn_count(), 1);
        assert_eq!(c.turns[0].question, "Q1");
        assert_ne!(c.last_active_at, created, "activity must move with a new turn");
    }

    #[test]
    fn history_returns_whole_pairs_oldest_first() {
        let c = session_with(2);
        let h = c.history();
        assert_eq!(
            h.iter().map(|t| t.role).collect::<Vec<_>>(),
            [Role::User, Role::Assistant, Role::User, Role::Assistant]
        );
        assert_eq!(contents(&h), ["Q1", "A1", "Q2", "A2"]);
    }

    #[test]
    fn history_always_keeps_the_newest_turn_even_when_it_alone_exceeds_the_budget() {
        let mut c = Conversation::new(at(0));
        c.record_turn("Why?".into(), "x".repeat(40_000), false, false, at(1));
        let h = c.history();
        assert_eq!(h.len(), 2, "the exchange is kept, not dropped");
        assert_eq!(h[0].content, "Why?", "the question carries the referent — keep it whole");
        assert!(h[1].content.ends_with(TRUNCATION_MARKER), "the answer is visibly trimmed");
        assert!(estimate_tokens(&h[1].content) <= HISTORY_TOKEN_BUDGET);
    }

    #[test]
    fn history_drops_the_oldest_turns_first_when_over_budget() {
        let mut c = Conversation::new(at(0));
        // Each answer costs ~half the budget, so only the newest two can fit.
        let big = "word ".repeat(HISTORY_TOKEN_BUDGET * 2 / 5);
        for i in 1..=4 {
            c.record_turn(format!("Q{i}"), format!("{i} {big}"), false, false, at(i));
        }
        let qs = questions(&c.history());
        assert!(qs.len() < 4, "the budget must have dropped something: {qs:?}");
        assert!(qs.iter().any(|q| q == "Q4"), "the newest must survive: {qs:?}");
        assert!(!qs.iter().any(|q| q == "Q1"), "the oldest must go first: {qs:?}");
    }

    #[test]
    fn history_stops_at_the_first_turn_that_does_not_fit_rather_than_leaving_a_gap() {
        let mut c = Conversation::new(at(0));
        c.record_turn("Q1".into(), "tiny".into(), false, false, at(1));
        c.record_turn("Q2".into(), "x".repeat(HISTORY_TOKEN_BUDGET * 8), false, false, at(2));
        c.record_turn("Q3".into(), "tiny".into(), false, false, at(3));
        let qs = questions(&c.history());
        assert!(qs.iter().any(|q| q == "Q3"), "the newest survives: {qs:?}");
        assert!(
            !qs.iter().any(|q| q == "Q1"),
            "Q1 must NOT be reinstated past the oversized Q2 — that would leave a hole: {qs:?}"
        );
    }

    #[test]
    fn history_never_exceeds_the_recency_window() {
        let c = session_with(20);
        assert_eq!(c.history().len(), MAX_VERBATIM_TURNS * 2);
    }

    #[test]
    fn history_replaces_image_payloads_with_a_marker() {
        let mut c = Conversation::new(at(0));
        c.record_turn("What is this?".into(), "A chart.".into(), false, true, at(1));
        let h = c.history();
        assert_eq!(h[0].content, format!("What is this? {IMAGE_MARKER}"));

        // An image-only ask (legal today) becomes just the marker.
        let mut only = Conversation::new(at(0));
        only.record_turn(String::new(), "A chart.".into(), false, true, at(1));
        assert_eq!(only.history()[0].content, IMAGE_MARKER);
    }

    #[test]
    fn history_skips_turns_the_summary_already_covers() {
        let mut c = session_with(5);
        assert!(c.apply_fold(3, "Notes about Q1-Q3.".into()));
        assert_eq!(contents(&c.history()), ["Q4", "A4", "Q5", "A5"]);
    }

    #[test]
    fn request_system_composes_prompt_and_summary_and_nothing_else() {
        let mut c = session_with(5);
        c.system = Some("Be brief.".into());
        c.apply_fold(3, "Tauri was discussed.".into());
        let s = c.request_system(false).unwrap();
        assert!(s.starts_with("Be brief."), "the user's prompt comes first: {s}");
        assert!(s.contains(SUMMARY_HEADING));
        assert!(s.contains("Tauri was discussed."));
        assert!(!s.contains(FRESHNESS_NOTE), "nothing was searched: {s}");
    }

    #[test]
    fn request_system_adds_the_freshness_note_only_when_search_is_on_and_used() {
        let mut plain = session_with(1);
        plain.system = Some("Be brief.".into());
        // Nothing was searched, so the note would be a lie either way.
        assert!(!plain.request_system(false).unwrap().contains(FRESHNESS_NOTE));
        assert!(!plain.request_system(true).unwrap().contains(FRESHNESS_NOTE));

        let mut searched = Conversation::new(at(0));
        searched.record_turn("Weather?".into(), "12C".into(), true, false, at(1));
        // Search is off for this request, so there is nothing to re-run.
        assert_eq!(searched.request_system(false), None);
        assert!(searched.request_system(true).unwrap().contains(FRESHNESS_NOTE));
    }

    #[test]
    fn request_system_returns_a_summary_only_prompt_when_the_user_configured_none() {
        let mut c = session_with(5);
        c.apply_fold(3, "Tauri was discussed.".into());
        let s = c.request_system(false).unwrap();
        assert!(s.starts_with(SUMMARY_HEADING), "no leading blank line or separator: {s:?}");
    }

    #[test]
    fn compose_system_covers_every_combination() {
        assert_eq!(compose_system(None, None, false), None);
        assert_eq!(compose_system(Some("  "), Some(" "), false), None);
        assert_eq!(compose_system(Some("Base."), None, false).as_deref(), Some("Base."));
        assert_eq!(compose_system(None, None, true).as_deref(), Some(FRESHNESS_NOTE));
        let all = compose_system(Some("Base."), Some("Notes."), true).unwrap();
        assert_eq!(all, format!("Base.\n\n{SUMMARY_HEADING}\nNotes.\n\n{FRESHNESS_NOTE}"));
    }

    #[test]
    fn no_fold_is_requested_below_the_threshold() {
        let mut c = session_with(FOLD_AFTER_TURNS);
        assert_eq!(c.take_fold_request(), None);
    }

    #[test]
    fn fold_request_covers_everything_older_than_the_recency_window() {
        let mut c = session_with(FOLD_AFTER_TURNS + 1);
        let req = c.take_fold_request().unwrap();
        assert_eq!(req.boundary, FOLD_AFTER_TURNS + 1 - KEEP_VERBATIM_ON_FOLD);
        assert!(req.prompt.contains("Q1"), "the folded turns are in the prompt");
        assert!(
            !req.prompt.contains(&format!("Q{}", FOLD_AFTER_TURNS + 1)),
            "the kept window must not be folded: {}",
            req.prompt
        );
    }

    #[test]
    fn fold_request_carries_the_previous_summary_forward() {
        let mut c = session_with(12);
        assert!(c.apply_fold(3, "Earlier notes.".into()));
        let req = c.take_fold_request().unwrap();
        assert!(req.prompt.contains("Summary so far: Earlier notes."));
        assert!(!req.prompt.contains("Q1\n"), "already-folded turns are not re-sent");
    }

    #[test]
    fn only_one_fold_runs_at_a_time() {
        let mut c = session_with(FOLD_AFTER_TURNS + 1);
        assert!(c.take_fold_request().is_some());
        assert_eq!(c.take_fold_request(), None, "a second fold must not start");
        c.fold_failed();
        assert!(c.take_fold_request().is_some(), "a failure must release the latch");
    }

    #[test]
    fn apply_fold_ignores_a_stale_boundary() {
        let mut c = session_with(8);
        assert!(c.apply_fold(4, "Newer.".into()));
        // A fold that started earlier and finished late must not rewind us.
        assert!(!c.apply_fold(2, "Older.".into()));
        assert_eq!(c.folded, 4);
        assert_eq!(c.summary.as_deref(), Some("Newer."));
        // Nor may a boundary past the end of the session land.
        assert!(!c.apply_fold(99, "Impossible.".into()));
        assert_eq!(c.folded, 4);
    }

    #[test]
    fn apply_fold_clamps_an_overlong_summary() {
        let mut c = session_with(4);
        // A wide-char reply also proves the clamp respects char boundaries.
        assert!(c.apply_fold(2, "要約".repeat(MAX_SUMMARY_CHARS)));
        assert!(c.summary.unwrap().chars().count() <= MAX_SUMMARY_CHARS);
    }

    #[test]
    fn fold_failed_leaves_the_session_usable() {
        let mut c = session_with(FOLD_AFTER_TURNS + 1);
        let before = c.turns.clone();
        c.take_fold_request().unwrap();
        c.fold_failed();
        assert!(!c.folding);
        assert_eq!(c.folded, 0);
        assert_eq!(c.summary, None);
        assert_eq!(c.turns, before, "a failed fold must not touch the transcript");
    }

    #[test]
    fn fold_prompt_clamps_long_answers_and_marks_searched_turns() {
        let mut searched = turn("Weather in Stockholm?", &"12C and clear. ".repeat(500));
        searched.web_search = true;
        let prompt = fold_prompt(None, &[searched, turn("And Oslo?", "Colder.")]);
        assert!(prompt.contains("(from a web search)"));
        assert!(
            !prompt.contains("Colder. (from a web search)"),
            "only searched turns get the marker"
        );
        assert!(prompt.len() < 500 * 15, "the long answer was clamped");
    }

    #[test]
    fn turn_count_includes_folded_turns() {
        let mut c = session_with(6);
        c.apply_fold(4, "Notes.".into());
        // The chip counts what the user actually asked, not what is replayed
        // verbatim — the folded turns are represented, not forgotten.
        assert_eq!(c.turn_count(), 6);
        assert_eq!(c.history().len(), 4);
    }
}
