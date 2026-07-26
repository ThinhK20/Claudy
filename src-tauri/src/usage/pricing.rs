//! Model pricing for the usage log's cost estimate.
//!
//! Pure: no `AppHandle`, no IO, no clock. Everything here is a lookup or a
//! multiplication, so the whole module is unit-testable.
//!
//! The bundled table is a *starting point*, not a source of truth — providers
//! change prices and Claudy has no way to notice. `PRICES_AS_OF` is shown next
//! to every cost figure in the UI so a stale table is visible rather than
//! silently wrong, and any model can be overridden from the Usage page.

use serde::{Deserialize, Serialize};

/// When the bundled table was last checked against the providers' pricing
/// pages. Surfaced in the UI — bump it whenever `BUILTIN` changes.
pub const PRICES_AS_OF: &str = "2026-07";

/// Prices are quoted per million tokens, the industry convention.
pub const TOKENS_PER_UNIT: f64 = 1_000_000.0;

/// A user-supplied price, persisted in `settings.json` under `usage`.
/// Overrides the bundled table for one model.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct ModelPrice {
    /// Model id as the provider reports it. Matched case-insensitively.
    pub model: String,
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
}

/// Resolved USD-per-million-token rates for one model.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Price {
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
}

/// Local models cost nothing to run — electricity isn't something Claudy can
/// price, and pretending otherwise would be worse than showing $0.
pub const FREE: Price = Price { input_per_mtok: 0.0, output_per_mtok: 0.0 };

/// `(model id or prefix, input $/Mtok, output $/Mtok)`, list prices as of
/// `PRICES_AS_OF`. Entries double as prefixes so dated ids
/// (`claude-sonnet-5-20260514`) resolve without an entry of their own.
///
/// Excludes batch, cache-write/read, and long-context tiers: Claudy only ever
/// makes plain single-shot requests, so the base rate is the right one.
const BUILTIN: &[(&str, f64, f64)] = &[
    // Anthropic
    ("claude-fable-5", 10.00, 50.00),
    ("claude-mythos-5", 10.00, 50.00),
    ("claude-opus-5", 5.00, 25.00),
    ("claude-opus-4", 5.00, 25.00),
    ("claude-sonnet-5", 3.00, 15.00),
    ("claude-sonnet-4", 3.00, 15.00),
    ("claude-haiku-4-5", 1.00, 5.00),
    // OpenAI
    ("gpt-4o-mini", 0.15, 0.60),
    ("gpt-4o", 2.50, 10.00),
    ("gpt-4.1-mini", 0.40, 1.60),
    ("gpt-4.1-nano", 0.10, 0.40),
    ("gpt-4.1", 2.00, 8.00),
    ("o4-mini", 1.10, 4.40),
    // Google Gemini
    ("gemini-2.5-flash-lite", 0.10, 0.40),
    ("gemini-2.5-flash", 0.30, 2.50),
    ("gemini-2.5-pro", 1.25, 10.00),
    ("gemini-2.0-flash", 0.10, 0.40),
];

/// Longest-prefix match over `(id, value)` rows.
fn longest_prefix<'a, T>(key: &str, rows: impl Iterator<Item = (&'a str, T)>) -> Option<T> {
    rows.filter(|(id, _)| key.starts_with(*id))
        .max_by_key(|(id, _)| id.len())
        .map(|(_, v)| v)
}

/// USD rates for one run, or `None` when nothing knows this model.
///
/// Order: local provider → exact override → exact builtin → longest-prefix
/// override → longest-prefix builtin. `None` means "tokens counted, cost
/// unknown" — the summary flags it and the UI offers to set a price. Guessing
/// would be worse than an honest gap.
pub fn resolve(provider: &str, model: &str, overrides: &[ModelPrice]) -> Option<Price> {
    if provider == "ollama" {
        return Some(FREE);
    }
    let key = model.trim().to_ascii_lowercase();
    if key.is_empty() {
        return None;
    }

    let as_price = |p: &ModelPrice| Price {
        input_per_mtok: p.input_per_mtok,
        output_per_mtok: p.output_per_mtok,
    };
    let usable: Vec<(String, &ModelPrice)> = overrides
        .iter()
        .filter(|p| is_usable(p))
        .map(|p| (p.model.trim().to_ascii_lowercase(), p))
        .collect();

    if let Some((_, p)) = usable.iter().find(|(id, _)| *id == key) {
        return Some(as_price(p));
    }
    if let Some((_, i, o)) = BUILTIN.iter().find(|(id, _, _)| *id == key) {
        return Some(Price { input_per_mtok: *i, output_per_mtok: *o });
    }
    if let Some(p) = longest_prefix(&key, usable.iter().map(|(id, p)| (id.as_str(), *p))) {
        return Some(as_price(p));
    }
    longest_prefix(&key, BUILTIN.iter().map(|(id, i, o)| (*id, (*i, *o))))
        .map(|(i, o)| Price { input_per_mtok: i, output_per_mtok: o })
}

/// A hand-edited or half-typed override must not poison the whole estimate:
/// a negative, NaN, or nameless price is ignored in favor of the builtin.
pub fn is_usable(p: &ModelPrice) -> bool {
    !p.model.trim().is_empty()
        && p.input_per_mtok.is_finite()
        && p.output_per_mtok.is_finite()
        && p.input_per_mtok >= 0.0
        && p.output_per_mtok >= 0.0
}

/// Cost of one run in USD.
pub fn cost_usd(price: Price, input_tokens: u32, output_tokens: u32) -> f64 {
    (f64::from(input_tokens) * price.input_per_mtok
        + f64::from(output_tokens) * price.output_per_mtok)
        / TOKENS_PER_UNIT
}

