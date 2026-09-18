//! The provider table, as data.
//!
//! One row per endpoint Writ can be pointed at, carrying everything the rest
//! of the app asks about a provider: where requests go, where its model list
//! is, which wire it speaks, where a key is obtained, and what model to start
//! from. The table is serialised over IPC so the settings dropdown and the
//! Rust side never disagree about a row.

use serde::{Deserialize, Serialize};

/// Where a provider runs, which decides what the UI asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderGroup {
    /// On this machine, over loopback. No key, and a port that can be probed.
    Local,
    /// A remote service. Needs a key, and needs host consent before a send.
    Hosted,
    /// An OpenAI-compatible endpoint the person typed themselves.
    Custom,
}

/// Which request and stream format an endpoint speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Wire {
    /// `POST /chat/completions` with `delta.content` frames.
    #[serde(rename = "openai")]
    OpenAi,
    /// The Anthropic Messages API: `POST /v1/messages`, `content_block_delta`
    /// frames.
    #[serde(rename = "anthropic")]
    Anthropic,
}

/// One row of the provider table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ProviderInfo {
    /// The id stored in `config.toml` and used as the keychain account.
    pub id: &'static str,
    /// The name shown in the dropdown.
    pub label: &'static str,
    /// Local, hosted, or typed by hand.
    pub group: ProviderGroup,
    /// The wire format requests are written in.
    pub wire: Wire,
    /// The API base requests are built on, ending before `/chat/completions`.
    /// Empty for `custom`, whose base URL is typed.
    pub base_url: &'static str,
    /// Absolute URL of the model list. Empty for `custom`, where it is
    /// derived as `{base_url}/models` by [`models_url_for`].
    pub models_url: &'static str,
    /// Where a person obtains a key, for the "Get a key" link.
    pub key_page_url: Option<&'static str>,
    /// The model id shown first among the suggestions, when the provider's
    /// own list cannot be read. Empty for the local rows, which answer with
    /// what is installed. Never written to `config.toml`: the first model of
    /// the provider's own list is
    /// (`writ_core::ai::models::seed_model`).
    pub default_model: &'static str,
    /// Whether a request needs a key. False for local and for custom.
    pub needs_key: bool,
    /// Whether the provider can hand over a key without one being pasted.
    pub supports_connect: bool,
    /// The loopback port the probe knocks on, for local rows.
    pub probe_port: Option<u16>,
    /// Model ids offered when the provider's own list cannot be read.
    /// Suggestions, not an inventory: the account may not carry them, and a
    /// suggestion is never seeded into the file for that reason. First entry is
    /// the one shown first. Empty for the rows whose list is whatever the
    /// runtime or the typed endpoint holds.
    ///
    /// These are read by hand from each provider's list and go stale when a
    /// provider retires an id. Refreshed rows carry the date they were read;
    /// the rest were written for 0.6 and have not been checked against a live
    /// list since.
    pub curated_models: &'static [&'static str],
}

