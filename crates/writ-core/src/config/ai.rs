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
    /// The provider id `model` was chosen under. An override is read only when
    /// this matches the connection's provider, so a model picked from one
    /// server is never sent to another. Empty in files written before the
    /// field existed, and in a chat that names no model of its own; the
    /// migration qualifies the first case at load.
    #[serde(default)]
    pub model_provider: String,
}

impl AiChatConfig {
    /// The chat's own model when it belongs to `provider`, `None` otherwise.
    ///
    /// An unqualified override answers `None`: the only place a qualifier is
    /// written without one being chosen is the migration, so anything that
    /// reaches here unqualified is a value of unknown origin.
    pub fn override_for(&self, provider: &str) -> Option<&str> {
        if self.model.is_empty() || self.model_provider.is_empty() {
            return None;
        }
        (self.model_provider == provider).then_some(self.model.as_str())
    }
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

    /// The model the chat sends: its own when it names one for this provider,
    /// the connection's otherwise.
    pub fn chat_model(&self) -> &str {
        self.chat
            .override_for(&self.provider)
            .unwrap_or(&self.model)
    }

    /// The same connection pointed at another provider.
    ///
    /// The one place a provider change happens, so the rule that an override
    /// belongs to the server it was picked from lives beside the rule that a
    /// row seeds its own model, rather than in whichever surface made the
    /// change. `default_model` is what the new row starts from; the hand-typed
    /// row has none, so the model already in the file is what the person typed
    /// and it is kept. Consent and both feature switches are untouched.
    pub fn with_provider(&self, provider: &str, default_model: &str) -> AiConfig {
        let keeps_model = providers::provider(provider)
            .map(|row| row.group == providers::ProviderGroup::Custom)
            .unwrap_or(true);
        let chat_keeps_override = self.chat.override_for(provider).is_some();
        AiConfig {
            provider: provider.to_string(),
            base_url: self.base_url.clone(),
            model: if keeps_model {
                self.model.clone()
            } else {
                default_model.to_string()
            },
            consented_hosts: self.consented_hosts.clone(),
            rewrite: self.rewrite.clone(),
            chat: AiChatConfig {
                enabled: self.chat.enabled,
                model: if chat_keeps_override {
                    self.chat.model.clone()
                } else {
                    String::new()
                },
                model_provider: if chat_keeps_override {
                    self.chat.model_provider.clone()
                } else {
                    String::new()
                },
            },
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
    /// The provider the model id was chosen under. Absent before the field
    /// existed, which is what the migration qualifies.
    #[serde(default)]
    pub model_provider: String,
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
            let chat_model_provider =
                qualify(&disk.chat.model, &disk.chat.model_provider, &provider);
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
                    model_provider: chat_model_provider,
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
                    model_provider: String::new(),
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

        let chat_model_provider = qualify(&chat_model, "", &provider);
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
                model_provider: chat_model_provider,
            },
        }
    }
}

