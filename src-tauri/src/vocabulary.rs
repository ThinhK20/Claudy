//! Custom vocabulary: user-defined corrections applied to every transcript
//! before it reaches an app.
//!
//! Whisper is strong on ordinary English and weak on exactly the words that
//! matter most to any one person — colleagues' names, product names, jargon,
//! the company's own spelling. Retraining the model is not an option, so the
//! fix lives here: a short list of "when you hear X, write Y" rules, applied
//! to the finished transcript at the single seam every capture passes
//! through (`stt::transcribe_samples`).
//!
//! Design decisions, all of them safety-first because this rewrites text the
//! user is about to put into another program:
//!
//! * **Whole words only.** A rule "cat" -> "dog" must never turn "category"
//!   into "dogegory". A match needs a non-word character (or an end) on any
//!   side where the pattern itself has a word character.
//! * **One pass, left to right, no re-entry.** Replacements are never
//!   re-examined, so "a" -> "b" plus "b" -> "a" terminates instead of looping.
//! * **Longest pattern wins.** "get hub actions" beats "get hub" at the same
//!   position regardless of the order the rules were typed in.
//! * **Case-insensitive by default, and case-preserving.** "cloudy" heard at
//!   the start of a sentence becomes "Claudy", not "claudy" — but only when
//!   the replacement is written all-lowercase, so a deliberate "GitHub" or
//!   "iPhone" is never mangled.

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

/// Upper bound on stored rules. Mirrored by `MAX_RULES` in
/// `src/components/transcription/vocabulary-manager.tsx`.
pub const MAX_RULES: usize = 200;

/// Upper bound on either side of a single rule. Long enough for a spoken
/// phrase, short enough that no rule can bloat settings.json.
pub const MAX_RULE_CHARS: usize = 200;

/// One "when you hear `from`, write `to`" correction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct VocabRule {
    pub id: String,
    /// What whisper tends to produce. Matched whole-word.
    pub from: String,
    /// What to write instead. Empty deletes the match (filler words).
    pub to: String,
    pub enabled: bool,
    /// When set, only an exact-case match is replaced and `to` is written
    /// verbatim — the escape hatch for rules like "it" -> "IT".
    pub match_case: bool,
}

impl Default for VocabRule {
    fn default() -> Self {
        Self {
            id: String::new(),
            from: String::new(),
            to: String::new(),
            enabled: true,
            match_case: false,
        }
    }
}