/// Every provider Writ knows, in the order the dropdown shows them.
pub const PROVIDERS: &[ProviderInfo] = &[
    ProviderInfo {
        id: "ollama",
        label: "Ollama",
        group: ProviderGroup::Local,
        wire: Wire::OpenAi,
        base_url: "http://localhost:11434/v1",
        models_url: "http://localhost:11434/api/tags",
        key_page_url: None,
        default_model: "",
        needs_key: false,
        supports_connect: false,
        probe_port: Some(11434),
        curated_models: &["qwen3:4b", "qwen3:8b", "gemma3:4b", "llama3.2:3b"],
    },
    ProviderInfo {
        id: "lmstudio",
        label: "LM Studio",
        group: ProviderGroup::Local,
        wire: Wire::OpenAi,
        base_url: "http://localhost:1234/v1",
        models_url: "http://localhost:1234/v1/models",
        key_page_url: None,
        default_model: "",
        needs_key: false,
        supports_connect: false,
        probe_port: Some(1234),
        curated_models: &[],
    },
    ProviderInfo {
        id: "anthropic",
        label: "Anthropic",
        group: ProviderGroup::Hosted,
        wire: Wire::Anthropic,
        base_url: "https://api.anthropic.com",
        models_url: "https://api.anthropic.com/v1/models",
        key_page_url: Some("https://console.anthropic.com/settings/keys"),
        default_model: "claude-sonnet-5",
        needs_key: true,
        supports_connect: false,
        probe_port: None,
        curated_models: &["claude-sonnet-5", "claude-opus-5", "claude-haiku-4-5"],
    },
    ProviderInfo {
        id: "openai",
        label: "OpenAI",
        group: ProviderGroup::Hosted,
        wire: Wire::OpenAi,
        base_url: "https://api.openai.com/v1",
        models_url: "https://api.openai.com/v1/models",
        key_page_url: Some("https://platform.openai.com/api-keys"),
        default_model: "gpt-5-mini",
        needs_key: true,
        supports_connect: false,
        probe_port: None,
        curated_models: &["gpt-5-mini", "gpt-5", "gpt-4.1-mini"],
    },
    ProviderInfo {
        id: "gemini",
        label: "Google Gemini",
        group: ProviderGroup::Hosted,
        wire: Wire::OpenAi,
        base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
        models_url: "https://generativelanguage.googleapis.com/v1beta/models",
        key_page_url: Some("https://aistudio.google.com/apikey"),
        default_model: "gemini-2.5-flash",
        needs_key: true,
        supports_connect: false,
        probe_port: None,
        curated_models: &["gemini-2.5-flash", "gemini-2.5-flash-lite"],
    },
    ProviderInfo {
        id: "openrouter",
        label: "OpenRouter",
        group: ProviderGroup::Hosted,
        wire: Wire::OpenAi,
        base_url: "https://openrouter.ai/api/v1",
        models_url: "https://openrouter.ai/api/v1/models",
        key_page_url: Some("https://openrouter.ai/settings/keys"),
        default_model: "meta-llama/llama-3.3-70b-instruct",
        needs_key: true,
        supports_connect: true,
        probe_port: None,
        curated_models: &[
            "meta-llama/llama-3.3-70b-instruct",
            "openai/gpt-5-mini",
            "google/gemma-4-26b-a4b-it:free",
        ],
    },
    ProviderInfo {
        id: "groq",
        label: "Groq",
        group: ProviderGroup::Hosted,
        wire: Wire::OpenAi,
        base_url: "https://api.groq.com/openai/v1",
        models_url: "https://api.groq.com/openai/v1/models",
        key_page_url: Some("https://console.groq.com/keys"),
        default_model: "llama-3.3-70b-versatile",
        needs_key: true,
        supports_connect: false,
        probe_port: None,
        curated_models: &["llama-3.3-70b-versatile", "llama-3.1-8b-instant"],
    },
    ProviderInfo {
        id: "deepseek",
        label: "DeepSeek",
        group: ProviderGroup::Hosted,
        wire: Wire::OpenAi,
        base_url: "https://api.deepseek.com",
        models_url: "https://api.deepseek.com/models",
        key_page_url: Some("https://platform.deepseek.com/api_keys"),
        // Read from the live list on 2026-09-18. `deepseek-chat` and
        // `deepseek-reasoner` are gone from it; the endpoint still answers a
        // request for `deepseek-reasoner` as an alias, which is why a stale
        // suggestion is not a safe thing to write into the file.
        default_model: "deepseek-flash",
        needs_key: true,
        supports_connect: false,
        probe_port: None,
        curated_models: &["deepseek-flash", "deepseek-v4-pro"],
    },
    ProviderInfo {
        id: "mistral",
        label: "Mistral",
        group: ProviderGroup::Hosted,
        wire: Wire::OpenAi,
        base_url: "https://api.mistral.ai/v1",
        models_url: "https://api.mistral.ai/v1/models",
        key_page_url: Some("https://console.mistral.ai/api-keys"),
        default_model: "mistral-small-latest",
        needs_key: true,
        supports_connect: false,
        probe_port: None,
        curated_models: &["mistral-small-latest", "mistral-large-latest"],
    },
    ProviderInfo {
        id: "xai",
        label: "xAI",
        group: ProviderGroup::Hosted,
        wire: Wire::OpenAi,
        base_url: "https://api.x.ai/v1",
        models_url: "https://api.x.ai/v1/models",
        key_page_url: Some("https://console.x.ai"),
        default_model: "grok-4",
        needs_key: true,
        supports_connect: false,
        probe_port: None,
        curated_models: &["grok-4", "grok-3-mini"],
    },
    ProviderInfo {
        id: "together",
        label: "Together",
        group: ProviderGroup::Hosted,
        wire: Wire::OpenAi,
        base_url: "https://api.together.xyz/v1",
        models_url: "https://api.together.xyz/v1/models",
        key_page_url: Some("https://api.together.ai/settings/api-keys"),
        default_model: "meta-llama/Llama-3.3-70B-Instruct-Turbo",
        needs_key: true,
        supports_connect: false,
        probe_port: None,
        curated_models: &["meta-llama/Llama-3.3-70B-Instruct-Turbo"],
    },
    ProviderInfo {
        id: "fireworks",
        label: "Fireworks",
        group: ProviderGroup::Hosted,
        wire: Wire::OpenAi,
        base_url: "https://api.fireworks.ai/inference/v1",
        models_url: "https://api.fireworks.ai/inference/v1/models",
        key_page_url: Some("https://app.fireworks.ai/settings/users/api-keys"),
        default_model: "accounts/fireworks/models/llama-v3p3-70b-instruct",
        needs_key: true,
        supports_connect: false,
        probe_port: None,
        curated_models: &["accounts/fireworks/models/llama-v3p3-70b-instruct"],
    },
    ProviderInfo {
        id: "custom",
        label: "Custom (OpenAI-compatible)",
        group: ProviderGroup::Custom,
        wire: Wire::OpenAi,
        base_url: "",
        models_url: "",
        key_page_url: None,
        default_model: "",
        needs_key: false,
        supports_connect: false,
        probe_port: None,
        curated_models: &[],
    },
];

