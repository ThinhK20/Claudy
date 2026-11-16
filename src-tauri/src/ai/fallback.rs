//! Model fallback: keep working when the configured model is overloaded.
//!
//! At peak hours a provider answers `503 UNAVAILABLE` / `529 overloaded_error`
//! instead of a completion. The model is the thing that is saturated, not the
//! account, so the same request usually succeeds against a sibling model. This
//! module owns the two pieces of state that make that automatic:
//!
//! * a per-provider **chain** of models to try, built in but user-overridable;
//! * a **cooldown** registry, so a model that just failed is skipped for a few
//!   minutes instead of costing every later request a failed round-trip.
//!
//! Deliberately free of `AppHandle` and HTTP: everything here is a pure
//! function over strings plus one in-memory map, so it is unit-testable.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// How long an overloaded model is skipped before we prefer it again. Long
/// enough to ride out a peak-hour spike, short enough that a user who picked
/// an expensive model is not quietly kept off it for the rest of the session.
pub const COOLDOWN_SECS: u64 = 300;

/// The models tried after the configured one, best first.
///
/// Only the two providers with a stable, published model catalogue get a
/// built-in chain. `openai_compatible` points at an arbitrary gateway or local
/// server and `ollama` at whatever the user has pulled, so guessing ids there
/// would trade a 503 for a 404 — those two fall back only when configured.
pub fn builtin_chain(provider_id: &str) -> &'static [&'static str] {
    match provider_id {
        "gemini" => &["gemini-2.5-flash", "gemini-2.5-flash-lite", "gemini-2.0-flash"],
        "anthropic" => &["claude-sonnet-5", "claude-haiku-4-5-20251001"],
        "deepseek" => &["deepseek-flash", "deepseek-v4-pro"],
        _ => &[],
    }
}

/// `(provider_id, model) -> the instant its cooldown expires`.
fn registry() -> &'static Mutex<HashMap<(String, String), Instant>> {
    static REGISTRY: OnceLock<Mutex<HashMap<(String, String), Instant>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

fn key(provider_id: &str, model: &str) -> (String, String) {
    (provider_id.to_string(), model.to_string())
}

/// Is this model still in its cooldown window? Expired entries are dropped on
/// read, so the map never grows past the models actually in use.
pub fn is_cooling(provider_id: &str, model: &str) -> bool {
    // A poisoned mutex must never fail a completion — treat it as "not
    // cooling" and let the request take its chances with the primary.
    let Ok(mut map) = registry().lock() else {
        return false;
    };
    let k = key(provider_id, model);
    match map.get(&k) {
        Some(until) if *until > Instant::now() => true,
        Some(_) => {
            map.remove(&k);
            false
        }
        None => false,
    }
}

/// Start (or extend) a cooldown. Returns `true` only when this STARTED one —
/// the caller uses that to notify the user once per outage rather than once
/// per request.
pub fn mark_overloaded(provider_id: &str, model: &str) -> bool {
    mark_overloaded_for(provider_id, model, Duration::from_secs(COOLDOWN_SECS))
}

/// `mark_overloaded` with an explicit window, so tests can watch one expire
/// without waiting five minutes or injecting a clock.
fn mark_overloaded_for(provider_id: &str, model: &str, window: Duration) -> bool {
    let was_cooling = is_cooling(provider_id, model);
    if let Ok(mut map) = registry().lock() {
        map.insert(key(provider_id, model), Instant::now() + window);
    }
    !was_cooling
}

/// The model answered, so it is healthy again — drop any cooldown early.
pub fn clear(provider_id: &str, model: &str) {
    if let Ok(mut map) = registry().lock() {
        map.remove(&key(provider_id, model));
    }
}