impl VocabRule {
    /// Whether this rule can change anything. A blank `from` would match at
    /// every position; a rule that rewrites a word to itself is dead weight.
    fn is_usable(&self) -> bool {
        let from = self.from.trim();
        self.enabled && !from.is_empty() && from != self.to.trim()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct VocabularySettings {
    /// Master switch, so a user can compare raw output against corrected
    /// output without deleting the list they built up.
    pub enabled: bool,
    pub rules: Vec<VocabRule>,
}

impl Default for VocabularySettings {
    fn default() -> Self {
        // On, but empty: existing installs keep today's behavior exactly, and
        // a rule added later works without hunting for a switch.
        Self { enabled: true, rules: Vec::new() }
    }
}

/// Characters that count as "inside a word" for boundary purposes. The
/// apostrophe is included on purpose: it keeps a rule for "dont" from firing
/// inside "don't" and keeps "I" from matching in "I'm".
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '\''
}

fn chars_eq(a: char, b: char, match_case: bool) -> bool {
    if match_case { a == b } else { a.to_lowercase().eq(b.to_lowercase()) }
}

/// Re-cases `to` to match how the speaker's words came out, but only when
/// `to` carries no capitals of its own — a replacement written "GitHub" or
/// "iPhone" was spelled deliberately and is always emitted as written.
fn cased_replacement(matched: &[char], to: &str) -> String {
    if to.chars().any(char::is_uppercase) {
        return to.to_string();
    }
    let letters: Vec<char> = matched.iter().copied().filter(|c| c.is_alphabetic()).collect();
    let Some(&first) = letters.first() else {
        return to.to_string();
    };
    // Two letters minimum for SHOUTING: a lone capital "I" is a normal
    // sentence start, not an acronym.
    if letters.len() > 1 && letters.iter().all(|c| c.is_uppercase()) {
        return to.to_uppercase();
    }
    if !first.is_uppercase() {
        return to.to_string();
    }
    let mut out = String::with_capacity(to.len());
    let mut capitalized = false;
    for c in to.chars() {
        if !capitalized && c.is_alphabetic() {
            out.extend(c.to_uppercase());
            capitalized = true;
        } else {
            out.push(c);
        }
    }
    out
}

/// Does `rule` match `text` starting at `at`? Returns how many characters it
/// consumed — never 0, since `is_usable` rejects a blank pattern.
fn match_len(rule: &VocabRule, from: &[char], text: &[char], at: usize) -> Option<usize> {
    let end = at + from.len();
    if end > text.len() {
        return None;
    }
    if !text[at..end]
        .iter()
        .zip(from)
        .all(|(&t, &f)| chars_eq(t, f, rule.match_case))
    {
        return None;
    }
    // A boundary is only required on a side where the pattern itself ends in
    // a word character — a rule for "..." or "£" has nothing to be cut off.
    let left_ok = !is_word_char(from[0]) || at == 0 || !is_word_char(text[at - 1]);
    let right_ok =
        !is_word_char(from[from.len() - 1]) || end == text.len() || !is_word_char(text[end]);
    (left_ok && right_ok).then_some(from.len())
}

/// Applies every usable rule to `text` in one left-to-right pass.
///
/// Pure and total: any rule list, any text, always terminates, and returns
/// the input unchanged when nothing applies.
pub fn apply(settings: &VocabularySettings, text: &str) -> String {
    if !settings.enabled {
        return text.to_string();
    }
    // Pre-split each pattern once, then order by length so a longer phrase
    // always beats the shorter one it contains.
    let mut rules: Vec<(&VocabRule, Vec<char>)> = settings
        .rules
        .iter()
        .filter(|r| r.is_usable())
        .take(MAX_RULES)
        .map(|r| (r, r.from.trim().chars().collect()))
        .collect();
    if rules.is_empty() {
        return text.to_string();
    }
    // Stable, so rules of equal length keep the order the user typed them in.
    rules.sort_by_key(|(_, from)| std::cmp::Reverse(from.len()));

    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut replaced_anything = false;

    while i < chars.len() {
        let hit = rules
            .iter()
            .find_map(|(rule, from)| match_len(rule, from, &chars, i).map(|len| (*rule, len)));

        let Some((rule, len)) = hit else {
            out.push(chars[i]);
            i += 1;
            continue;
        };

        replaced_anything = true;
        let to = rule.to.trim();
        if to.is_empty() {
            // A deletion would otherwise leave a double space behind
            // ("I um think" -> "I  think"), so swallow one space with it.
            i += len;
            if out.ends_with(' ') && chars.get(i) == Some(&' ') {
                i += 1;
            }
            continue;
        }
        if rule.match_case {
            out.push_str(to);
        } else {
            out.push_str(&cased_replacement(&chars[i..i + len], to));
        }
        i += len;
    }

    // Only a deletion can leave stray edge whitespace, and whisper's own
    // output is already trimmed — so don't touch text nothing happened to.
    if replaced_anything { out.trim().to_string() } else { out }
}

/// Runs the live rules from settings. The single call site is
/// `stt::transcribe_samples`, so dictation, the assistant's spoken questions
/// and the mic test all see the same corrections.
pub fn apply_settings(settings: &crate::config::Settings, text: &str) -> String {
    apply(&settings.vocabulary, text)
}

/// Powers the "Try it" box: runs unsaved draft rules so the user can watch a
/// rule work before committing to it. Same code path as the real thing —
/// there is no second implementation to drift.
#[tauri::command]
pub fn preview_vocabulary(text: String, rules: Vec<VocabRule>) -> String {
    apply(&VocabularySettings { enabled: true, rules }, &text)
}

/// Rejects a rule list that could not have come from the UI. Length-only, in
/// the spirit of `update_settings`: anything stricter risks bouncing a
/// settings write for an unrelated reason.
pub fn validate(settings: &VocabularySettings) -> Result<(), String> {
    if settings.rules.len() > MAX_RULES {
        return Err(format!("Too many vocabulary rules — the limit is {MAX_RULES}"));
    }
    for rule in &settings.rules {
        if rule.from.chars().count() > MAX_RULE_CHARS || rule.to.chars().count() > MAX_RULE_CHARS {
            return Err(format!(
                "A vocabulary rule is longer than {MAX_RULE_CHARS} characters — shorten it"
            ));
        }
    }
    Ok(())
}

/// What an import did, so the UI can report it instead of silently changing
/// the list under the user. Mirrors `prompts::ImportReport`.
#[derive(Debug, Default, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub added: usize,
    pub updated: usize,
    /// Dropped because the list is already at `MAX_RULES`.
    pub skipped: usize,
}

