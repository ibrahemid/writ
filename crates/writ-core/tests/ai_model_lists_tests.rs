//! What the first model of a provider is, against the lists providers answer.
//!
//! The fixtures in `fixtures/ai-models/` are dated and sourced in that
//! folder's `SOURCES.md`: OpenRouter's is the response its public list gave,
//! the others are each provider's documented catalogue in the shape its list
//! endpoint answers. A seed taken from a sorted list sends the first request
//! to whatever sorts first, which on these lists is a completion-only model,
//! the most expensive model a provider sells, or an arbitrary vendor's.

use std::collections::HashMap;

use writ_core::ai::models::{
    filter_openai_ids, parse_anthropic_page, parse_model_list, seed_model, ListFamily, ModelCatalog,
};
use writ_core::ai::providers::provider;

const OPENAI: &str = include_str!("fixtures/ai-models/openai-models-2026-10-13.json");
const OPENAI_CHAT: &str =
    include_str!("fixtures/ai-models/openai-chat-completions-2026-10-13.json");
const ANTHROPIC: &str = include_str!("fixtures/ai-models/anthropic-models-2026-10-13.json");
const GEMINI: &str = include_str!("fixtures/ai-models/gemini-models-2026-10-13.json");
const OPENROUTER: &str = include_str!("fixtures/ai-models/openrouter-models-2026-10-13.json");
const TOGETHER: &str = include_str!("fixtures/ai-models/together-models-2026-10-13.json");

/// The ids each provider's list holds, read the way the app reads them.
fn listed(provider_id: &str) -> Vec<String> {
    match provider_id {
        "openai" => filter_openai_ids(
            parse_model_list(ListFamily::OpenAi, OPENAI).expect("the OpenAI list parses"),
        ),
        "anthropic" => {
            parse_anthropic_page(ANTHROPIC)
                .expect("the Anthropic list parses")
                .ids
        }
        "gemini" => parse_model_list(ListFamily::Gemini, GEMINI).expect("the Gemini list parses"),
        "openrouter" => {
            parse_model_list(ListFamily::OpenAi, OPENROUTER).expect("the OpenRouter list parses")
        }
        "together" => {
            parse_model_list(ListFamily::OpenAi, TOGETHER).expect("the Together list parses")
        }
        other => panic!("no fixture for {other}"),
    }
}

fn seed_from(provider_id: &str, ids: Vec<String>) -> String {
    seed_model(provider_id, Some(&ModelCatalog::live(provider_id, ids)))
}

/// Which OpenAI ids the documentation says answer `v1/chat/completions`.
fn openai_chat_support() -> HashMap<String, bool> {
    let parsed: serde_json::Value =
        serde_json::from_str(OPENAI_CHAT).expect("the support table parses");
    parsed["chat_completions"]
        .as_object()
        .expect("an object of ids")
        .iter()
        .map(|(id, supported)| (id.clone(), supported.as_bool().expect("a bool")))
        .collect()
}

#[test]
fn each_hosted_row_starts_from_its_own_default_when_the_list_holds_it() {
    for (provider_id, expected) in [
        ("openai", "gpt-6-luna"),
        ("anthropic", "claude-sonnet-5-5"),
        ("gemini", "gemini-3.8-flash"),
        ("openrouter", "meta-llama/llama-3.3-70b-instruct"),
        ("together", "openai/gpt-oss-120b"),
    ] {
        let row = provider(provider_id).expect("a table row");
        assert_eq!(row.default_model, expected, "{provider_id}'s default moved");
        let ids = listed(provider_id);
        assert!(
            ids.iter().any(|id| id == expected),
            "{provider_id}'s list no longer holds {expected}"
        );
        assert_eq!(seed_from(provider_id, ids), expected, "{provider_id}");
    }
}

#[test]
fn the_seed_is_never_what_the_list_sorts_first() {
    // On every recorded list the first id after sorting is a model nobody
    // chose: a completion-only OpenAI id, the dearest Claude, an arbitrary
    // OpenRouter vendor's. The seed must not depend on the order at all.
    for provider_id in ["openai", "anthropic", "gemini", "openrouter", "together"] {
        let ids = listed(provider_id);
        let first = ids.first().expect("a non-empty list").clone();
        let seed = seed_from(provider_id, ids);
        assert_ne!(seed, first, "{provider_id} seeded the first sorted id");
    }
}

