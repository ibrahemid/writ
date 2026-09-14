//! `[ai]` configuration section: one connection, two feature switches.
//!
//! A connection is a provider id from [`crate::ai::providers`], a model, and,
//! for `custom` only, a base URL typed by hand. Rewriting selected text and
//! the chat pane are switches over that one connection, each invisible until
//! it is turned on, and the chat may name a model of its own. Nothing leaves
//! the machine until a hosted provider is chosen and its host is consented to;
//! the default is a local Ollama server. API keys are never stored here — they
//! live in the OS keychain under the provider id.
//!
//! Files written by earlier versions carried a `preset`, a base URL for every
//! provider and a second endpoint under `[ai.chat]`. They are read through
//! [`AiConfigOnDisk`] and converted by one pure function,
//! `AiConfig::from(AiConfigOnDisk)`. Serialisation writes the new shape only,
//! so a file upgrades on its next save.

use serde::{Deserialize, Serialize};

use crate::ai::providers::{self, Wire};

/// The provider a fresh install talks to: Ollama on this machine.
pub const DEFAULT_PROVIDER: &str = "ollama";

/// The base URL the pre-1.0 DeepSeek preset carried. The table dropped the
/// `/v1` the server accepts either way, so a config that still names it is
/// recognised as DeepSeek rather than a hand-typed endpoint.
const LEGACY_DEEPSEEK_BASE_URL: &str = "https://api.deepseek.com/v1";

fn default_provider() -> String {
    DEFAULT_PROVIDER.to_string()
}

/// Whether selected text can be rewritten (`[ai.rewrite]`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiRewriteConfig {
    /// Master switch. When `false` (default) the rewrite commands, their menu
    /// entries and their network path do not exist.
    #[serde(default)]
    pub enabled: bool,
}

/// Whether the chat pane exists, and which model it talks to (`[ai.chat]`).
///
/// The endpoint is the connection's; only the model may differ, for the case
/// where a person proofreads with a small model and talks to a large one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AiChatConfig {
    /// Master switch. When `false` (default) the pane, its palette entry and
    /// its network path do not exist.
    #[serde(default)]
    pub enabled: bool,
    /// Model id for chat requests. Empty means the connection's model.
    #[serde(default)]
    pub model: String,
}

/// The one AI connection and the two features that use it (`[ai]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "AiConfigOnDisk")]
pub struct AiConfig {
    /// Provider id from [`crate::ai::providers::PROVIDERS`]. Drives the base
    /// URL, the wire format, the model list and the keychain account.
    #[serde(default = "default_provider")]
    pub provider: String,
    /// The typed OpenAI-compatible base URL. Read only when `provider` is
    /// `custom`; every other row takes its base URL from the table.
    #[serde(default)]
    pub base_url: String,
    /// Model id sent in each request. Empty until the list is fetched or a
    /// model is picked; a request refuses to run until it is set.
    #[serde(default)]
    pub model: String,
    /// Hosts (non-local) for which the one-time send notice was accepted. A
    /// hosted request refuses to run until the request's own host is listed,
    /// so consenting to one provider never covers another or a hand-edited
    /// URL.
    #[serde(default)]
    pub consented_hosts: Vec<String>,
    /// Rewriting selected text.
    #[serde(default)]
    pub rewrite: AiRewriteConfig,
    /// The chat pane.
    #[serde(default)]
    pub chat: AiChatConfig,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            base_url: String::new(),
            model: String::new(),
            consented_hosts: Vec::new(),
            rewrite: AiRewriteConfig::default(),
            chat: AiChatConfig::default(),
        }
    }
}

impl AiConfig {
    /// Where requests go: the provider table's base URL, or the typed one for
    /// `custom` and for any id the table does not know.
    pub fn effective_base_url(&self) -> String {
        match providers::provider(&self.provider) {
            Some(row) if !row.base_url.is_empty() => row.base_url.to_string(),
            _ => self.base_url.clone(),
        }
    }