/// The models to try, best first: the configured model, then the chain.
///
/// Deduped, and a model currently in cooldown is moved to the BACK rather than
/// dropped — with every model cooling we must still send something, so the
/// list may never come back empty.
pub fn candidates(
    provider_id: &str,
    configured: &str,
    overrides: &[String],
    default_model: &str,
) -> Vec<String> {
    let primary = super::or_default(configured, default_model).to_string();

    // An explicit list is the user's intent, so it REPLACES the built-in chain
    // rather than extending it — otherwise there would be no way to shorten one.
    let chain: Vec<String> = if overrides.iter().any(|m| !m.trim().is_empty()) {
        overrides.iter().map(|m| m.trim().to_string()).filter(|m| !m.is_empty()).collect()
    } else {
        builtin_chain(provider_id).iter().map(|m| (*m).to_string()).collect()
    };

    let mut ordered: Vec<String> = Vec::with_capacity(1 + chain.len());
    for model in std::iter::once(primary).chain(chain) {
        if !ordered.contains(&model) {
            ordered.push(model);
        }
    }

    // Stable partition: healthy models keep their order, cooling ones keep
    // theirs behind them.
    let (ready, cooling): (Vec<String>, Vec<String>) =
        ordered.into_iter().partition(|m| !is_cooling(provider_id, m));
    ready.into_iter().chain(cooling).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owned(models: &[&str]) -> Vec<String> {
        models.iter().map(|m| (*m).to_string()).collect()
    }

    // The registry is process-global and tests run in parallel, so every test
    // that touches it uses a provider id of its own.

    #[test]
    fn builtin_chains_exist_only_where_model_ids_are_predictable() {
        assert_eq!(builtin_chain("gemini")[0], "gemini-2.5-flash");
        assert_eq!(builtin_chain("anthropic")[0], "claude-sonnet-5");
        // A gateway or a local ollama could serve anything — never guess.
        assert!(builtin_chain("openai_compatible").is_empty());
        assert!(builtin_chain("ollama").is_empty());
        assert!(builtin_chain("skynet").is_empty());
    }

    #[test]
    fn deepseek_flash_and_v4_pro_back_each_other_up() {
        assert_eq!(
            candidates("deepseek", "", &[], "deepseek-flash"),
            owned(&["deepseek-flash", "deepseek-v4-pro"])
        );
        assert_eq!(
            candidates("deepseek", "deepseek-v4-pro", &[], "deepseek-flash"),
            owned(&["deepseek-v4-pro", "deepseek-flash"])
        );
    }

    #[test]
    fn candidates_start_with_the_configured_model_and_never_repeat_it() {
        let c = candidates("gemini", "gemini-2.5-flash", &[], "gemini-2.5-flash");
        assert_eq!(c[0], "gemini-2.5-flash");
        assert_eq!(c.iter().filter(|m| *m == "gemini-2.5-flash").count(), 1);
        assert_eq!(c.len(), builtin_chain("gemini").len(), "the chain gained a duplicate");
    }

    #[test]
    fn an_empty_configured_model_resolves_to_the_provider_default() {
        let c = candidates("gemini", "   ", &[], "gemini-2.5-pro");
        assert_eq!(c[0], "gemini-2.5-pro");
    }

    #[test]
    fn an_override_list_replaces_the_builtin_chain() {
        let c = candidates("gemini", "gemini-2.5-pro", &owned(&["a", " ", " b "]), "d");
        assert_eq!(c, owned(&["gemini-2.5-pro", "a", "b"]), "blank entries must be dropped");
    }

    #[test]
    fn a_provider_without_a_chain_gets_exactly_one_candidate() {
        let c = candidates("ollama", "llama3.2", &[], "llama3.2");
        assert_eq!(c, owned(&["llama3.2"]), "no chain means the single-shot behavior of old");
    }

    #[test]
    fn a_cooling_model_moves_to_the_back_but_is_never_dropped() {
        let p = "test-reorder";
        assert!(mark_overloaded(p, "hot"));
        assert_eq!(
            candidates(p, "hot", &owned(&["warm", "spare"]), "hot"),
            owned(&["warm", "spare", "hot"])
        );

        // Everything cooling: order is preserved and the list still stands.
        mark_overloaded(p, "warm");
        mark_overloaded(p, "spare");
        assert_eq!(
            candidates(p, "hot", &owned(&["warm", "spare"]), "hot"),
            owned(&["hot", "warm", "spare"]),
            "with nothing healthy we must still send the preferred model"
        );
    }

    #[test]
    fn marking_reports_only_the_transition_into_cooldown() {
        let p = "test-transition";
        assert!(mark_overloaded(p, "m"), "the first failure is news");
        assert!(!mark_overloaded(p, "m"), "a repeat must not re-notify the user");
        clear(p, "m");
        assert!(!is_cooling(p, "m"));
        assert!(mark_overloaded(p, "m"), "after recovery it is news again");
        clear(p, "m");
    }

    #[test]
    fn a_cooldown_expires_on_its_own() {
        let p = "test-expiry";
        assert!(mark_overloaded_for(p, "m", Duration::from_millis(30)));
        assert!(is_cooling(p, "m"));
        std::thread::sleep(Duration::from_millis(60));
        assert!(!is_cooling(p, "m"), "the model must be preferred again after the window");
        assert!(mark_overloaded(p, "m"), "an expired entry is a fresh transition");
        clear(p, "m");
    }

    #[test]
    fn cooldowns_are_scoped_to_one_provider() {
        // Two providers can legitimately serve models with the same name (a
        // gateway proxying gemini, say) — one being down says nothing about
        // the other.
        assert!(mark_overloaded("test-scope-a", "shared"));
        assert!(!is_cooling("test-scope-b", "shared"));
        clear("test-scope-a", "shared");
    }
}
