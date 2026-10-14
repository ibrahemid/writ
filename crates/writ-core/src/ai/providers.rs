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
    /// A model built into the operating system, reached without a network
    /// address ([`crate::ai::on_device`]). Offered only where the system has
    /// one ([`offered_providers`]).
    OnDevice,
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
    /// The model a row starts from, and the id shown first among the
    /// suggestions when the provider's own list cannot be read. Written to
    /// `config.toml` only once the provider's own list holds it
    /// (`crate::ai::models::seed_model`). Empty for the local rows, whose
    /// lists are whatever is installed.
    pub default_model: &'static str,
    /// Whether a request needs a key. False for local and for custom.
    pub needs_key: bool,
    /// Whether the provider can hand over a key without one being pasted.
    pub supports_connect: bool,
    /// The loopback port the probe knocks on, for local rows.
    pub probe_port: Option<u16>,
    /// Model ids offered when the provider's own list cannot be read, and the
    /// ids a row falls back to, in order, when its list lacks
    /// `default_model`. Suggestions, not an inventory: one is written to the
    /// file only once the provider's own list holds it. First entry is the one
    /// shown first. Empty for the rows whose list is whatever the runtime or
    /// the typed endpoint holds.
    ///
    /// These are read by hand from each provider's documentation or list and
    /// go stale when a provider retires an id; a stale one costs a row its
    /// seed, never a failed send, and is corrected here in a release. Each
    /// hosted row carries the date it was read.
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
        id: "llamacpp",
        label: "llama.cpp",
        group: ProviderGroup::Local,
        wire: Wire::OpenAi,
        base_url: "http://localhost:8080/v1",
        models_url: "http://localhost:8080/v1/models",
        key_page_url: None,
        default_model: "",
        needs_key: false,
        supports_connect: false,
        probe_port: Some(8080),
        curated_models: &[],
    },
    ProviderInfo {
        id: "jan",
        label: "Jan",
        group: ProviderGroup::Local,
        wire: Wire::OpenAi,
        base_url: "http://localhost:1337/v1",
        models_url: "http://localhost:1337/v1/models",
        key_page_url: None,
        default_model: "",
        needs_key: false,
        supports_connect: false,
        probe_port: Some(1337),
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
        // Read from the models overview on 2026-10-13. Sonnet first, Haiku
        // before Opus, so a list that lacks the default falls to the cheaper
        // model rather than the dearest.
        default_model: "claude-sonnet-5-5",
        needs_key: true,
        supports_connect: false,
        probe_port: None,
        curated_models: &["claude-sonnet-5-5", "claude-haiku-5-5", "claude-opus-5-5"],
    },
    ProviderInfo {
        id: "openai",
        label: "OpenAI",
        group: ProviderGroup::Hosted,
        wire: Wire::OpenAi,
        base_url: "https://api.openai.com/v1",
        models_url: "https://api.openai.com/v1/models",
        key_page_url: Some("https://platform.openai.com/api-keys"),
        // Read from the model catalogue on 2026-10-13: the three featured
        // models, cheapest first. `gpt-5-mini` is deprecated and leaves the
        // API on 2026-12-11.
        default_model: "gpt-6-luna",
        needs_key: true,
        supports_connect: false,
        probe_port: None,
        curated_models: &["gpt-6-luna", "gpt-6.1-sol", "gpt-6-astra"],
    },
    ProviderInfo {
        id: "gemini",
        label: "Google Gemini",
        group: ProviderGroup::Hosted,
        wire: Wire::OpenAi,
        base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
        models_url: "https://generativelanguage.googleapis.com/v1beta/models",
        key_page_url: Some("https://aistudio.google.com/apikey"),
        // Read from the models page on 2026-10-13, which serves the 2.5 models
        // only to accounts that already used them.
        default_model: "gemini-3.8-flash",
        needs_key: true,
        supports_connect: false,
        probe_port: None,
        curated_models: &["gemini-3.8-flash", "gemini-3.5-flash-lite"],
    },
    ProviderInfo {
        id: "openrouter",
        label: "OpenRouter",
        group: ProviderGroup::Hosted,
        wire: Wire::OpenAi,
        base_url: "https://openrouter.ai/api/v1",
        models_url: "https://openrouter.ai/api/v1/models",
        key_page_url: Some("https://openrouter.ai/settings/keys"),
        // Each id was in the public list on 2026-10-13.
        default_model: "meta-llama/llama-3.3-70b-instruct",
        needs_key: true,
        supports_connect: true,
        probe_port: None,
        curated_models: &[
            "meta-llama/llama-3.3-70b-instruct",
            "openai/gpt-6-luna",
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
        // Read from the models and deprecations pages on 2026-10-13. Llama
        // 3.3 70B and 3.1 8B left the free and developer tiers on 2026-08-16;
        // these are the replacements the deprecation page names for them.
        default_model: "openai/gpt-oss-120b",
        needs_key: true,
        supports_connect: false,
        probe_port: None,
        curated_models: &["openai/gpt-oss-120b", "openai/gpt-oss-20b"],
    },
    ProviderInfo {
        id: "deepseek",
        label: "DeepSeek",
        group: ProviderGroup::Hosted,
        wire: Wire::OpenAi,
        base_url: "https://api.deepseek.com",
        models_url: "https://api.deepseek.com/models",
        key_page_url: Some("https://platform.deepseek.com/api_keys"),
        // Read from the live list on 2026-09-18 and the models page on
        // 2026-10-13. `deepseek-chat` and `deepseek-reasoner` are gone from
        // it; the endpoint still answers a request for `deepseek-reasoner` as
        // an alias, which is why a stale suggestion is not a safe thing to
        // write into the file.
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
        // Read from the model pages on 2026-10-13: the aliases name Mistral
        // Small 4 and Mistral Large 3.
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
        key_page_url: Some("https://console.x.ai/team/default/api-keys"),
        // Read from the models page and the retirement guide on 2026-10-13.
        // `grok-4` was retired on 2026-05-15 and is answered by `grok-4.3`,
        // the model the guide names in its place; `grok-4.7` is the one the
        // models page recommends for chat.
        default_model: "grok-4.3",
        needs_key: true,
        supports_connect: false,
        probe_port: None,
        curated_models: &["grok-4.3", "grok-4.7"],
    },
    ProviderInfo {
        id: "together",
        label: "Together",
        group: ProviderGroup::Hosted,
        wire: Wire::OpenAi,
        // The host the API reference and quickstart name on 2026-10-13.
        base_url: "https://api.together.ai/v1",
        models_url: "https://api.together.ai/v1/models",
        key_page_url: Some("https://api.together.ai/settings/projects/~current/api-keys"),
        // Read from the serverless catalogue on 2026-10-13, which no longer
        // serves the Llama 3.3 Turbo model this row named.
        default_model: "openai/gpt-oss-120b",
        needs_key: true,
        supports_connect: false,
        probe_port: None,
        curated_models: &[
            "openai/gpt-oss-120b",
            "Qwen/Qwen3.8-Flash",
            "deepseek-ai/DeepSeek-V4.1-Flash",
        ],
    },
    ProviderInfo {
        id: "fireworks",
        label: "Fireworks",
        group: ProviderGroup::Hosted,
        wire: Wire::OpenAi,
        base_url: "https://api.fireworks.ai/inference/v1",
        models_url: "https://api.fireworks.ai/inference/v1/models",
        key_page_url: Some("https://app.fireworks.ai/settings/users/api-keys"),
        // Read from the serverless pricing page and the quickstart on
        // 2026-10-13. Llama 3.3 70B is no longer served serverless; GLM 5.3
        // Flash is the quickstart's model and the cheaper of these two.
        default_model: "accounts/fireworks/models/glm-5p3-flash",
        needs_key: true,
        supports_connect: false,
        probe_port: None,
        curated_models: &[
            "accounts/fireworks/models/glm-5p3-flash",
            "accounts/fireworks/models/gpt-oss-120b",
        ],
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
    // No URL, no model list and no wire: requests go through the system
    // framework, and every HTTP path refuses this row before it resolves an
    // endpoint (`AiConfig::effective_base_url` answers nothing for it).
    ProviderInfo {
        id: "apple",
        label: "Apple Intelligence",
        group: ProviderGroup::OnDevice,
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

/// The rows this machine shows: every row, less the on-device one where the
/// system has no on-device model to offer
/// ([`super::on_device::offers_on_device`]).
pub fn offered_providers(on_device: bool) -> Vec<ProviderInfo> {
    PROVIDERS
        .iter()
        .filter(|row| on_device || row.group != ProviderGroup::OnDevice)
        .copied()
        .collect()
}

/// The field a request's output ceiling is sent under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CeilingField {
    /// `max_tokens`: the Messages API, and every OpenAI-compatible server
    /// other than OpenAI's own.
    MaxTokens,
    /// `max_completion_tokens`: OpenAI, whose reasoning models refuse
    /// `max_tokens`.
    MaxCompletionTokens,
}

impl CeilingField {
    /// The field name as the body carries it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MaxTokens => "max_tokens",
            Self::MaxCompletionTokens => "max_completion_tokens",
        }
    }
}