/// Appends rules, replacing any whose `from` already exists (case-insensitive)
/// so importing the same file twice doesn't double the list.
pub fn merge(existing: &[VocabRule], incoming: Vec<VocabRule>) -> (Vec<VocabRule>, ImportReport) {
    let mut out: Vec<VocabRule> = existing.to_vec();
    let mut report = ImportReport::default();
    for rule in incoming {
        let key = rule.from.trim().to_lowercase();
        if key.is_empty() {
            continue;
        }
        match out.iter().position(|r| r.from.trim().to_lowercase() == key) {
            // Keep the existing id so a row already on screen doesn't jump.
            Some(i) => {
                out[i] = VocabRule { id: out[i].id.clone(), ..rule };
                report.updated += 1;
            }
            None if out.len() < MAX_RULES => {
                out.push(rule);
                report.added += 1;
            }
            // Silently truncating would lose rules the user thinks they
            // imported, so the count comes back and the UI says so.
            None => report.skipped += 1,
        }
    }
    (out, report)
}

/// Reads rules out of a shared JSON file. Accepts either a bare array or the
/// object `export_json` writes, and skips entries too broken to use rather
/// than failing the whole import.
pub fn parse_import(json: &str) -> Result<Vec<VocabRule>, String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("Not a valid JSON file: {e}"))?;
    let array = match value {
        serde_json::Value::Array(a) => a,
        serde_json::Value::Object(mut o) => match o.remove("rules") {
            Some(serde_json::Value::Array(a)) => a,
            _ => return Err("No \"rules\" list in this file".into()),
        },
        _ => return Err("Expected a list of vocabulary rules".into()),
    };
    let rules: Vec<VocabRule> = array
        .into_iter()
        .filter_map(|v| serde_json::from_value::<VocabRule>(v).ok())
        .filter(|r| {
            !r.from.trim().is_empty()
                && r.from.chars().count() <= MAX_RULE_CHARS
                && r.to.chars().count() <= MAX_RULE_CHARS
        })
        .map(|r| VocabRule { id: new_id(), ..r })
        .collect();
    if rules.is_empty() {
        return Err("No usable vocabulary rules in this file".into());
    }
    Ok(rules)
}

