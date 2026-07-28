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
    /// Rewrite the text into a clearer instruction for a language model.
    ImprovePrompt,
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
            "improve_prompt" => Ok(Self::ImprovePrompt),
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
///
/// Every variant carries the text shown to the user: the IPC layer converts
/// with `to_string()` and adds nothing, so the wording lives here beside the
/// policy that produces it.
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
    /// The feature's master switch is off.
    #[error("Rewriting is turned off.")]
    Disabled,
    /// The configured base URL did not parse, or carried no host.
    #[error("The base URL is not a valid URL.")]
    InvalidBaseUrl,
    /// The scheme/host pair failed the outbound guard.
    #[error("This base URL is not allowed. Use https, or http for localhost.")]
    EndpointNotAllowed,
    /// No model id is configured.
    #[error("Choose a model in AI settings.")]
    ModelRequired,
    /// The one-time send notice for this host has not been accepted.
    #[error("Confirm sending text to {host} first.")]
    ConsentRequired {
        /// The host awaiting consent, as stored in `consented_hosts`.
        host: String,
    },
    /// No API key is stored for the hosted provider.
    #[error("Add an API key for {host} first.")]
    ApiKeyRequired {
        /// The host the missing key belongs to.
        host: String,
    },
}

/// A configured endpoint, resolved once and reused by every caller that needs
/// to know where a request would go.
///
/// This is the single authority for turning a configured `base_url` into a
/// host: the rewrite guard, the consent recorder, and the connection probe all
/// go through it, so the string written into `consented_hosts` is always the
/// string the guard later checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointTarget {
    /// Host alone, lowercased by the URL parser. The `consented_hosts` key.
    pub host: String,
    /// Host with `:port` when the URL carries one. Display and probe detail
    /// only — never the consent key, so moving a local server between ports
    /// cannot silently re-use another host's consent.
    pub host_port: String,
    /// The endpoint reaches somewhere other than this machine.
    pub is_hosted: bool,
    /// The scheme/host pair passes the outbound guard.
    pub is_allowed: bool,
}

/// Resolves a configured base URL into its [`EndpointTarget`].
///
/// Errors only when the URL is unparseable or host-less; a well-formed URL that
/// fails the outbound guard resolves successfully with `is_allowed` false, so
/// callers can report *which* endpoint was refused.
pub fn resolve_endpoint(base_url: &str) -> Result<EndpointTarget, PolishError> {
    let url = url::Url::parse(base_url.trim()).map_err(|_| PolishError::InvalidBaseUrl)?;
    let host = url
        .host_str()
        .ok_or(PolishError::InvalidBaseUrl)?
        .to_string();
    let host_port = match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.clone(),
    };
    let is_allowed = is_endpoint_allowed(url.scheme(), &host);
    Ok(EndpointTarget {
        is_hosted: is_hosted(&host),
        is_allowed,
        host,
        host_port,
    })
}

const RETURN_ONLY: &str = "Return only the resulting text, with no preamble, \
explanation, notes, or code fences.";

