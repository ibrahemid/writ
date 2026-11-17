//! Which stored key a transcription request carries.

use crate::config::dictation::{DictationConfig, DictationProvider};
use crate::config::AiConfig;
use crate::polish;

/// The key store account dictation reads its key from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyAccount {
    /// The account name in the key store.
    pub account: String,
    /// The account is the AI connection's, so dictation reads it and never
    /// writes it.
    pub shared_with_ai: bool,
}

/// The account for `d`'s key. Dictation shares the AI connection's key when
/// both name the same provider and resolve to the same host; otherwise it has
/// an account of its own per provider.
pub fn key_account(d: &DictationConfig, ai: &AiConfig) -> KeyAccount {
    let same_provider = d.provider.id() == ai.provider;
    let same_host = same_provider
        && match (
            polish::resolve_endpoint(&d.effective_base_url()),
            polish::resolve_endpoint(&ai.effective_base_url()),
        ) {
            (Ok(ours), Ok(theirs)) => ours.host_port == theirs.host_port,
            _ => false,
        };
    if same_host {
        return KeyAccount {
            account: ai.provider.clone(),
            shared_with_ai: true,
        };
    }
    KeyAccount {
        account: own_account(d.provider).to_string(),
        shared_with_ai: false,
    }
}

/// Dictation's own account for `provider`.
pub fn own_account(provider: DictationProvider) -> &'static str {
    match provider {
        DictationProvider::OpenAi => "dictation.openai",
        DictationProvider::Groq => "dictation.groq",
        DictationProvider::Custom => "dictation.custom",
    }
}