/// Every model in the bundled table, for the Pricing editor's suggestions.
pub fn builtin_models() -> Vec<ModelPrice> {
    BUILTIN
        .iter()
        .map(|(model, i, o)| ModelPrice {
            model: (*model).to_string(),
            input_per_mtok: *i,
            output_per_mtok: *o,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn price(model: &str, i: f64, o: f64) -> ModelPrice {
        ModelPrice { model: model.into(), input_per_mtok: i, output_per_mtok: o }
    }

    #[test]
    fn ollama_is_always_free_even_for_an_unknown_model() {
        // The model runs on this machine; there is no bill to estimate.
        let p = resolve("ollama", "some-random-local-model", &[]).unwrap();
        assert_eq!(p, FREE);
        assert_eq!(cost_usd(p, 100_000, 100_000), 0.0);
    }

    #[test]
    fn exact_builtin_lookup_is_case_insensitive() {
        let p = resolve("anthropic", "Claude-Sonnet-5", &[]).unwrap();
        assert_eq!(p.input_per_mtok, 3.00);
        assert_eq!(p.output_per_mtok, 15.00);
    }

    #[test]
    fn a_dated_model_id_resolves_by_longest_prefix() {
        // Providers append snapshot dates; the base entry must still price it.
        let dated = resolve("anthropic", "claude-sonnet-5-20260514", &[]).unwrap();
        assert_eq!(dated, resolve("anthropic", "claude-sonnet-5", &[]).unwrap());
    }

    #[test]
    fn longest_prefix_wins_over_a_shorter_one() {
        // "claude-haiku-4-5" and "claude-opus-4" both exist; a haiku id must
        // not fall back to a shorter, pricier entry.
        let haiku = resolve("anthropic", "claude-haiku-4-5-20251001", &[]).unwrap();
        assert_eq!(haiku.input_per_mtok, 1.00);
        // Same shape on the OpenAI side: mini is cheaper than the base model.
        let mini = resolve("openai_compatible", "gpt-4o-mini-2026-01-01", &[]).unwrap();
        assert_eq!(mini.input_per_mtok, 0.15);
    }

    #[test]
    fn an_override_beats_the_builtin_table() {
        let overrides = vec![price("claude-sonnet-5", 1.0, 2.0)];
        let p = resolve("anthropic", "claude-sonnet-5", &overrides).unwrap();
        assert_eq!(p.input_per_mtok, 1.0);
        assert_eq!(p.output_per_mtok, 2.0);
    }

    #[test]
    fn an_override_can_price_a_model_the_table_has_never_heard_of() {
        let overrides = vec![price("my-private-deployment", 7.0, 9.0)];
        assert!(resolve("openai_compatible", "my-private-deployment", &[]).is_none());
        let p = resolve("openai_compatible", "my-private-deployment", &overrides).unwrap();
        assert_eq!(p.output_per_mtok, 9.0);
    }

    #[test]
    fn an_exact_builtin_beats_a_prefix_override() {
        // "gpt-4" would prefix-match, but an exact table hit is more specific.
        let overrides = vec![price("gpt-4", 99.0, 99.0)];
        let p = resolve("openai_compatible", "gpt-4o", &overrides).unwrap();
        assert_eq!(p.input_per_mtok, 2.50, "exact builtin should win");
    }

    #[test]
    fn unknown_models_have_no_price_rather_than_a_guessed_one() {
        assert!(resolve("openai_compatible", "mystery-model-9000", &[]).is_none());
        assert!(resolve("anthropic", "", &[]).is_none(), "empty model can't be priced");
        assert!(resolve("anthropic", "   ", &[]).is_none());
    }

    #[test]
    fn a_broken_override_is_ignored_instead_of_poisoning_the_estimate() {
        // settings.json is hand-editable and the editor accepts partial input.
        for bad in [
            price("claude-sonnet-5", -1.0, 5.0),
            price("claude-sonnet-5", f64::NAN, 5.0),
            price("claude-sonnet-5", 1.0, f64::INFINITY),
            price("   ", 1.0, 2.0),
        ] {
            let p = resolve("anthropic", "claude-sonnet-5", std::slice::from_ref(&bad)).unwrap();
            assert_eq!(
                p.input_per_mtok, 3.00,
                "bad override {bad:?} should fall back to the builtin"
            );
        }
    }

    #[test]
    fn cost_is_input_and_output_priced_separately_per_million_tokens() {
        let p = Price { input_per_mtok: 3.0, output_per_mtok: 15.0 };
        // 1M in + 1M out = $3 + $15
        assert!((cost_usd(p, 1_000_000, 1_000_000) - 18.0).abs() < 1e-9);
        // A realistic single prompt run is fractions of a cent.
        let one_run = cost_usd(p, 412, 96);
        assert!((one_run - 0.002676).abs() < 1e-9, "got {one_run}");
        assert_eq!(cost_usd(p, 0, 0), 0.0);
    }

    #[test]
    fn every_builtin_entry_is_lowercase_and_usable() {
        // Lookup lowercases the key, so an uppercase entry would be unreachable.
        for p in builtin_models() {
            assert_eq!(p.model, p.model.to_ascii_lowercase(), "{} must be lowercase", p.model);
            assert!(is_usable(&p), "{} has an unusable price", p.model);
            assert!(
                p.output_per_mtok >= p.input_per_mtok,
                "{}: output is normally at least input",
                p.model
            );
        }
    }

    #[test]
    fn the_default_model_of_every_provider_has_a_price() {
        // A fresh install shows a real cost estimate rather than "no price".
        for id in crate::config::PROVIDER_IDS {
            let p = crate::ai::provider(id).unwrap();
            assert!(
                resolve(id, p.default_model(), &[]).is_some(),
                "{id}'s default model {} is unpriced",
                p.default_model()
            );
        }
    }
}
