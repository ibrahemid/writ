//! The transcription services Settings offers by name.

use crate::ai::providers;
use crate::config::dictation::DictationProvider;

/// One named transcription service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DictationPreset {
    /// The config word and the provider table's row id.
    pub id: &'static str,
    /// The name Settings shows.
    pub label: &'static str,
    /// The model a new setup starts with.
    pub model: &'static str,
}

/// OpenAI and Groq. `custom` is any other OpenAI-compatible server.
pub const PRESETS: [DictationPreset; 2] = [
    DictationPreset {
        id: "openai",
        label: "OpenAI",
        model: "gpt-transcribe",
    },
    DictationPreset {
        id: "groq",
        label: "Groq",
        model: "whisper-large-v3-turbo",
    },
];

/// The preset for `provider`; `None` for `custom`.
pub fn preset(provider: DictationProvider) -> Option<&'static DictationPreset> {
    PRESETS.iter().find(|p| p.id == provider.id())
}

/// The preset's base URL, read from the AI provider table's row.
pub fn base_url(provider: DictationProvider) -> Option<&'static str> {
    preset(provider)
        .and_then(|p| providers::provider(p.id))
        .map(|row| row.base_url)
        .filter(|url| !url.is_empty())
}

/// Where the preset's keys are made, read from the AI provider table's row.
pub fn key_page(provider: DictationProvider) -> Option<&'static str> {
    preset(provider)
        .and_then(|p| providers::provider(p.id))
        .and_then(|row| row.key_page_url)
}