/// The provider an override belongs to.
///
/// A chat that names no model has no qualifier, and a file that carries one
/// keeps it. Anything else is a file written before the field existed, whose
/// override was picked under the provider that file names.
fn qualify(model: &str, on_disk: &str, provider: &str) -> String {
    if model.is_empty() {
        return String::new();
    }
    if on_disk.is_empty() {
        return provider.to_string();
    }
    on_disk.to_string()
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
        // `model_provider` is the chat's own field and stays; the second
        // endpoint's `provider` key is what must be gone.
        assert!(
            !chat_section
                .lines()
                .any(|line| line.starts_with("provider")),
            "{s}"
        );
        assert!(chat_section.contains("model_provider ="), "{s}");
    }

    #[test]
    fn chat_override_is_dropped_when_the_provider_changes() {
        let mut c = AiConfig {
            provider: "ollama".to_string(),
            model: "qwen3:4b".to_string(),
            chat: AiChatConfig {
                enabled: true,
                model: "qwen2.5-coder:0.5b".to_string(),
                model_provider: "ollama".to_string(),
            },
            ..AiConfig::default()
        };
        assert_eq!(c.chat.override_for("ollama"), Some("qwen2.5-coder:0.5b"));
        assert_eq!(c.chat_model(), "qwen2.5-coder:0.5b");

        // The same file, read after the connection moved to another provider:
        // the id belongs to a server that is no longer being talked to, and
        // sending it is the 400 this field exists to stop.
        c.provider = "deepseek".to_string();
        c.model = "deepseek-chat".to_string();
        assert_eq!(c.chat.override_for("deepseek"), None);
        assert_eq!(c.chat_model(), "deepseek-chat");
    }

    #[test]
    fn a_file_without_model_provider_qualifies_the_override_to_its_provider() {
        let c = parse(
            "provider = \"ollama\"\nmodel = \"qwen3:4b\"\n\n[chat]\nenabled = true\nmodel = \"qwen2.5-coder:0.5b\"\n",
        );
        assert_eq!(c.chat.model_provider, "ollama");
        assert_eq!(c.chat_model(), "qwen2.5-coder:0.5b");

        // Nothing is stamped onto a chat that names no model of its own, so an
        // empty override never becomes one the next provider change has to drop.
        let bare = parse("provider = \"ollama\"\nmodel = \"qwen3:4b\"\n\n[chat]\nenabled = true\n");
        assert!(bare.chat.model_provider.is_empty());
        assert_eq!(bare.chat_model(), "qwen3:4b");

        // A qualifier already on disk is read as written.
        let qualified = parse(
            "provider = \"deepseek\"\nmodel = \"deepseek-chat\"\n\n[chat]\nenabled = true\nmodel = \"qwen2.5-coder:0.5b\"\nmodel_provider = \"ollama\"\n",
        );
        assert_eq!(qualified.chat.model_provider, "ollama");
        assert_eq!(qualified.chat_model(), "deepseek-chat");
    }

    #[test]
    fn with_provider_seeds_the_default_model_and_clears_a_foreign_override() {
        let ollama = AiConfig {
            provider: "ollama".to_string(),
            model: "qwen3:4b".to_string(),
            consented_hosts: vec!["api.deepseek.com".to_string()],
            rewrite: AiRewriteConfig { enabled: true },
            chat: AiChatConfig {
                enabled: true,
                model: "qwen2.5-coder:0.5b".to_string(),
                model_provider: "ollama".to_string(),
            },
            ..AiConfig::default()
        };

        let switched = ollama.with_provider("deepseek", "deepseek-chat");
        assert_eq!(switched.provider, "deepseek");
        assert_eq!(switched.model, "deepseek-chat");
        assert!(
            switched.chat.model.is_empty(),
            "the foreign override is gone"
        );
        assert!(switched.chat.model_provider.is_empty());
        assert_eq!(switched.chat_model(), "deepseek-chat");
        assert!(switched.chat.enabled, "a switch is not a preference");
        assert!(switched.rewrite.enabled);
        assert_eq!(
            switched.consented_hosts,
            vec!["api.deepseek.com".to_string()],
            "consent is not given back"
        );

        // An override that belongs to the provider being switched to is its own
        // choice and stays.
        let kept = AiConfig {
            provider: "openai".to_string(),
            chat: AiChatConfig {
                enabled: true,
                model: "deepseek-reasoner".to_string(),
                model_provider: "deepseek".to_string(),
            },
            ..ollama.clone()
        }
        .with_provider("deepseek", "deepseek-chat");
        assert_eq!(kept.chat.model, "deepseek-reasoner");
        assert_eq!(kept.chat_model(), "deepseek-reasoner");

        // The hand-typed row has no model of its own to seed, so the one in the
        // file is what the person typed and it is left alone.
        let custom = switched.with_provider("custom", "");
        assert_eq!(custom.provider, "custom");
        assert_eq!(custom.model, "deepseek-chat");
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
                model_provider: "custom".to_string(),
            },
        };
        let s = toml::to_string(&c).unwrap();
        let back: AiConfig = toml::from_str(&s).unwrap();
        assert_eq!(c, back);
        assert_eq!(back.chat_model(), "house-model-large");
        assert_eq!(back.effective_base_url(), "https://llm.example/v1");
    }
}