#[test]
fn without_its_default_a_row_takes_the_first_suggestion_the_list_holds() {
    let without_default: Vec<String> = listed("openai")
        .into_iter()
        .filter(|id| id != "gpt-6-luna")
        .collect();
    assert_eq!(seed_from("openai", without_default), "gpt-6.1-sol");

    let anthropic_without_default: Vec<String> = listed("anthropic")
        .into_iter()
        .filter(|id| id != "claude-sonnet-5-5")
        .collect();
    assert_eq!(
        seed_from("anthropic", anthropic_without_default),
        "claude-haiku-5-5"
    );
}

#[test]
fn a_list_holding_none_of_the_rows_ids_seeds_nothing() {
    // The picker opens instead: an id the provider listed but nobody chose is
    // a request sent to a model for a reason nobody gave.
    for provider_id in ["openai", "anthropic", "gemini", "openrouter", "together"] {
        let row = provider(provider_id).expect("a table row");
        let foreign: Vec<String> = listed(provider_id)
            .into_iter()
            .filter(|id| id != row.default_model && !row.curated_models.contains(&id.as_str()))
            .collect();
        assert!(!foreign.is_empty(), "{provider_id}");
        assert_eq!(seed_from(provider_id, foreign), "", "{provider_id}");
    }
}

#[test]
fn every_seed_is_a_model_that_answers_chat_completions() {
    let support = openai_chat_support();
    let seed = seed_from("openai", listed("openai"));
    assert_eq!(support.get(&seed), Some(&true), "{seed}");
}

#[test]
fn the_openai_list_keeps_exactly_what_the_documentation_says_answers_chat() {
    let support = openai_chat_support();
    let all = parse_model_list(ListFamily::OpenAi, OPENAI).expect("the OpenAI list parses");
    assert_eq!(
        all.len(),
        support.len(),
        "the two fixtures name the same ids"
    );
    let kept = filter_openai_ids(all.clone());
    for id in &all {
        let supported = support.get(id).copied().expect("every id has an answer");
        assert_eq!(
            kept.contains(id),
            supported,
            "{id}: the documentation says chat/completions is {}",
            if supported {
                "supported"
            } else {
                "not supported"
            }
        );
    }
    for completion_only in [
        "babbage-002",
        "davinci-002",
        "computer-use-preview",
        "o1-pro",
    ] {
        assert!(
            !kept.iter().any(|id| id == completion_only),
            "{completion_only}"
        );
    }
}

#[test]
fn a_model_picked_by_hand_from_the_current_family_stays_on_offer() {
    let ids = listed("openai");
    for id in ["gpt-5-mini", "gpt-5", "gpt-5-mini-2025-08-07"] {
        assert!(ids.iter().any(|listed| listed == id), "{id}");
    }
}

#[test]
fn a_fine_tuned_chat_model_stays_on_offer() {
    let kept = filter_openai_ids(vec![
        "ft:gpt-4o-mini-2024-07-18:acme:pro-notes:AbC123".to_string(),
        "ft:davinci-002:acme::XyZ".to_string(),
    ]);
    assert_eq!(
        kept,
        vec!["ft:gpt-4o-mini-2024-07-18:acme:pro-notes:AbC123".to_string()]
    );
}

#[test]
fn a_model_offered_only_to_approved_organizations_stays_out_of_the_picker() {
    // OpenAI's catalogue, read on 2026-10-13, offers GPT-Rosalind
    // (`gpt-rosalind-research`) to approved organizations, with no model page
    // to say what it answers.
    let kept = filter_openai_ids(
        ["gpt-rosalind", "gpt-rosalind-research", "gpt-6-luna"]
            .map(String::from)
            .to_vec(),
    );
    assert_eq!(kept, vec!["gpt-6-luna".to_string()]);
}

#[test]
fn togethers_bare_list_is_read_and_holds_only_text_models() {
    let ids = listed("together");
    assert!(ids.iter().any(|id| id == "openai/gpt-oss-120b"));
    for not_text in [
        "black-forest-labs/FLUX.2-pro",
        "openai/gpt-image-2",
        "minimax/hailuo-02",
    ] {
        assert!(!ids.iter().any(|id| id == not_text), "{not_text}");
    }
}

#[test]
fn the_gemini_list_offers_only_what_generates_content() {
    let ids = listed("gemini");
    assert!(ids.iter().any(|id| id == "gemini-3.8-flash"));
    for elsewhere in ["gemini-3.8-live", "gemini-embedding-001"] {
        assert!(!ids.iter().any(|id| id == elsewhere), "{elsewhere}");
    }
}
