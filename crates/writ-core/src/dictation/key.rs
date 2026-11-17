//! Which stored key a transcription request carries.

use crate::config::dictation::{DictationConfig, DictationProvider};
use crate::config::AiConfig;

/// The account a custom server's key falls back to when its base URL names
/// no address. Nothing writes it and no request reads it: a base URL without
/// an address is refused before any key is looked up.
const CUSTOM_WITHOUT_ADDRESS: &str = "dictation.custom";

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
/// Chat or Rewrite is on (so the connection's key field is in Settings) and
/// both name the same provider at the same address (scheme, host and port);
/// otherwise it has an account of its own ([`own_account`]).
pub fn key_account(d: &DictationConfig, ai: &AiConfig) -> KeyAccount {
    let connection_shown = ai.chat.enabled || ai.rewrite.enabled;
    let same_provider = d.provider.id() == ai.provider;
    let same_address = connection_shown
        && same_provider
        && match (
            address(&d.effective_base_url()),
            address(&ai.effective_base_url()),
        ) {
            (Some(ours), Some(theirs)) => ours == theirs,
            _ => false,
        };
    if same_address {
        return KeyAccount {
            account: ai.provider.clone(),
            shared_with_ai: true,
        };
    }
    KeyAccount {
        account: own_account(d).unwrap_or_else(|| CUSTOM_WITHOUT_ADDRESS.to_string()),
        shared_with_ai: false,
    }
}

/// Dictation's own account for `d`: one per preset provider, and one per
/// address for a custom server (`dictation.custom.https://host:port`), so a
/// key typed for one server never goes to another. `None` for a custom base
/// URL that names no address.
pub fn own_account(d: &DictationConfig) -> Option<String> {
    match d.provider {
        DictationProvider::OpenAi => Some("dictation.openai".to_string()),
        DictationProvider::Groq => Some("dictation.groq".to_string()),
        DictationProvider::Custom => address(&d.effective_base_url())
            .map(|address| format!("{CUSTOM_WITHOUT_ADDRESS}.{address}")),
    }
}

/// The address `base_url` names: its scheme, host and port, with the port
/// left out when it is the scheme's default.
pub fn address(base_url: &str) -> Option<String> {
    let url = url::Url::parse(base_url.trim()).ok()?;
    let host = url.host_str()?;
    Some(match url.port() {
        Some(port) => format!("{}://{host}:{port}", url.scheme()),
        None => format!("{}://{host}", url.scheme()),
    })
}
