//! `[dictation]` configuration section.
//!
//! Dictation is off on a fresh install and on every file written before the
//! section existed. `engine` is written only once the person picks one; until
//! then [`effective_engine`] decides it from what the machine supports. The
//! section holds no secrets: keys live in the key store and the hosts audio
//! may go to in `consented_hosts`, which only Rust writes.

use serde::{Deserialize, Deserializer, Serialize};

use crate::dictation::{language, presets};

/// Which engine turns speech into text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DictationEngine {
    /// An OpenAI-compatible transcription endpoint.
    Api,
    /// Apple's on-device speech model (macOS 26 and later).
    OnDevice,
}

/// Which transcription service the API engine talks to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DictationProvider {
    /// OpenAI's transcription endpoint.
    #[serde(rename = "openai")]
    OpenAi,
    /// Groq's OpenAI-compatible transcription endpoint.
    #[serde(rename = "groq")]
    Groq,
    /// Any OpenAI-compatible server at `base_url`.
    #[serde(rename = "custom")]
    Custom,
}

impl DictationProvider {
    /// The config word, which is also the provider table's row id.
    pub fn id(self) -> &'static str {
        match self {
            Self::OpenAi => "openai",
            Self::Groq => "groq",
            Self::Custom => "custom",
        }
    }
}

/// The engine a session runs on: the configured one, or, when none is
/// configured, on-device where this machine supports it and the API elsewhere.
pub fn effective_engine(
    configured: Option<DictationEngine>,
    on_device_supported: bool,
) -> DictationEngine {
    match configured {
        Some(engine) => engine,
        None if on_device_supported => DictationEngine::OnDevice,
        None => DictationEngine::Api,
    }
}

/// The dictation switch and its settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DictationConfig {
    /// The app switch (ADR-042).
    pub enabled: bool,
    /// The engine the person picked. Absent until they pick one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub engine: Option<DictationEngine>,
    /// The transcription service for the API engine.
    pub provider: DictationProvider,
    /// The server address for the `custom` provider.
    pub base_url: String,
    /// The transcription model the API engine asks for.
    pub model: String,
    /// An ISO-639-1 code from [`language::LANGUAGES`], or empty for automatic.
    #[serde(deserialize_with = "deserialize_language")]
    pub language: String,
    /// Whether each segment goes through the AI connection before insertion.
    pub clean_up: bool,
    /// Hosts the person has allowed audio to be sent to, kept apart from the
    /// hosts allowed for text. Written only by Rust.
    pub consented_hosts: Vec<String>,
}

impl Default for DictationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            engine: None,
            provider: DictationProvider::OpenAi,
            base_url: String::new(),
            model: "gpt-transcribe".to_string(),
            language: String::new(),
            clean_up: false,
            consented_hosts: Vec::new(),
        }
    }
}

impl DictationConfig {
    /// Where transcription requests go: the preset's base URL, or the typed
    /// one for `custom`.
    pub fn effective_base_url(&self) -> String {
        presets::base_url(self.provider)
            .map(str::to_string)
            .unwrap_or_else(|| self.base_url.trim().to_string())
    }

    /// The engine a session runs on ([`effective_engine`]).
    pub fn effective_engine(&self, on_device_supported: bool) -> DictationEngine {
        effective_engine(self.engine, on_device_supported)
    }
}

/// Reads `language`, refusing a code outside the list so the startup read
/// names it and keeps automatic in its place (ADR-048).
fn deserialize_language<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let raw = String::deserialize(deserializer)?;
    let code = raw.trim().to_ascii_lowercase();
    if code.is_empty() || language::language(&code).is_some() {
        Ok(code)
    } else {
        Err(serde::de::Error::custom(format!(
            "unknown dictation language `{raw}`"
        )))
    }
}