/// How one host wants a request body written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BodyRules {
    /// The field the output ceiling goes under.
    pub ceiling: CeilingField,
    /// Whether a sampling temperature other than the model's own default is
    /// accepted.
    pub takes_temperature: bool,
}

impl Default for BodyRules {
    fn default() -> Self {
        Self {
            ceiling: CeilingField::MaxTokens,
            takes_temperature: true,
        }
    }
}

/// The hosts that refuse what the others accept, read from their
/// documentation on 2026-10-13. OpenAI deprecates `max_tokens` and its
/// reasoning models refuse it, and they refuse any temperature unless their
/// reasoning is off. Claude 4.7 and later refuse any temperature but their own
/// on every request. Both answer the refusal with a 400.
///
/// Keyed by host rather than by provider row, because a hand-typed endpoint
/// on one of these hosts reaches the same models.
const HOST_BODY_RULES: &[(&str, BodyRules)] = &[
    (
        OPENAI_API_HOST,
        BodyRules {
            ceiling: CeilingField::MaxCompletionTokens,
            takes_temperature: false,
        },
    ),
    (
        "api.anthropic.com",
        BodyRules {
            ceiling: CeilingField::MaxTokens,
            takes_temperature: false,
        },
    ),
];

/// OpenAI's API host. Its regional hosts, `eu.api.openai.com` and the others
/// OpenAI's data residency guide lists (read on 2026-10-13), each put a prefix
/// on this one and serve the same API, so they take its rules.
const OPENAI_API_HOST: &str = "api.openai.com";