pub fn export_json(rules: &[VocabRule]) -> Result<String, String> {
    serde_json::to_string_pretty(&serde_json::json!({
        "kind": "claudy-vocabulary",
        "version": 1,
        "rules": rules,
    }))
    .map_err(|e| e.to_string())
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[tauri::command]
pub fn import_vocabulary(app: AppHandle, path: String) -> Result<ImportReport, String> {
    let raw = std::fs::read_to_string(&path).map_err(|e| format!("Could not read {path}: {e}"))?;
    let incoming = parse_import(&raw)?;
    // The other settings writer besides `update_settings`; same lock.
    let _guard = crate::config::lock_store();
    let mut settings = crate::config::load(&app)?;
    let (rules, report) = merge(&settings.vocabulary.rules, incoming);
    settings.vocabulary.rules = rules;
    crate::config::save(&app, &settings)?;
    Ok(report)
}

#[tauri::command]
pub fn export_vocabulary(app: AppHandle, path: String) -> Result<usize, String> {
    let rules = crate::config::load(&app)?.vocabulary.rules;
    std::fs::write(&path, export_json(&rules)?)
        .map_err(|e| format!("Could not write {path}: {e}"))?;
    Ok(rules.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Terse rule builder — the tests are about matching, not construction.
    fn rule(from: &str, to: &str) -> VocabRule {
        VocabRule { id: from.into(), from: from.into(), to: to.into(), ..Default::default() }
    }

    fn run(rules: Vec<VocabRule>, text: &str) -> String {
        apply(&VocabularySettings { enabled: true, rules }, text)
    }

    #[test]
    fn an_empty_rule_list_returns_the_text_untouched() {
        assert_eq!(run(vec![], "hello there"), "hello there");
    }

    #[test]
    fn a_matching_rule_rewrites_the_word() {
        assert_eq!(run(vec![rule("get hub", "GitHub")], "push it to get hub"), "push it to GitHub");
    }

    #[test]
    fn replacement_never_fires_inside_a_longer_word() {
        // The whole reason boundaries exist: "category" must survive a
        // "cat" -> "dog" rule intact.
        assert_eq!(run(vec![rule("cat", "dog")], "the category of cat"), "the category of dog");
    }

    #[test]
    fn a_match_at_the_very_start_and_very_end_still_counts() {
        assert_eq!(run(vec![rule("cat", "dog")], "cat"), "dog");
        assert_eq!(run(vec![rule("cat", "dog")], "cat naps"), "dog naps");
        assert_eq!(run(vec![rule("cat", "dog")], "one cat"), "one dog");
    }

    #[test]
    fn punctuation_around_a_word_does_not_block_the_match() {
        assert_eq!(
            run(vec![rule("cloudy", "Claudy")], "(cloudy), \"cloudy\"; cloudy."),
            "(Claudy), \"Claudy\"; Claudy."
        );
    }

    #[test]
    fn an_apostrophe_protects_a_contraction() {
        // "we" must not fire inside "we're" — the apostrophe counts as part
        // of the word, so the standalone "we" later in the line is the only
        // match. Lowercase on purpose: this is about boundaries, not casing.
        assert_eq!(run(vec![rule("we", "they")], "we're sure we are"), "we're sure they are");
        // Same guard from the other side: "re" must not fire after one.
        assert_eq!(run(vec![rule("re", "regarding")], "we're here"), "we're here");
    }

    #[test]
    fn matching_is_case_insensitive_by_default() {
        let out = run(vec![rule("cloudy", "Claudy")], "Cloudy, CLOUDY and cloudy");
        assert_eq!(out, "Claudy, Claudy and Claudy");
    }

    #[test]
    fn a_lowercase_replacement_takes_the_casing_it_was_spoken_with() {
        let rules = vec![rule("teh", "the")];
        assert_eq!(run(rules.clone(), "teh thing"), "the thing");
        assert_eq!(run(rules.clone(), "Teh thing"), "The thing");
        assert_eq!(run(rules, "TEH thing"), "THE thing");
    }

    #[test]
    fn a_replacement_that_spells_its_own_capitals_is_never_re_cased() {
        // The single most important case-preservation rule: nobody wants
        // "GITHUB" or "Github" out of a rule they wrote as "GitHub".
        let rules = vec![rule("get hub", "GitHub"), rule("i phone", "iPhone")];
        assert_eq!(run(rules.clone(), "GET HUB and I PHONE"), "GitHub and iPhone");
        assert_eq!(run(rules.clone(), "Get hub and I phone"), "GitHub and iPhone");
        assert_eq!(run(rules, "get hub and i phone"), "GitHub and iPhone");
    }

    #[test]
    fn a_single_capital_letter_capitalizes_rather_than_shouts() {
        // "A" -> "a" at a sentence start is a capital, not an acronym.
        assert_eq!(run(vec![rule("A", "one")], "A dog"), "One dog");
    }

    #[test]
    fn match_case_makes_the_rule_literal_in_both_directions() {
        let r = VocabRule { match_case: true, ..rule("it", "IT") };
        // Only the exact-case occurrence is touched...
        assert_eq!(run(vec![r.clone()], "it is IT work"), "IT is IT work");
        // ...and the replacement is written exactly as typed, never re-cased.
        let r2 = VocabRule { match_case: true, ..rule("api", "API") };
        assert_eq!(run(vec![r2], "the api and the API"), "the API and the API");
    }

    #[test]
    fn the_longest_matching_pattern_wins_whatever_order_it_was_added_in() {
        let short_first = vec![rule("get hub", "GitHub"), rule("get hub actions", "GitHub Actions")];
        let long_first = vec![rule("get hub actions", "GitHub Actions"), rule("get hub", "GitHub")];
        for rules in [short_first, long_first] {
            assert_eq!(run(rules, "run get hub actions now"), "run GitHub Actions now");
        }
    }

    #[test]
    fn a_replacement_is_never_rewritten_by_another_rule() {
        // Without single-pass semantics this pair would loop forever.
        let rules = vec![rule("ping", "pong"), rule("pong", "ping")];
        assert_eq!(run(rules, "ping pong"), "pong ping");
    }

    #[test]
    fn an_empty_replacement_deletes_the_word_and_one_of_its_spaces() {
        assert_eq!(run(vec![rule("um", "")], "I um think so"), "I think so");
        assert_eq!(run(vec![rule("um", "")], "um I think"), "I think");
        assert_eq!(run(vec![rule("um", "")], "I think um"), "I think");
    }

    #[test]
    fn a_disabled_rule_does_nothing() {
        let r = VocabRule { enabled: false, ..rule("cat", "dog") };
        assert_eq!(run(vec![r], "one cat"), "one cat");
    }

    #[test]
    fn the_master_switch_turns_every_rule_off_without_deleting_it() {
        let settings = VocabularySettings { enabled: false, rules: vec![rule("cat", "dog")] };
        assert_eq!(apply(&settings, "one cat"), "one cat");
        assert_eq!(settings.rules.len(), 1, "the list must survive being switched off");
    }

    #[test]
    fn a_blank_pattern_can_never_match_anything() {
        // A zero-length pattern matching at every position would hang the
        // scan or explode the output; it must be filtered out entirely.
        for blank in ["", "   ", "\t\n"] {
            assert_eq!(run(vec![rule(blank, "X")], "hello"), "hello");
        }
    }

    #[test]
    fn a_rule_that_rewrites_a_word_to_itself_is_ignored() {
        assert_eq!(run(vec![rule("cat", "cat")], "a cat"), "a cat");
        assert_eq!(run(vec![rule("cat", " cat ")], "a cat"), "a cat");
    }

    #[test]
    fn surrounding_whitespace_in_a_rule_is_ignored() {
        assert_eq!(run(vec![rule("  cat  ", "  dog  ")], "a cat"), "a dog");
    }

    #[test]
    fn non_ascii_words_match_and_re_case_correctly() {
        assert_eq!(run(vec![rule("nguyen", "Nguyễn")], "hi nguyen"), "hi Nguyễn");
        assert_eq!(run(vec![rule("café", "coffee")], "a CAFÉ now"), "a COFFEE now");
    }

    #[test]
    fn a_pattern_with_no_word_characters_matches_mid_word() {
        // "--" has no side that could be cut off mid-word, so it may match
        // anywhere — that is what makes punctuation rules usable at all.
        assert_eq!(run(vec![rule("--", "—")], "a--b"), "a—b");
    }

    #[test]
    fn text_that_matches_nothing_keeps_its_original_whitespace() {
        // Only a deletion may trim; an untouched transcript is passed through
        // byte for byte.
        assert_eq!(run(vec![rule("cat", "dog")], "  spaced  "), "  spaced  ");
    }

    #[test]
    fn rules_beyond_the_cap_are_ignored_rather_than_failing() {
        let mut rules: Vec<VocabRule> = (0..MAX_RULES + 5)
            .map(|i| rule(&format!("word{i}"), &format!("W{i}")))
            .collect();
        rules.reverse(); // the survivors are the first MAX_RULES, whatever they are
        let out = run(rules, "word0 word1");
        assert!(!out.is_empty(), "an over-long list must still transcribe");
    }

    #[test]
    fn validate_accepts_a_normal_list_and_rejects_an_oversized_one() {
        let ok = VocabularySettings { enabled: true, rules: vec![rule("a b", "c")] };
        assert!(validate(&ok).is_ok());

        let too_many = VocabularySettings {
            enabled: true,
            rules: (0..MAX_RULES + 1).map(|i| rule(&format!("w{i}"), "x")).collect(),
        };
        assert!(validate(&too_many).unwrap_err().contains("Too many"));

        let too_long = VocabularySettings {
            enabled: true,
            rules: vec![rule(&"x".repeat(MAX_RULE_CHARS + 1), "y")],
        };
        assert!(validate(&too_long).unwrap_err().contains("longer than"));
    }

    #[test]
    fn settings_round_trip_camel_case_and_fill_missing_with_defaults() {
        let json = serde_json::json!({
            "enabled": true,
            "rules": [{ "id": "1", "from": "get hub", "to": "GitHub", "matchCase": true }]
        });
        let s: VocabularySettings = serde_json::from_value(json).unwrap();
        assert!(s.rules[0].match_case);
        assert!(s.rules[0].enabled, "a missing enabled flag defaults to on");

        let v = serde_json::to_value(&s).unwrap();
        assert!(v["rules"][0].get("matchCase").is_some());
        assert!(v["rules"][0].get("match_case").is_none());
    }

    #[test]
    fn merge_replaces_by_pattern_and_appends_the_rest() {
        let existing = vec![rule("cat", "dog"), rule("um", "")];
        let (merged, report) = merge(&existing, vec![rule("CAT", "lion"), rule("teh", "the")]);
        assert_eq!(merged.len(), 3, "a case-different pattern is the same rule");
        assert_eq!(merged[0].to, "lion");
        assert_eq!(merged[0].id, "cat", "the existing rule keeps its id");
        assert_eq!(merged[2].from, "teh");
        assert_eq!(report, ImportReport { added: 1, updated: 1, skipped: 0 });
    }

    #[test]
    fn merge_never_grows_past_the_cap_and_says_what_it_dropped() {
        let existing: Vec<VocabRule> =
            (0..MAX_RULES).map(|i| rule(&format!("w{i}"), "x")).collect();
        let (merged, report) = merge(&existing, vec![rule("brand new", "y")]);
        assert_eq!(merged.len(), MAX_RULES);
        assert_eq!(report.skipped, 1, "a dropped rule must be reported, not lost silently");
        assert_eq!(report.added, 0);
    }

    #[test]
    fn merge_can_still_update_an_existing_rule_when_the_list_is_full() {
        // An update replaces in place, so the cap is no reason to refuse it.
        let existing: Vec<VocabRule> =
            (0..MAX_RULES).map(|i| rule(&format!("w{i}"), "x")).collect();
        let (merged, report) = merge(&existing, vec![rule("w0", "updated")]);
        assert_eq!(merged[0].to, "updated");
        assert_eq!(report, ImportReport { added: 0, updated: 1, skipped: 0 });
    }

    #[test]
    fn import_accepts_both_a_bare_array_and_an_exported_file() {
        let exported = export_json(&[rule("get hub", "GitHub")]).unwrap();
        assert_eq!(parse_import(&exported).unwrap()[0].to, "GitHub");

        let bare = r#"[{ "from": "teh", "to": "the" }]"#;
        assert_eq!(parse_import(bare).unwrap()[0].from, "teh");
    }

    #[test]
    fn import_gives_every_rule_a_fresh_id() {
        // Two imports of the same file must not collide on React keys.
        let json = r#"[{ "id": "same", "from": "teh", "to": "the" }]"#;
        let a = parse_import(json).unwrap();
        let b = parse_import(json).unwrap();
        assert_ne!(a[0].id, "same");
        assert_ne!(a[0].id, b[0].id);
    }

    #[test]
    fn import_skips_broken_entries_instead_of_failing_the_file() {
        let json = r#"[
            { "from": "teh", "to": "the" },
            { "from": "   ", "to": "blank pattern" },
            "not an object",
            { "from": "ok", "to": "fine" }
        ]"#;
        let rules = parse_import(json).unwrap();
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[1].from, "ok");
    }

    #[test]
    fn import_reports_a_clear_reason_when_there_is_nothing_to_load() {
        assert!(parse_import("not json at all").unwrap_err().contains("valid JSON"));
        assert!(parse_import(r#"{"nope": 1}"#).unwrap_err().contains("rules"));
        assert!(parse_import("[]").unwrap_err().contains("No usable"));
        assert!(parse_import("42").unwrap_err().contains("Expected a list"));
    }

    #[test]
    fn a_realistic_rule_set_cleans_up_a_realistic_transcript() {
        let rules = vec![
            rule("cloudy", "Claudy"),
            rule("get hub", "GitHub"),
            rule("tow re", "Tauri"),
            rule("um", ""),
        ];
        let heard = "So um cloudy ships from get hub and it's built on tow re.";
        assert_eq!(run(rules, heard), "So Claudy ships from GitHub and it's built on Tauri.");
    }
}