    /// Which wire format the connection speaks. `custom` and an unknown id
    /// speak the OpenAI-compatible one.
    pub fn wire(&self) -> Wire {
        providers::provider(&self.provider)
            .map(|row| row.wire)
            .unwrap_or(Wire::OpenAi)
    }

    /// The model the chat sends: its own when it names one, the connection's
    /// otherwise.
    pub fn chat_model(&self) -> &str {
        if self.chat.model.is_empty() {
            &self.model
        } else {
            &self.chat.model
        }
    }
}

/// The `[ai.rewrite]` table as it may appear on disk.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AiRewriteConfigOnDisk {
    /// Whether rewriting is on.
    #[serde(default)]
    pub enabled: bool,
}

/// The `[ai.chat]` table as it may appear on disk, old fields included.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AiChatConfigOnDisk {
    /// Whether the chat pane is on.
    #[serde(default)]
    pub enabled: bool,
    /// The wire id the chat used to carry: `anthropic` or `openai_compatible`.
    #[serde(default)]
    pub provider: Option<String>,
    /// The second endpoint the chat used to carry.
    #[serde(default)]
    pub base_url: Option<String>,
    /// Model id for chat requests.
    #[serde(default)]
    pub model: String,
}

/// Every `[ai]` field any released version has written, each defaulted.
///
/// Reading is one-way: this shape exists so [`AiConfig`] can be built from a
/// file of any age by one pure conversion, and it is never written back.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AiConfigOnDisk {
    /// The connection's provider id. Present only in the current shape, which
    /// is how a file's age is told.
    #[serde(default)]
    pub provider: Option<String>,
    /// The old master switch for rewriting.
    #[serde(default)]
    pub enabled: bool,
    /// The old provider preset id.
    #[serde(default)]
    pub preset: Option<String>,
    /// The base URL: the connection's in the current shape, the rewrite
    /// path's in the old one.
    #[serde(default)]
    pub base_url: Option<String>,
    /// Model id.
    #[serde(default)]
    pub model: String,
    /// Consented hosts, unchanged in every shape.
    #[serde(default)]
    pub consented_hosts: Vec<String>,
    /// The rewrite switch in the current shape.
    #[serde(default)]
    pub rewrite: AiRewriteConfigOnDisk,
    /// The chat table, old fields included.
    #[serde(default)]
    pub chat: AiChatConfigOnDisk,
}

/// The provider id and typed base URL an old `[ai.chat]` table resolves to.
///
/// An `anthropic` chat is the Anthropic row. An OpenAI-compatible one is
/// whichever row carries that base URL, ignoring a trailing slash and
/// accepting the DeepSeek preset's old `/v1`; anything else is `custom`, whose
/// URL is kept as typed.
fn connection_from_chat(chat: &AiChatConfigOnDisk) -> (String, String) {
    if chat.provider.as_deref() == Some("anthropic") {
        return ("anthropic".to_string(), String::new());
    }
    let typed = chat.base_url.clone().unwrap_or_default();
    let trimmed = typed.trim_end_matches('/');
    if trimmed == LEGACY_DEEPSEEK_BASE_URL {
        return ("deepseek".to_string(), String::new());
    }
    let matched = providers::PROVIDERS
        .iter()
        .find(|row| !row.base_url.is_empty() && row.base_url.trim_end_matches('/') == trimmed);
    match matched {
        Some(row) => (row.id.to_string(), String::new()),
        None => ("custom".to_string(), typed),
    }
}