/// How a request to `host` is written. A host the table does not name takes
/// the defaults every OpenAI-compatible server reads. A host written with the
/// root's trailing dot (`api.openai.com.`) is the same host.
pub fn body_rules(host: &str) -> BodyRules {
    let host = host.strip_suffix('.').unwrap_or(host);
    let host = if is_subdomain_of(host, OPENAI_API_HOST) {
        OPENAI_API_HOST
    } else {
        host
    };
    HOST_BODY_RULES
        .iter()
        .find(|(named, _)| named.eq_ignore_ascii_case(host))
        .map(|(_, rules)| *rules)
        .unwrap_or_default()
}

/// Whether `host` is a subdomain of `parent`, ignoring ASCII case.
fn is_subdomain_of(host: &str, parent: &str) -> bool {
    let (host, parent) = (host.as_bytes(), parent.as_bytes());
    let Some(label_end) = host.len().checked_sub(parent.len() + 1) else {
        return false;
    };
    label_end > 0 && host[label_end] == b'.' && host[label_end + 1..].eq_ignore_ascii_case(parent)
}

/// Where the model list for a configured provider is fetched from.
///
/// `custom_base_url` is read only for rows that carry no list URL of their
/// own, where the list is `{base_url}/models`. An unknown id answers `None`.
pub fn models_url_for(cfg_provider: &str, custom_base_url: &str) -> Option<String> {
    let row = provider(cfg_provider)?;
    if row.group == ProviderGroup::OnDevice {
        return None;
    }
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
        assert_eq!(local.len(), 4);
        for p in local {
            assert!(!p.needs_key, "{} should not need a key", p.id);
            assert!(p.key_page_url.is_none(), "{} has a key page", p.id);
            assert!(p.probe_port.is_some(), "{} has no probe port", p.id);
        }
        assert_eq!(provider("ollama").unwrap().probe_port, Some(11434));
        assert_eq!(provider("lmstudio").unwrap().probe_port, Some(1234));
        assert_eq!(provider("llamacpp").unwrap().probe_port, Some(8080));
        assert_eq!(provider("jan").unwrap().probe_port, Some(1337));
    }

    #[test]
    fn the_on_device_row_is_offered_only_where_the_system_has_the_model() {
        let row = provider(super::super::on_device::ON_DEVICE_PROVIDER).expect("the on-device row");
        assert_eq!(row.group, ProviderGroup::OnDevice);
        assert!(!row.needs_key);
        assert!(row.base_url.is_empty() && row.models_url.is_empty());
        assert_eq!(models_url_for("apple", "https://typed.example/v1"), None);

        let without = offered_providers(false);
        assert!(without.iter().all(|row| row.id != "apple"));
        assert_eq!(without.len(), PROVIDERS.len() - 1);
        let with = offered_providers(true);
        assert_eq!(with.len(), PROVIDERS.len());
        assert_eq!(with[0].id, "ollama");
        assert_eq!(
            serde_json::to_string(&ProviderGroup::OnDevice).unwrap(),
            "\"on_device\""
        );
    }

    #[test]
    fn llama_cpp_and_jan_speak_the_openai_path_on_their_own_ports() {
        let llamacpp = provider("llamacpp").unwrap();
        assert_eq!(llamacpp.base_url, "http://localhost:8080/v1");
        assert_eq!(llamacpp.models_url, "http://localhost:8080/v1/models");
        let jan = provider("jan").unwrap();
        assert_eq!(jan.base_url, "http://localhost:1337/v1");
        assert_eq!(jan.models_url, "http://localhost:1337/v1/models");
        for row in [llamacpp, jan] {
            assert_eq!(row.wire, Wire::OpenAi);
            assert!(row.curated_models.is_empty(), "{}", row.id);
        }
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
            ["openai/gpt-oss-120b", "openai/gpt-oss-20b"]
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
    fn openai_takes_its_ceiling_under_its_own_name_and_no_temperature() {
        let openai = body_rules("api.openai.com");
        assert_eq!(openai.ceiling, CeilingField::MaxCompletionTokens);
        assert_eq!(openai.ceiling.as_str(), "max_completion_tokens");
        assert!(!openai.takes_temperature);
        assert_eq!(body_rules("API.OpenAI.com"), openai);
    }

    #[test]
    fn openai_s_regional_hosts_take_its_rules() {
        // The data residency guide's hosts, read on 2026-10-13: each is a
        // prefix on api.openai.com and serves the same API.
        let openai = body_rules("api.openai.com");
        for region in ["us", "eu", "au", "ca", "jp", "in", "sg", "kr", "gb", "ae"] {
            let host = format!("{region}.api.openai.com");
            assert_eq!(body_rules(&host), openai, "{host}");
        }
        assert_eq!(body_rules("EU.API.OpenAI.com"), openai);
        for lookalike in [
            "api.openai.com.example",
            "xapi.openai.com",
            ".api.openai.com",
            "eu-api.openai.com",
            "openai.com",
            "eu.api.anthropic.com",
        ] {
            assert_eq!(body_rules(lookalike), BodyRules::default(), "{lookalike}");
        }
    }

    #[test]
    fn a_host_written_with_the_root_s_dot_takes_the_same_rules() {
        for host in [
            "api.openai.com",
            "API.OpenAI.com",
            "eu.api.openai.com",
            "api.anthropic.com",
            "openrouter.ai",
        ] {
            assert_eq!(body_rules(&format!("{host}.")), body_rules(host), "{host}.");
        }
        for not_a_host in ["api.openai.com..", "."] {
            assert_eq!(body_rules(not_a_host), BodyRules::default(), "{not_a_host}");
        }
    }

    #[test]
    fn anthropic_takes_max_tokens_and_no_temperature() {
        let anthropic = body_rules("api.anthropic.com");
        assert_eq!(anthropic.ceiling.as_str(), "max_tokens");
        assert!(!anthropic.takes_temperature);
    }

    #[test]
    fn every_other_host_takes_the_defaults() {
        for host in [
            "localhost",
            "openrouter.ai",
            "api.groq.com",
            "generativelanguage.googleapis.com",
            "openai.com.example",
        ] {
            assert_eq!(body_rules(host), BodyRules::default(), "{host}");
        }
        assert_eq!(BodyRules::default().ceiling, CeilingField::MaxTokens);
        assert!(BodyRules::default().takes_temperature);
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
