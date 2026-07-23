//! Prompt construction and endpoint policy for text rewriting.
//!
//! Pure, framework-free logic: the set of rewrite actions, the system/user
//! message pair each one produces, and the host policy that decides whether an
//! endpoint may be contacted. The Tauri adapter parses the configured URL and
//! hands the scheme and host to [`is_endpoint_allowed`]; it never re-implements
//! the decision.

use serde::{Deserialize, Serialize};

/// Sampling temperature used for every rewrite. Low, to keep edits faithful.
pub const POLISH_TEMPERATURE: f32 = 0.3;

/// One OpenAI-compatible chat message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatMessage {
    /// `system` or `user`.
    pub role: String,
    /// Message content.
    pub content: String,
}

impl ChatMessage {
    fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".to_string(),
            content: content.into(),
        }
    }

    fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".to_string(),
            content: content.into(),
        }
    }
}

/// A rewrite the user can run over selected text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolishAction {
    /// Fix spelling, grammar, and punctuation only.
    Proofread,
    /// Restate the same meaning in different wording.
    Rephrase,
    /// Tighten and smooth while keeping meaning and voice.
    Polish,
    /// Apply a free-form user instruction to the text.
    Custom {
        /// The user's instruction.
        instruction: String,
    },
}

impl PolishAction {
    /// Parses the wire action id used by the IPC command. `custom` requires a
    /// non-empty instruction.
    pub fn parse(id: &str, custom_instruction: Option<String>) -> Result<Self, PolishError> {
        match id {
            "proofread" => Ok(Self::Proofread),
            "rephrase" => Ok(Self::Rephrase),
            "polish" => Ok(Self::Polish),
            "custom" => {
                let instruction = custom_instruction.unwrap_or_default();
                if instruction.trim().is_empty() {
                    return Err(PolishError::EmptyInstruction);
                }
                Ok(Self::Custom { instruction })
            }
            other => Err(PolishError::UnknownAction(other.to_string())),
        }
    }
}

/// Reasons a rewrite request is rejected before any network call.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PolishError {
    /// The action id did not match a known rewrite.
    #[error("unknown rewrite action: {0}")]
    UnknownAction(String),
    /// A custom rewrite was requested with no instruction.
    #[error("a custom rewrite needs an instruction")]
    EmptyInstruction,
    /// The selected text was empty or whitespace-only.
    #[error("there is no text to rewrite")]
    EmptyText,
}

const RETURN_ONLY: &str = "Return only the resulting text, with no preamble, \
explanation, notes, or code fences.";

/// Builds the system+user message pair for an action over `text`.
///
/// Errors if `text` is empty or whitespace-only.
pub fn build_messages(action: &PolishAction, text: &str) -> Result<Vec<ChatMessage>, PolishError> {
    if text.trim().is_empty() {
        return Err(PolishError::EmptyText);
    }

    let system = match action {
        PolishAction::Proofread => format!(
            "Fix spelling, grammar, and punctuation in the user's text. Keep the wording \
             and voice unchanged; do not rephrase. {RETURN_ONLY}"
        ),
        PolishAction::Rephrase => format!(
            "Rephrase the user's text to express the same meaning in different wording. \
             {RETURN_ONLY}"
        ),
        PolishAction::Polish => {
            format!("Tighten and smooth the user's text. Keep its meaning and voice. {RETURN_ONLY}")
        }
        PolishAction::Custom { instruction } => {
            format!("{}\n\n{RETURN_ONLY}", instruction.trim())
        }
    };

    Ok(vec![ChatMessage::system(system), ChatMessage::user(text)])
}

/// Whether `host` refers to the local machine.
pub fn is_localhost(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1" | "[::1]")
}

/// Whether an endpoint reaches a hosted (non-local) provider. Hosted endpoints
/// require the one-time consent and an API key.
pub fn is_hosted(host: &str) -> bool {
    !is_localhost(host)
}

/// The outbound guard. `https` is always allowed; `http` only to the local
/// machine, so a plaintext request can never leave the device.
pub fn is_endpoint_allowed(scheme: &str, host: &str) -> bool {
    match scheme {
        "https" => true,
        "http" => is_localhost(host),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proofread_builds_system_and_user() {
        let msgs = build_messages(&PolishAction::Proofread, "teh cat").unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "system");
        assert!(msgs[0].content.to_lowercase().contains("spelling"));
        assert!(msgs[0].content.contains("Return only"));
        assert_eq!(msgs[1].role, "user");
        assert_eq!(msgs[1].content, "teh cat");
    }

    #[test]
    fn rephrase_and_polish_differ() {
        let r = build_messages(&PolishAction::Rephrase, "x").unwrap();
        let p = build_messages(&PolishAction::Polish, "x").unwrap();
        assert!(r[0].content.to_lowercase().contains("rephrase"));
        assert!(p[0].content.to_lowercase().contains("tighten"));
        assert_ne!(r[0].content, p[0].content);
    }

    #[test]
    fn custom_carries_the_instruction() {
        let action = PolishAction::Custom {
            instruction: "make it formal".to_string(),
        };
        let msgs = build_messages(&action, "hey there").unwrap();
        assert!(msgs[0].content.contains("make it formal"));
        assert!(msgs[0].content.contains("Return only"));
        assert_eq!(msgs[1].content, "hey there");
    }

    #[test]
    fn empty_text_is_rejected() {
        assert_eq!(
            build_messages(&PolishAction::Proofread, "   \n"),
            Err(PolishError::EmptyText)
        );
    }

    #[test]
    fn parse_known_actions() {
        assert_eq!(
            PolishAction::parse("proofread", None).unwrap(),
            PolishAction::Proofread
        );
        assert_eq!(
            PolishAction::parse("custom", Some("shorten".to_string())).unwrap(),
            PolishAction::Custom {
                instruction: "shorten".to_string()
            }
        );
    }

    #[test]
    fn parse_rejects_unknown_and_empty_custom() {
        assert!(matches!(
            PolishAction::parse("nope", None),
            Err(PolishError::UnknownAction(_))
        ));
        assert_eq!(
            PolishAction::parse("custom", Some("  ".to_string())),
            Err(PolishError::EmptyInstruction)
        );
        assert_eq!(
            PolishAction::parse("custom", None),
            Err(PolishError::EmptyInstruction)
        );
    }

    #[test]
    fn endpoint_policy_allows_https_anywhere() {
        assert!(is_endpoint_allowed("https", "api.groq.com"));
        assert!(is_endpoint_allowed("https", "localhost"));
    }

    #[test]
    fn endpoint_policy_allows_http_only_to_loopback() {
        assert!(is_endpoint_allowed("http", "localhost"));
        assert!(is_endpoint_allowed("http", "127.0.0.1"));
        assert!(is_endpoint_allowed("http", "::1"));
        assert!(!is_endpoint_allowed("http", "api.groq.com"));
        // The classic substring-bypass hosts must not slip through.
        assert!(!is_endpoint_allowed("http", "localhost.evil.com"));
        assert!(!is_endpoint_allowed("http", "127.0.0.1.evil.com"));
    }

    #[test]
    fn endpoint_policy_rejects_other_schemes() {
        assert!(!is_endpoint_allowed("ftp", "localhost"));
        assert!(!is_endpoint_allowed("file", "localhost"));
        assert!(!is_endpoint_allowed("ws", "api.groq.com"));
    }

    #[test]
    fn hosted_classification() {
        assert!(!is_hosted("localhost"));
        assert!(!is_hosted("127.0.0.1"));
        assert!(is_hosted("api.openai.com"));
    }
}