impl From<AiConfigOnDisk> for AiConfig {
    fn from(disk: AiConfigOnDisk) -> Self {
        let consented_hosts = disk.consented_hosts;

        // The current shape names its provider; nothing else has to be read.
        if let Some(provider) = disk.provider {
            let provider = if provider.trim().is_empty() {
                default_provider()
            } else {
                provider
            };
            return Self {
                provider,
                base_url: disk.base_url.unwrap_or_default(),
                model: disk.model,
                consented_hosts,
                rewrite: AiRewriteConfig {
                    enabled: disk.rewrite.enabled,
                },
                chat: AiChatConfig {
                    enabled: disk.chat.enabled,
                    model: disk.chat.model,
                },
            };
        }

        let typed_base_url = disk.base_url.unwrap_or_default();
        let chat_on = disk.chat.enabled;

        // The chat alone: the connection is the endpoint it was pointed at.
        if chat_on && !disk.enabled {
            let (provider, base_url) = connection_from_chat(&disk.chat);
            return Self {
                provider,
                base_url,
                model: disk.chat.model,
                consented_hosts,
                rewrite: AiRewriteConfig { enabled: false },
                chat: AiChatConfig {
                    enabled: true,
                    model: String::new(),
                },
            };
        }

        // Otherwise the rewrite side is the connection, and a chat that was
        // pointed elsewhere gives up its endpoint but keeps its switch.
        let provider = match disk.preset {
            Some(preset) if !preset.trim().is_empty() => preset,
            _ => default_provider(),
        };
        let base_url = if provider == "custom" {
            typed_base_url
        } else {
            String::new()
        };
        let chat_model = if chat_on {
            let (chat_provider, chat_base_url) = connection_from_chat(&disk.chat);
            let same_connection = chat_provider == provider && chat_base_url == base_url;
            if same_connection && disk.chat.model != disk.model {
                disk.chat.model
            } else {
                String::new()
            }
        } else {
            String::new()
        };

        Self {
            provider,
            base_url,
            model: disk.model,
            consented_hosts,
            rewrite: AiRewriteConfig {
                enabled: disk.enabled,
            },
            chat: AiChatConfig {
                enabled: chat_on,
                model: chat_model,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::providers::Wire;

    fn parse(toml_src: &str) -> AiConfig {
        toml::from_str(toml_src).unwrap()
    }

    #[test]
    fn defaults_are_off_and_local() {
        let c = AiConfig::default();
        assert_eq!(c.provider, "ollama");
        assert!(c.base_url.is_empty());
        assert!(c.model.is_empty());
        assert!(c.consented_hosts.is_empty());
        assert!(!c.rewrite.enabled);
        assert!(!c.chat.enabled);
        assert!(c.chat.model.is_empty());
        assert_eq!(c.effective_base_url(), "http://localhost:11434/v1");
        assert_eq!(c.wire(), Wire::OpenAi);
    }

    #[test]
    fn row_1_a_file_written_before_the_chat_existed() {
        let c = parse(
            "enabled = true\npreset = \"groq\"\nbase_url = \"https://api.groq.com/openai/v1\"\nmodel = \"llama-3.3-70b-versatile\"\nconsented_hosts = [\"api.groq.com\"]\n",
        );
        assert_eq!(c.provider, "groq");
        assert!(c.rewrite.enabled);
        assert_eq!(c.model, "llama-3.3-70b-versatile");
        assert!(c.base_url.is_empty(), "a preset's base url is the table's");
        assert_eq!(c.effective_base_url(), "https://api.groq.com/openai/v1");
        assert!(!c.chat.enabled);
    }

    #[test]
    fn row_1_a_hand_typed_base_url_is_kept_as_custom() {
        let c = parse(
            "enabled = true\npreset = \"custom\"\nbase_url = \"https://llm.example/v1\"\nmodel = \"local-thing\"\n",
        );
        assert_eq!(c.provider, "custom");
        assert_eq!(c.base_url, "https://llm.example/v1");
        assert_eq!(c.effective_base_url(), "https://llm.example/v1");
    }

    #[test]
    fn row_2_rewrite_on_with_the_chat_off_or_absent() {
        let with_table = parse(
            "enabled = true\npreset = \"openai\"\nmodel = \"gpt-4o-mini\"\n\n[chat]\nenabled = false\nprovider = \"anthropic\"\nbase_url = \"https://api.anthropic.com\"\nmodel = \"claude-opus-5\"\n",
        );
        assert_eq!(with_table.provider, "openai");
        assert!(with_table.rewrite.enabled);
        assert_eq!(with_table.model, "gpt-4o-mini");
        assert!(!with_table.chat.enabled);
        assert!(with_table.chat.model.is_empty());

        let absent = parse("enabled = true\npreset = \"openai\"\nmodel = \"gpt-4o-mini\"\n");
        assert_eq!(absent, with_table);
    }

    #[test]
    fn row_3_the_chat_alone_on_anthropic() {
        let c = parse(
            "enabled = false\npreset = \"ollama\"\n\n[chat]\nenabled = true\nprovider = \"anthropic\"\nbase_url = \"https://api.anthropic.com\"\nmodel = \"claude-opus-5\"\n",
        );
        assert_eq!(c.provider, "anthropic");
        assert_eq!(c.model, "claude-opus-5");
        assert!(c.chat.enabled);
        assert!(!c.rewrite.enabled);
        assert!(c.chat.model.is_empty());
        assert_eq!(c.chat_model(), "claude-opus-5");
        assert_eq!(c.wire(), Wire::Anthropic);
        assert_eq!(c.effective_base_url(), "https://api.anthropic.com");
    }

    #[test]
    fn row_4_the_chat_alone_at_a_base_url_the_table_knows() {
        let c = parse(
            "enabled = false\n\n[chat]\nenabled = true\nprovider = \"openai_compatible\"\nbase_url = \"https://api.groq.com/openai/v1/\"\nmodel = \"llama-3.3-70b-versatile\"\n",
        );
        assert_eq!(c.provider, "groq");
        assert!(c.base_url.is_empty());
        assert_eq!(c.model, "llama-3.3-70b-versatile");

        let deepseek = parse(
            "[chat]\nenabled = true\nprovider = \"openai_compatible\"\nbase_url = \"https://api.deepseek.com/v1\"\nmodel = \"deepseek-chat\"\n",
        );
        assert_eq!(deepseek.provider, "deepseek");
        assert_eq!(deepseek.effective_base_url(), "https://api.deepseek.com");
    }

    #[test]
    fn row_5_the_chat_alone_at_a_base_url_the_table_does_not_know() {
        let c = parse(
            "[chat]\nenabled = true\nprovider = \"openai_compatible\"\nbase_url = \"https://llm.example/v1\"\nmodel = \"house-model\"\n",
        );
        assert_eq!(c.provider, "custom");
        assert_eq!(c.base_url, "https://llm.example/v1");
        assert_eq!(c.model, "house-model");
        assert_eq!(c.effective_base_url(), "https://llm.example/v1");
    }

    #[test]
    fn row_6_both_on_against_the_same_provider() {
        let same_model = parse(
            "enabled = true\npreset = \"groq\"\nmodel = \"llama-3.3-70b-versatile\"\n\n[chat]\nenabled = true\nprovider = \"openai_compatible\"\nbase_url = \"https://api.groq.com/openai/v1\"\nmodel = \"llama-3.3-70b-versatile\"\n",
        );
        assert_eq!(same_model.provider, "groq");
        assert!(same_model.rewrite.enabled && same_model.chat.enabled);
        assert!(same_model.chat.model.is_empty());
        assert_eq!(same_model.chat_model(), "llama-3.3-70b-versatile");

        let other_model = parse(
            "enabled = true\npreset = \"groq\"\nmodel = \"llama-3.1-8b-instant\"\n\n[chat]\nenabled = true\nprovider = \"openai_compatible\"\nbase_url = \"https://api.groq.com/openai/v1\"\nmodel = \"llama-3.3-70b-versatile\"\n",
        );
        assert_eq!(other_model.model, "llama-3.1-8b-instant");
        assert_eq!(other_model.chat.model, "llama-3.3-70b-versatile");
        assert_eq!(other_model.chat_model(), "llama-3.3-70b-versatile");
    }

    #[test]
    fn row_7_both_on_against_different_providers() {
        let c = parse(
            "enabled = true\npreset = \"groq\"\nmodel = \"llama-3.3-70b-versatile\"\n\n[chat]\nenabled = true\nprovider = \"anthropic\"\nbase_url = \"https://api.anthropic.com\"\nmodel = \"claude-opus-5\"\n",
        );
        assert_eq!(c.provider, "groq");
        assert_eq!(c.model, "llama-3.3-70b-versatile");
        assert!(c.chat.enabled);
        assert!(c.chat.model.is_empty());
        assert_eq!(c.chat_model(), "llama-3.3-70b-versatile");
        assert_eq!(c.wire(), Wire::OpenAi);
    }

    #[test]
    fn row_8_an_empty_table_is_the_default() {
        assert_eq!(parse(""), AiConfig::default());
        assert_eq!(
            AiConfig::from(AiConfigOnDisk::default()),
            AiConfig::default()
        );
    }

    #[test]
    fn consented_hosts_survive_every_row() {
        let hosts = "consented_hosts = [\"api.groq.com\", \"api.anthropic.com\"]\n";
        let rows = [
            format!("enabled = true\npreset = \"groq\"\n{hosts}"),
            format!("enabled = true\npreset = \"custom\"\nbase_url = \"https://llm.example/v1\"\n{hosts}"),
            format!("{hosts}\n[chat]\nenabled = true\nprovider = \"anthropic\"\nbase_url = \"https://api.anthropic.com\"\n"),
            format!("{hosts}\n[chat]\nenabled = true\nprovider = \"openai_compatible\"\nbase_url = \"https://api.groq.com/openai/v1\"\n"),
            format!("{hosts}\n[chat]\nenabled = true\nprovider = \"openai_compatible\"\nbase_url = \"https://llm.example/v1\"\n"),
            format!("enabled = true\npreset = \"groq\"\n{hosts}\n[chat]\nenabled = true\nprovider = \"openai_compatible\"\nbase_url = \"https://api.groq.com/openai/v1\"\n"),
            format!("enabled = true\npreset = \"groq\"\n{hosts}\n[chat]\nenabled = true\nprovider = \"anthropic\"\nbase_url = \"https://api.anthropic.com\"\n"),
            format!("provider = \"openai\"\n{hosts}\n[rewrite]\nenabled = true\n"),
        ];
        for row in rows {
            let c = parse(&row);
            assert_eq!(
                c.consented_hosts,
                vec!["api.groq.com".to_string(), "api.anthropic.com".to_string()],
                "lost the consented hosts in {row}"
            );
        }
    }

    #[test]
    fn serialises_the_new_shape_only() {
        #[derive(serde::Serialize)]
        struct Section {
            ai: AiConfig,
        }
        let migrated = parse(
            "enabled = true\npreset = \"groq\"\nbase_url = \"https://api.groq.com/openai/v1\"\nmodel = \"llama-3.3-70b-versatile\"\n\n[chat]\nenabled = true\nprovider = \"anthropic\"\nbase_url = \"https://api.anthropic.com\"\nmodel = \"claude-opus-5\"\n",
        );
        let s = toml::to_string(&Section { ai: migrated }).unwrap();
        assert!(s.contains("provider = \"groq\""), "{s}");
        assert!(s.contains("[ai.rewrite]"), "{s}");
        assert!(s.contains("[ai.chat]"), "{s}");
        assert!(!s.contains("preset"), "{s}");
        let chat_section = s.split("[ai.chat]").nth(1).unwrap();
        assert!(!chat_section.contains("base_url"), "{s}");
        assert!(!chat_section.contains("provider"), "{s}");
    }

    #[test]
    fn the_new_shape_round_trips_through_toml() {
        let c = AiConfig {
            provider: "custom".to_string(),
            base_url: "https://llm.example/v1".to_string(),
            model: "house-model".to_string(),
            consented_hosts: vec!["llm.example".to_string()],
            rewrite: AiRewriteConfig { enabled: true },
            chat: AiChatConfig {
                enabled: true,
                model: "house-model-large".to_string(),
            },
        };
        let s = toml::to_string(&c).unwrap();
        let back: AiConfig = toml::from_str(&s).unwrap();
        assert_eq!(c, back);
        assert_eq!(back.chat_model(), "house-model-large");
        assert_eq!(back.effective_base_url(), "https://llm.example/v1");
    }
}