/// Placeholder contract for [`PolishAction::ImprovePrompt`].
///
/// Writ's prompt workbench fills `{{name}}` slots (see
/// [`crate::prompt::placeholder`]); a rewrite that paraphrased them into prose
/// would silently destroy a prompt buffer, so the instruction is explicit and
/// asserted by a test.
const KEEP_PLACEHOLDERS: &str = "Reproduce every {{placeholder}} token exactly as written, \
including the doubled braces and any \\{{ escape. Never rename, translate, fill in, or remove one.";

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
        PolishAction::ImprovePrompt => format!(
            "Rewrite the user's text as a clearer instruction for a language model. State the \
             task, the expected output, and any constraints the text implies. Keep every \
             requirement the original asks for and invent none. {KEEP_PLACEHOLDERS} {RETURN_ONLY}"
        ),
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

    #[test]
    fn improve_prompt_keeps_placeholders_verbatim() {
        let msgs = build_messages(&PolishAction::ImprovePrompt, "summarise {{doc}}").unwrap();
        let system = &msgs[0].content;
        // The placeholder contract is load-bearing: without it a rewrite can
        // paraphrase `{{doc}}` away and break the prompt workbench.
        assert!(system.contains("{{placeholder}}"));
        assert!(system.contains("\\{{"));
        assert!(system.to_lowercase().contains("language model"));
        assert!(system.contains("Return only"));
        assert_eq!(msgs[1].content, "summarise {{doc}}");
    }

    #[test]
    fn improve_prompt_differs_from_the_other_actions() {
        let improve = build_messages(&PolishAction::ImprovePrompt, "x").unwrap();
        for other in [
            PolishAction::Proofread,
            PolishAction::Rephrase,
            PolishAction::Polish,
        ] {
            let msgs = build_messages(&other, "x").unwrap();
            assert_ne!(improve[0].content, msgs[0].content);
            // Only the prompt action carries the placeholder contract.
            assert!(!msgs[0].content.contains("{{placeholder}}"));
        }
    }

    #[test]
    fn parse_accepts_improve_prompt() {
        assert_eq!(
            PolishAction::parse("improve_prompt", None).unwrap(),
            PolishAction::ImprovePrompt
        );
    }

    #[test]
    fn resolve_endpoint_splits_host_and_port() {
        let t = resolve_endpoint("http://localhost:11434/v1").unwrap();
        assert_eq!(t.host, "localhost");
        assert_eq!(t.host_port, "localhost:11434");
        assert!(!t.is_hosted);
        assert!(t.is_allowed);

        let t = resolve_endpoint("https://api.deepseek.com/v1").unwrap();
        assert_eq!(t.host, "api.deepseek.com");
        // No explicit port, so the two forms agree.
        assert_eq!(t.host_port, "api.deepseek.com");
        assert!(t.is_hosted);
        assert!(t.is_allowed);
    }

    #[test]
    fn resolve_endpoint_reports_a_refused_endpoint_rather_than_erroring() {
        // A well-formed URL that fails the guard still resolves, so the caller
        // can name the host it refused.
        let t = resolve_endpoint("http://api.groq.com/v1").unwrap();
        assert_eq!(t.host, "api.groq.com");
        assert!(!t.is_allowed);
        assert!(t.is_hosted);
    }

    #[test]
    fn resolve_endpoint_normalizes_the_consent_key() {
        // Whitespace, case and a default port must not produce three different
        // keys for one provider, or consent would never stick.
        for raw in [
            "  https://API.DeepSeek.com/v1  ",
            "https://api.deepseek.com/v1/",
            "https://api.deepseek.com:443/v1",
        ] {
            assert_eq!(resolve_endpoint(raw).unwrap().host, "api.deepseek.com");
        }
    }

    #[test]
    fn resolve_endpoint_handles_ipv6_loopback() {
        let t = resolve_endpoint("http://[::1]:8080/v1").unwrap();
        assert_eq!(t.host, "[::1]");
        assert_eq!(t.host_port, "[::1]:8080");
        assert!(!t.is_hosted);
        assert!(t.is_allowed);
    }

    #[test]
    fn resolve_endpoint_rejects_unparseable_and_hostless_urls() {
        assert_eq!(
            resolve_endpoint("not a url"),
            Err(PolishError::InvalidBaseUrl)
        );
        assert_eq!(
            resolve_endpoint("file:///etc/passwd"),
            Err(PolishError::InvalidBaseUrl)
        );
    }

    #[test]
    fn consent_and_key_errors_name_the_host() {
        let consent = PolishError::ConsentRequired {
            host: "api.deepseek.com".to_string(),
        };
        let key = PolishError::ApiKeyRequired {
            host: "api.deepseek.com".to_string(),
        };
        // The old messages named no host, which is what made the failure a
        // dead end for the user.
        assert!(consent.to_string().contains("api.deepseek.com"));
        assert!(key.to_string().contains("api.deepseek.com"));
    }
}