/// The row for an id, or `None` when the id is not one of ours.
pub fn provider(id: &str) -> Option<&'static ProviderInfo> {
    PROVIDERS.iter().find(|p| p.id == id)
}

/// Where the model list for a configured provider is fetched from.
///
/// `custom_base_url` is read only for rows that carry no list URL of their
/// own, where the list is `{base_url}/models`. An unknown id answers `None`.
pub fn models_url_for(cfg_provider: &str, custom_base_url: &str) -> Option<String> {
    let row = provider(cfg_provider)?;
    if row.models_url.is_empty() {
        Some(format!("{}/models", custom_base_url.trim_end_matches('/')))
    } else {
        Some(row.models_url.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_id_is_unique() {
        let mut seen = HashSet::new();
        for p in PROVIDERS {
            assert!(seen.insert(p.id), "duplicate id {}", p.id);
        }
        assert_eq!(seen.len(), PROVIDERS.len());
    }

    #[test]
    fn every_hosted_row_has_a_key_page_and_needs_a_key() {
        for p in PROVIDERS
            .iter()
            .filter(|p| p.group == ProviderGroup::Hosted)
        {
            assert!(p.needs_key, "{} should need a key", p.id);
            assert!(p.key_page_url.is_some(), "{} has no key page", p.id);
            assert!(!p.base_url.is_empty(), "{} has no base url", p.id);
            assert!(!p.models_url.is_empty(), "{} has no models url", p.id);
        }
    }

    #[test]
    fn local_rows_have_a_probe_port_and_no_key_page() {
        let local: Vec<_> = PROVIDERS
            .iter()
            .filter(|p| p.group == ProviderGroup::Local)
            .collect();
        assert_eq!(local.len(), 2);
        for p in local {
            assert!(!p.needs_key, "{} should not need a key", p.id);
            assert!(p.key_page_url.is_none(), "{} has a key page", p.id);
            assert!(p.probe_port.is_some(), "{} has no probe port", p.id);
        }
        assert_eq!(provider("ollama").unwrap().probe_port, Some(11434));
        assert_eq!(provider("lmstudio").unwrap().probe_port, Some(1234));
    }

    #[test]
    fn gemini_lists_models_natively_and_chats_through_the_openai_path() {
        let gemini = provider("gemini").unwrap();
        assert_eq!(
            gemini.base_url,
            "https://generativelanguage.googleapis.com/v1beta/openai"
        );
        assert_eq!(
            gemini.models_url,
            "https://generativelanguage.googleapis.com/v1beta/models"
        );
        assert_eq!(gemini.wire, Wire::OpenAi);
    }

    #[test]
    fn custom_carries_no_urls_of_its_own() {
        let custom = provider("custom").unwrap();
        assert!(custom.base_url.is_empty());
        assert!(custom.models_url.is_empty());
        assert!(!custom.needs_key);
        assert_eq!(custom.group, ProviderGroup::Custom);
    }

    #[test]
    fn only_openrouter_supports_connect() {
        let connectable: Vec<_> = PROVIDERS
            .iter()
            .filter(|p| p.supports_connect)
            .map(|p| p.id)
            .collect();
        assert_eq!(connectable, vec!["openrouter"]);
    }

    #[test]
    fn a_custom_models_url_is_derived_from_the_typed_base_url() {
        assert_eq!(
            models_url_for("custom", "https://x.example/v1/").as_deref(),
            Some("https://x.example/v1/models")
        );
        assert_eq!(
            models_url_for("ollama", "").as_deref(),
            Some("http://localhost:11434/api/tags")
        );
        assert_eq!(models_url_for("nope", "https://x.example/v1"), None);
    }

    #[test]
    fn group_and_wire_serialise_as_the_ipc_strings() {
        assert_eq!(
            serde_json::to_string(&ProviderGroup::Local).unwrap(),
            "\"local\""
        );
        assert_eq!(
            serde_json::to_string(&ProviderGroup::Hosted).unwrap(),
            "\"hosted\""
        );
        assert_eq!(
            serde_json::to_string(&ProviderGroup::Custom).unwrap(),
            "\"custom\""
        );
        assert_eq!(serde_json::to_string(&Wire::OpenAi).unwrap(), "\"openai\"");
        assert_eq!(
            serde_json::to_string(&Wire::Anthropic).unwrap(),
            "\"anthropic\""
        );
    }

    #[test]
    fn the_table_is_the_one_source_of_the_curated_ids() {
        // The first entry is the id the picker shows first, so the order is
        // part of the data rather than incidental.
        assert_eq!(provider("ollama").unwrap().curated_models[0], "qwen3:4b");
        assert_eq!(
            provider("groq").unwrap().curated_models,
            ["llama-3.3-70b-versatile", "llama-3.1-8b-instant"]
        );
        assert_eq!(
            provider("deepseek").unwrap().curated_models,
            ["deepseek-flash", "deepseek-v4-pro"]
        );
        // The rows whose list is whatever the runtime or the typed endpoint
        // holds suggest nothing.
        assert!(provider("lmstudio").unwrap().curated_models.is_empty());
        assert!(provider("custom").unwrap().curated_models.is_empty());

        // Every hosted row's default is one of its own suggestions, so the id
        // shown first is always one the picker offers.
        for row in PROVIDERS
            .iter()
            .filter(|p| p.group == ProviderGroup::Hosted)
        {
            assert!(
                row.curated_models.contains(&row.default_model),
                "{} shows a model first that it does not suggest",
                row.id
            );
        }
    }

    #[test]
    fn anthropic_is_the_only_anthropic_wire() {
        let anthropic: Vec<_> = PROVIDERS
            .iter()
            .filter(|p| p.wire == Wire::Anthropic)
            .map(|p| p.id)
            .collect();
        assert_eq!(anthropic, vec!["anthropic"]);
    }
}
