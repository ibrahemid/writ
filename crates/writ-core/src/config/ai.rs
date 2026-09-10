//! `[ai]` configuration section.
//!
//! Two opt-in surfaces reach a model: text rewriting against an
//! OpenAI-compatible endpoint, and the chat pane under `[ai.chat]` with an
//! endpoint of its own. Each is invisible until its `enabled` is set, and both
//! default to a local Ollama server, so nothing leaves the machine without an
//! explicit choice of a hosted provider. API keys are never stored here — they
//! live in the OS keychain (or in memory for the session) behind IPC commands.
//! Every field has a serde default so existing configs upgrade cleanly.

use serde::{Deserialize, Serialize};

fn default_enabled() -> bool {
    false
}

fn default_preset() -> String {
    "ollama".to_string()
}

fn default_base_url() -> String {
    "http://localhost:11434/v1".to_string()
}

fn default_model() -> String {
    String::new()
}

fn default_consented_hosts() -> Vec<String> {
    Vec::new()
}

fn default_chat_enabled() -> bool {
    false
}

fn default_chat_provider() -> String {
    "openai_compatible".to_string()
}

fn default_chat_base_url() -> String {
    "http://localhost:11434/v1".to_string()
}

fn default_chat_model() -> String {
    String::new()
}

/// Configuration for the chat pane (`[ai.chat]`).
///
/// Its own endpoint, because the model a person talks to is not always the one
/// that proofreads a paragraph. Consent is not its own: a host reached from
/// here is recorded in [`AiConfig::consented_hosts`] like any other, so one
/// answer covers both surfaces of that host and neither covers a second host.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiChatConfig {
    /// Master switch. When `false` (default) the pane, its palette entry and
    /// its network path do not exist.
    #[serde(default = "default_chat_enabled")]
    pub enabled: bool,
    /// Wire format: `anthropic` or `openai_compatible`
    /// ([`crate::chat::Provider`]).
    #[serde(default = "default_chat_provider")]
    pub provider: String,
    /// Where requests go. Defaults to the same local Ollama the rewrite path
    /// defaults to. Validated host-side before every request.
    #[serde(default = "default_chat_base_url")]
    pub base_url: String,
    /// Model id sent in each request. Empty by default; a send refuses until
    /// it is set.
    #[serde(default = "default_chat_model")]
    pub model: String,
}

impl Default for AiChatConfig {
    fn default() -> Self {
        Self {
            enabled: default_chat_enabled(),
            provider: default_chat_provider(),
            base_url: default_chat_base_url(),
            model: default_chat_model(),
        }
    }
}

fn default_chat() -> AiChatConfig {
    AiChatConfig::default()
}

/// Configuration for the opt-in rewrite feature (`[ai]`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AiConfig {
    /// Master switch. When `false` (default) no rewrite commands, UI, or
    /// network paths exist.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Provider preset id: `ollama`, `groq`, `gemini`, `deepseek`,
    /// `openrouter`, or `custom`. Drives the default `base_url` and the
    /// keychain account under which the key is stored.
    ///
    /// The chat pane's providers are not preset ids: it speaks the Messages
    /// API as well as `chat/completions`, this path speaks only the latter,
    /// and its keys live under their own namespaced accounts
    /// ([`writ_core::chat::Provider::key_account`]).
    #[serde(default = "default_preset")]
    pub preset: String,
    /// OpenAI-compatible API base, ending before `/chat/completions`. Defaults
    /// to a local Ollama server. Validated host-side before every request.
    #[serde(default = "default_base_url")]
    pub base_url: String,
    /// Model id sent in each request. Empty by default; a rewrite refuses to
    /// run until it is set.
    #[serde(default = "default_model")]
    pub model: String,
    /// Hosts (non-local) for which the one-time send notice was accepted. A
    /// hosted request refuses to run until the request's own host is listed, so
    /// consenting to one provider never covers another or a hand-edited URL.
    #[serde(default = "default_consented_hosts")]
    pub consented_hosts: Vec<String>,
    /// The chat pane's own switch, endpoint and model (`[ai.chat]`).
    #[serde(default = "default_chat")]
    pub chat: AiChatConfig,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            preset: default_preset(),
            base_url: default_base_url(),
            model: default_model(),
            consented_hosts: default_consented_hosts(),
            chat: default_chat(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_off_and_local() {
        let c = AiConfig::default();
        assert!(!c.enabled);
        assert_eq!(c.preset, "ollama");
        assert_eq!(c.base_url, "http://localhost:11434/v1");
        assert!(c.model.is_empty());
        assert!(c.consented_hosts.is_empty());
    }

    #[test]
    fn chat_is_off_and_local_until_it_is_configured() {
        let c = AiConfig::default();
        assert!(!c.chat.enabled);
        assert_eq!(c.chat.provider, "openai_compatible");
        assert_eq!(c.chat.base_url, "http://localhost:11434/v1");
        assert!(c.chat.model.is_empty());
    }

    #[test]
    fn a_config_written_before_chat_existed_upgrades() {
        let c: AiConfig =
            toml::from_str("enabled = true\npreset = \"groq\"\nmodel = \"llama-3.3-70b\"").unwrap();
        assert_eq!(c.chat, AiChatConfig::default());
    }

    #[test]
    fn chat_keeps_its_own_endpoint() {
        let c: AiConfig = toml::from_str(
            "preset = \"ollama\"\n\n[chat]\nenabled = true\nprovider = \"anthropic\"\nbase_url = \"https://api.anthropic.com\"\nmodel = \"claude-opus-5\"",
        )
        .unwrap();
        assert_eq!(c.base_url, "http://localhost:11434/v1");
        assert!(c.chat.enabled);
        assert_eq!(c.chat.provider, "anthropic");
        assert_eq!(c.chat.base_url, "https://api.anthropic.com");
    }

    #[test]
    fn empty_table_yields_defaults() {
        let c: AiConfig = toml::from_str("").unwrap();
        assert_eq!(c, AiConfig::default());
    }

    #[test]
    fn partial_table_keeps_other_defaults() {
        let c: AiConfig =
            toml::from_str("enabled = true\npreset = \"groq\"\nmodel = \"llama-3.3-70b\"").unwrap();
        assert!(c.enabled);
        assert_eq!(c.preset, "groq");
        assert_eq!(c.model, "llama-3.3-70b");
        // Untouched fields fall back to defaults.
        assert_eq!(c.base_url, "http://localhost:11434/v1");
        assert!(c.consented_hosts.is_empty());
    }

    #[test]
    fn round_trips_through_toml() {
        let c = AiConfig {
            enabled: true,
            preset: "openrouter".to_string(),
            base_url: "https://openrouter.ai/api/v1".to_string(),
            model: "openai/gpt-4o-mini".to_string(),
            consented_hosts: vec!["openrouter.ai".to_string()],
            chat: AiChatConfig {
                enabled: true,
                provider: "anthropic".to_string(),
                base_url: "https://api.anthropic.com".to_string(),
                model: "claude-opus-5".to_string(),
            },
        };
        let s = toml::to_string(&c).unwrap();
        let back: AiConfig = toml::from_str(&s).unwrap();
        assert_eq!(c, back);
    }
}
