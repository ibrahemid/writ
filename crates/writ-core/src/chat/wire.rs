//! The request body one send carries and what one line of its stream means.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::*;
use crate::ai::providers::{BodyRules, Wire};

/// The version of the Anthropic Messages API this module is written against.
pub const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Output ceiling for one reply, on either wire.
///
/// Anthropic requires the field. The OpenAI-compatible servers do not, and
/// their defaults are small enough to cut a whole-note reply in half: a reply
/// that carries a note back has to fit inside this, so both bodies carry it.
///
/// On `api.openai.com` the ceiling also counts a reasoning model's reasoning
/// tokens, so a long run can end the reply as truncated, at worst before any
/// text. OpenAI's reasoning guide suggests 25,000 to start with; the gpt-4o
/// models still offered take at most 16,384, so the ceiling stays under that.
pub const MAX_REPLY_TOKENS: u32 = 16_000;

/// Which wire format the configured endpoint speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    /// The Anthropic Messages API: `POST /v1/messages`, `x-api-key` and
    /// `anthropic-version` headers, `content_block_delta` frames.
    Anthropic,
    /// `POST /chat/completions` with `delta.content` frames: a local model
    /// through Ollama, and most hosted providers.
    OpenAiCompatible,
}

impl Provider {
    /// Parses the id stored in `config.toml`.
    pub fn parse(id: &str) -> Result<Self, ChatError> {
        match id.trim() {
            "anthropic" => Ok(Self::Anthropic),
            "openai_compatible" => Ok(Self::OpenAiCompatible),
            other => Err(ChatError::UnknownProvider(other.to_string())),
        }
    }

    /// The id as `config.toml` spells it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Anthropic => "anthropic",
            Self::OpenAiCompatible => "openai_compatible",
        }
    }

    /// The wire a provider row speaks, as this module names it.
    ///
    /// The connection stores a provider id from
    /// [`crate::ai::providers::PROVIDERS`], and the table says which wire that
    /// row answers on. This is the one conversion, so a row added to the table
    /// reaches both features without a second match arm anywhere.
    pub fn from_wire(wire: Wire) -> Self {
        match wire {
            Wire::Anthropic => Self::Anthropic,
            Wire::OpenAi => Self::OpenAiCompatible,
        }
    }
}

/// Who said one turn of the conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    /// The person typing in the pane.
    User,
    /// The model.
    Assistant,
}

impl Role {
    fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
        }
    }
}

/// One turn of the conversation, as the pane holds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatTurn {
    /// Who said it.
    pub role: Role,
    /// What was said.
    pub content: String,
}

/// A parameter Writ writes into a request body.
///
/// Named so a refusal can say which parameter a model did not take. Only the
/// parameters Writ itself sends are here, so the name a sentence carries is
/// always Writ's own word, matched against the envelope's `param`, and never
/// the host's text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestParam {
    /// `model`.
    Model,
    /// `messages`.
    Messages,
    /// `system`, the Messages API's instruction field.
    System,
    /// `stream`.
    Stream,
    /// `max_tokens`.
    MaxTokens,
    /// `max_completion_tokens`.
    MaxCompletionTokens,
    /// `temperature`.
    Temperature,
}

impl RequestParam {
    /// The parameter's name as the body carries it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Model => "model",
            Self::Messages => "messages",
            Self::System => "system",
            Self::Stream => "stream",
            Self::MaxTokens => "max_tokens",
            Self::MaxCompletionTokens => "max_completion_tokens",
            Self::Temperature => "temperature",
        }
    }

    /// The parameter a refusal names, when it is one Writ sends.
    fn from_token(token: &str) -> Option<Self> {
        [
            Self::Model,
            Self::Messages,
            Self::System,
            Self::Stream,
            Self::MaxTokens,
            Self::MaxCompletionTokens,
            Self::Temperature,
        ]
        .into_iter()
        .find(|param| param.as_str() == token)
    }
}

/// A reason a provider gave for refusing a request, when it is one of the
/// eight Writ has a sentence for.
///
/// The enum is the whole of what may be taken out of a refusal body. Nothing
/// carries the provider's own words, so "no response text is shown, stored or
/// logged" is a property of the type rather than a habit of its callers
/// (ADR-031 rule 5.2, narrowed by the ADR-040 amendments of 2026-09-17 and
/// 2026-10-13). The two that name a parameter carry a [`RequestParam`], which
/// is itself a closed set. It serializes as its word, [`RejectCode::as_str`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectCode {
    /// The model id is not one the provider serves.
    ModelNotFound,
    /// The request was not accepted as written.
    InvalidRequest,
    /// The key was refused.
    InvalidApiKey,
    /// The account has no credit left.
    InsufficientQuota,
    /// Too many requests, too quickly.
    RateLimited,
    /// The conversation is longer than the model takes.
    ContextLengthExceeded,
    /// The model does not take a parameter the request carried.
    UnsupportedParameter(Option<RequestParam>),
    /// The model does not take the value one parameter was given.
    UnsupportedValue(Option<RequestParam>),
}

impl RejectCode {
    /// The clause Writ writes for this reason, in Writ's words.
    pub fn reason(self) -> String {
        match self {
            Self::ModelNotFound => "the model does not exist".to_string(),
            Self::InvalidRequest => "the request was not accepted".to_string(),
            Self::InvalidApiKey => "the API key was not accepted".to_string(),
            Self::InsufficientQuota => "the account is out of credit".to_string(),
            Self::RateLimited => "too many requests were sent".to_string(),
            Self::ContextLengthExceeded => "the conversation is too long".to_string(),
            Self::UnsupportedParameter(Some(param)) => {
                format!("the model does not accept the {} parameter", param.as_str())
            }
            Self::UnsupportedParameter(None) => {
                "the model does not accept a parameter the request carried".to_string()
            }
            Self::UnsupportedValue(Some(param)) => {
                format!(
                    "the model does not accept the value given for {}",
                    param.as_str()
                )
            }
            Self::UnsupportedValue(None) => {
                "the model does not accept a value the request carried".to_string()
            }
        }
    }

    /// The word this reason is logged under. A fixed token, never the
    /// provider's text.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ModelNotFound => "model_not_found",
            Self::InvalidRequest => "invalid_request",
            Self::InvalidApiKey => "invalid_api_key",
            Self::InsufficientQuota => "insufficient_quota",
            Self::RateLimited => "rate_limited",
            Self::ContextLengthExceeded => "context_length_exceeded",
            Self::UnsupportedParameter(_) => "unsupported_parameter",
            Self::UnsupportedValue(_) => "unsupported_value",
        }
    }

    /// The reason a token names, or `None` when it is not on the allowlist.
    /// `param` is the parameter the envelope named, already narrowed to one
    /// Writ sends.
    fn from_token(token: &str, param: Option<RequestParam>) -> Option<Self> {
        match token {
            "model_not_found" | "not_found_error" => Some(Self::ModelNotFound),
            "invalid_request_error" | "invalid_request" => Some(Self::InvalidRequest),
            "invalid_api_key" | "authentication_error" => Some(Self::InvalidApiKey),
            "insufficient_quota" => Some(Self::InsufficientQuota),
            "rate_limit_exceeded" | "rate_limit_error" => Some(Self::RateLimited),
            "context_length_exceeded" => Some(Self::ContextLengthExceeded),
            "unsupported_parameter" => Some(Self::UnsupportedParameter(param)),
            "unsupported_value" => Some(Self::UnsupportedValue(param)),
            _ => None,
        }
    }
}

impl Serialize for RejectCode {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// Reads the reason out of a refusal envelope.
///
/// Both shapes carry it under `error`: OpenAI-compatible hosts in `code` with
/// the family in `type`, Anthropic in `type` alone. The narrower field is read
/// first. `param` is read only to be matched against the parameters Writ
/// sends. Everything else in the body, the provider's own sentence included,
/// is never looked at.
pub fn parse_reject_code(body: &str) -> Option<RejectCode> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    let error = value.get("error")?;
    let param = error
        .get("param")
        .and_then(serde_json::Value::as_str)
        .and_then(RequestParam::from_token);
    ["code", "type"]
        .into_iter()
        .filter_map(|field| error.get(field)?.as_str())
        .find_map(|token| RejectCode::from_token(token, param))
}

/// The label the provider table gives an id, or the id when it is not ours.
fn provider_label(id: &str) -> &str {
    crate::ai::providers::provider(id)
        .map(|row| row.label)
        .unwrap_or(id)
}

/// The sentence a refusal reads as. Writ's words around a status and, when the
/// allowlist matched, one clause naming the reason.
///
/// Chat and Rewrite both say it, so one host's refusal reads the same in
/// either.
pub fn reject_sentence(provider: &str, status: u16, code: Option<RejectCode>) -> String {
    let label = provider_label(provider);
    match code {
        Some(code) => format!(
            "{label} rejected the request ({status}): {}.",
            code.reason()
        ),
        None => format!("{label} rejected the request ({status})."),
    }
}

/// Who a request was sent as: the connection it was frozen from.
///
/// Captured once when the request is built and carried on every frame it
/// produces, so a reply and a refusal both name the model that answered rather
/// than whatever the config says by the time they land.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestIdentity {
    /// The provider id the request went to.
    pub provider: String,
    /// The model id that was sent.
    pub model: String,
    /// The host it was sent to.
    pub host: String,
}

/// Reasons a chat request is refused before any network call.
///
/// Every variant carries the text the user reads, so the wording lives beside
/// the policy that produces it. Nothing here can hold a key, a prompt or a
/// reply (ADR-031 rule 5.2).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ChatError {
    /// The feature's switch is off.
    #[error("Chat is turned off.")]
    Disabled,
    /// The configured provider id is not one this build speaks.
    #[error("Choose a provider in AI settings.")]
    UnknownProvider(String),
    /// The configured base URL did not parse, or carried no host.
    #[error("The chat base URL is not a valid URL.")]
    InvalidBaseUrl,
    /// The scheme/host pair failed the outbound guard.
    #[error("This base URL is not allowed. Use https, or http for localhost.")]
    EndpointNotAllowed,
    /// No model id is configured.
    #[error("Choose a chat model in AI settings.")]
    ModelRequired,
    /// The send notice for this host has not been accepted.
    #[error("Confirm sending your files to {host} first.")]
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
    /// The message had no text in it.
    #[error("there is nothing to send")]
    EmptyMessage,
    /// The local runtime the connection points at is not answering.
    #[error("{} is not running at {host_port}.", provider_label(runtime))]
    LocalServerOffline {
        /// The provider id whose label names the runtime.
        runtime: String,
        /// Where it was expected to answer.
        host_port: String,
    },
    /// The provider's own list came back empty, so there is nothing to send.
    #[error("{} listed no models.", provider_label(provider))]
    EmptyModelList {
        /// The provider id that listed nothing.
        provider: String,
    },
    /// The chosen model is not in the list the provider answered for this
    /// account.
    #[error("{model} is not available on {}.", provider_label(provider))]
    ModelUnavailable {
        /// The model id that would have been sent.
        model: String,
        /// The provider id that does not list it.
        provider: String,
    },
    /// The provider answered a non-2xx status.
    #[error("{}", reject_sentence(provider, *status, *code))]
    ProviderRejected {
        /// The provider id that refused.
        provider: String,
        /// The HTTP status it answered.
        status: u16,
        /// The reason it named, when that reason is on the allowlist.
        code: Option<RejectCode>,
    },
}

impl ChatError {
    /// The machine word the pane routes its recovery on. The sentence is what
    /// a person reads; this is what the code matches.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::UnknownProvider(_) => "unknown_provider",
            Self::InvalidBaseUrl => "invalid_base_url",
            Self::EndpointNotAllowed => "endpoint_not_allowed",
            Self::ModelRequired => "model_required",
            Self::ConsentRequired { .. } => "consent_required",
            Self::ApiKeyRequired { .. } => "api_key_required",
            Self::EmptyMessage => "empty_message",
            Self::LocalServerOffline { .. } => "local_server_offline",
            Self::EmptyModelList { .. } => "empty_model_list",
            Self::ModelUnavailable { .. } => "model_unavailable",
            Self::ProviderRejected { .. } => "provider_rejected",
        }
    }

    /// The status a provider answered, for the failure that carries one.
    pub fn status(&self) -> Option<u16> {
        match self {
            Self::ProviderRejected { status, .. } => Some(*status),
            _ => None,
        }
    }
}

/// What the pane is told when a reply fails.
///
/// `message` is Writ's own sentence, built from the typed failure; no part of
/// a response body reaches this struct. `provider` and `model` are the
/// connection the request was frozen against, so a refusal names the model
/// that was refused rather than whatever is configured by the time it lands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChatErrorFrame {
    /// The machine word, from [`ChatError::kind`].
    pub kind: String,
    /// The sentence a person reads.
    pub message: String,
    /// The provider the request went to.
    pub provider: String,
    /// The model that was sent.
    pub model: String,
    /// The status the provider answered, when it answered one.
    pub status: Option<u16>,
}

impl ChatErrorFrame {
    /// The frame a typed failure reads as.
    pub fn from_error(error: &ChatError, identity: &RequestIdentity) -> Self {
        Self {
            kind: error.kind().to_string(),
            message: error.to_string(),
            provider: identity.provider.clone(),
            model: identity.model.clone(),
            status: error.status(),
        }
    }

    /// A frame for a failure with no typed variant of its own: a transport
    /// fault, or a stream the host ended. `message` is still Writ's sentence.
    pub fn untyped(kind: &str, message: String, identity: &RequestIdentity) -> Self {
        Self {
            kind: kind.to_string(),
            message,
            provider: identity.provider.clone(),
            model: identity.model.clone(),
            status: None,
        }
    }
}

impl From<crate::polish::PolishError> for ChatError {
    fn from(error: crate::polish::PolishError) -> Self {
        match error {
            crate::polish::PolishError::EndpointNotAllowed => Self::EndpointNotAllowed,
            _ => Self::InvalidBaseUrl,
        }
    }
}

/// The endpoint one request goes to.
///
/// The base URL is what the user configured, so both spellings of the
/// Anthropic base (`https://api.anthropic.com` and `.../v1`) reach
/// `/v1/messages` once.
pub fn endpoint(provider: Provider, base_url: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    match provider {
        Provider::Anthropic => match base.ends_with("/v1") {
            true => format!("{base}/messages"),
            false => format!("{base}/v1/messages"),
        },
        Provider::OpenAiCompatible => format!("{base}/chat/completions"),
    }
}

/// Builds the JSON body for one request.
///
/// The attached notes lead as their own `user` message rather than being
/// folded into the system prompt: the system prompt is the instruction and the
/// notes are input, and both wire formats accept two user turns in a row.
///
/// For [`Provider::Anthropic`] the system prompt is the top-level `system`
/// field, outside `messages`. For [`Provider::OpenAiCompatible`] it is the
/// first message, which is the shape [`crate::polish::build_messages`] already
/// produces for a rewrite.
///
/// `rules` is how the request's host wants the body written
/// ([`crate::ai::providers::body_rules`]): the ceiling goes under the field it
/// reads, so OpenAI's reasoning models are sent `max_completion_tokens`. The
/// chat sends no temperature anywhere.
pub fn build_request_body(
    provider: Provider,
    rules: BodyRules,
    model: &str,
    system: &str,
    turns: &[ChatTurn],
    context: &[AttachedNote],
) -> Value {
    let mut messages: Vec<Value> = Vec::with_capacity(turns.len() + 2);
    if provider == Provider::OpenAiCompatible {
        messages.push(json!({ "role": "system", "content": system }));
    }
    let notes = context_block(context);
    if !notes.is_empty() {
        messages.push(json!({ "role": "user", "content": notes }));
    }
    for turn in turns {
        messages.push(json!({ "role": turn.role.as_str(), "content": turn.content }));
    }

    let mut body = match provider {
        Provider::Anthropic => json!({
            "model": model.trim(),
            "stream": true,
            "system": system,
            "messages": messages,
        }),
        Provider::OpenAiCompatible => json!({
            "model": model.trim(),
            "stream": true,
            "messages": messages,
        }),
    };
    body[rules.ceiling.as_str()] = json!(MAX_REPLY_TOKENS);
    body
}

/// What one line of a stream turned out to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delta {
    /// Text to append to the reply.
    Text(String),
    /// The reply is complete.
    Done,
    /// The reply ended because it reached the token ceiling, so what arrived
    /// is the start of a reply rather than the whole of one.
    ///
    /// The caller treats it as the end of the stream and says so: a proposal
    /// cut off mid-note is dropped as unterminated, and a reply that stops
    /// mid-sentence with no word about it reads as a broken feature.
    Truncated,
    /// The provider reported a failure mid-stream.
    ///
    /// Nothing the server wrote comes out with it. The frame's `type` and
    /// `message` are both response text a host chooses, so neither may be
    /// shown and neither may be logged (ADR-031 rule 5.2); a caller has the
    /// fact that the stream failed, which is all it can act on.
    Failed,
    /// A keep-alive, a frame carrying no text, or a line that did not parse.
    Ignore,
}

/// Reads one already-trimmed SSE line.
///
/// Pure over the line, so both grammars are asserted against recorded frames.
/// Anything unrecognised is [`Delta::Ignore`]: a malformed line yields no text
/// and does not stop the stream.
pub fn parse_delta(provider: Provider, line: &str) -> Delta {
    let Some(rest) = line.strip_prefix("data:") else {
        return Delta::Ignore;
    };
    let payload = rest.trim();
    if payload.is_empty() {
        return Delta::Ignore;
    }
    match provider {
        Provider::Anthropic => parse_anthropic_payload(payload),
        Provider::OpenAiCompatible => parse_openai_payload(payload),
    }
}

fn parse_anthropic_payload(payload: &str) -> Delta {
    let Ok(value) = serde_json::from_str::<Value>(payload) else {
        return Delta::Ignore;
    };
    match value.get("type").and_then(Value::as_str) {
        Some("content_block_delta") => {
            let delta = value.get("delta");
            let is_text = delta
                .and_then(|d| d.get("type"))
                .and_then(Value::as_str)
                .is_some_and(|kind| kind == "text_delta");
            let text = delta.and_then(|d| d.get("text")).and_then(Value::as_str);
            match (is_text, text) {
                (true, Some(text)) if !text.is_empty() => Delta::Text(text.to_string()),
                _ => Delta::Ignore,
            }
        }
        // The stop reason arrives on `message_delta`, before `message_stop`.
        Some("message_delta") => match value
            .get("delta")
            .and_then(|delta| delta.get("stop_reason"))
            .and_then(Value::as_str)
        {
            Some("max_tokens") => Delta::Truncated,
            _ => Delta::Ignore,
        },
        Some("message_stop") => Delta::Done,
        Some("error") => Delta::Failed,
        _ => Delta::Ignore,
    }
}

fn parse_openai_payload(payload: &str) -> Delta {
    if payload == "[DONE]" {
        return Delta::Done;
    }
    let Ok(value) = serde_json::from_str::<Value>(payload) else {
        return Delta::Ignore;
    };
    // Text first. Plenty of servers carry `"error": null` on every chunk
    // because it is in their response schema, and a chunk that says something
    // has said it whatever else the frame carries.
    let content = value
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("delta"))
        .and_then(|d| d.get("content"))
        .and_then(Value::as_str);
    if let Some(text) = content {
        if !text.is_empty() {
            return Delta::Text(text.to_string());
        }
    }
    // The ceiling was reached. Read after the text because a server that
    // carries both in one frame has still said something; every server this
    // is written against sends the reason in a frame of its own.
    if value
        .get("choices")
        .and_then(|choices| choices.get(0))
        .and_then(|choice| choice.get("finish_reason"))
        .and_then(Value::as_str)
        .is_some_and(|reason| reason == "length")
    {
        return Delta::Truncated;
    }
    // A server that fails mid-stream sends one of these and closes. Without
    // this arm the reply reads as complete and empty, which is a failed
    // request the pane cannot tell from a model with nothing to say.
    //
    // Any shape that is not `null` counts: the field is an object in the
    // OpenAI spelling and a bare string in several servers that copy it, and
    // `null` is what a server writes when nothing went wrong.
    if value
        .get("error")
        .is_some_and(|reported| !reported.is_null())
    {
        return Delta::Failed;
    }
    Delta::Ignore
}

#[cfg(test)]
mod reject_tests {
    use super::*;

    #[test]
    fn the_openai_envelope_is_read_from_its_code_then_its_type() {
        // OpenAI names the model failure in `code` and the family in `type`;
        // the narrower field wins.
        assert_eq!(
            parse_reject_code(
                r#"{"error":{"message":"The model does not exist","type":"invalid_request_error","code":"model_not_found"}}"#
            ),
            Some(RejectCode::ModelNotFound)
        );
        // DeepSeek answers the same shape with the family in both fields.
        assert_eq!(
            parse_reject_code(
                r#"{"error":{"message":"Model Not Exist","type":"invalid_request_error","code":"invalid_request_error"}}"#
            ),
            Some(RejectCode::InvalidRequest)
        );
        assert_eq!(
            parse_reject_code(r#"{"error":{"code":"insufficient_quota"}}"#),
            Some(RejectCode::InsufficientQuota)
        );
        assert_eq!(
            parse_reject_code(r#"{"error":{"code":"context_length_exceeded"}}"#),
            Some(RejectCode::ContextLengthExceeded)
        );
    }

    #[test]
    fn the_anthropic_envelope_is_read_from_its_type() {
        assert_eq!(
            parse_reject_code(
                r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#
            ),
            Some(RejectCode::InvalidApiKey)
        );
        assert_eq!(
            parse_reject_code(
                r#"{"type":"error","error":{"type":"rate_limit_error","message":"slow down"}}"#
            ),
            Some(RejectCode::RateLimited)
        );
        assert_eq!(
            parse_reject_code(
                r#"{"type":"error","error":{"type":"not_found_error","message":"model: nope"}}"#
            ),
            Some(RejectCode::ModelNotFound)
        );
    }

    #[test]
    fn an_unsupported_parameter_is_named_in_the_sentence() {
        let parameter = r#"{"error":{"message":"Unsupported parameter: 'max_tokens' is not supported with this model. Use 'max_completion_tokens' instead.","type":"invalid_request_error","param":"max_tokens","code":"unsupported_parameter"}}"#;
        let refused = ChatError::ProviderRejected {
            provider: "openai".to_string(),
            status: 400,
            code: parse_reject_code(parameter),
        };
        assert_eq!(
            refused.to_string(),
            "OpenAI rejected the request (400): the model does not accept the max_tokens parameter."
        );

        let value = r#"{"error":{"message":"Unsupported value: 'temperature' does not support 0.3 with this model. Only the default (1) value is supported.","type":"invalid_request_error","param":"temperature","code":"unsupported_value"}}"#;
        let refused = ChatError::ProviderRejected {
            provider: "openai".to_string(),
            status: 400,
            code: parse_reject_code(value),
        };
        assert_eq!(
            refused.to_string(),
            "OpenAI rejected the request (400): the model does not accept the value given for temperature."
        );

        // A parameter Writ never sends is the host's own text, so it is not
        // repeated: the sentence says a parameter was refused and stops.
        let unknown = r#"{"error":{"type":"invalid_request_error","param":"ZZ-server-text","code":"unsupported_parameter"}}"#;
        let refused = ChatError::ProviderRejected {
            provider: "openai".to_string(),
            status: 400,
            code: parse_reject_code(unknown),
        };
        assert_eq!(
            refused.to_string(),
            "OpenAI rejected the request (400): the model does not accept a parameter the request carried."
        );
        assert!(!refused.to_string().contains("ZZ-server-text"));
    }

    #[test]
    fn a_reason_serializes_as_the_word_it_is_logged_under() {
        for code in [
            RejectCode::ModelNotFound,
            RejectCode::InvalidApiKey,
            RejectCode::UnsupportedParameter(Some(RequestParam::Temperature)),
            RejectCode::UnsupportedParameter(None),
            RejectCode::UnsupportedValue(Some(RequestParam::MaxTokens)),
        ] {
            assert_eq!(
                serde_json::to_value(code).expect("serializes"),
                json!(code.as_str())
            );
        }
    }

    #[test]
    fn anything_else_is_no_code_at_all() {
        // A word nobody allowed, a body that is not JSON, an envelope without
        // an error, and an empty body all answer the same: the status is the
        // whole of what may be said.
        for body in [
            r#"{"error":{"code":"teapot","message":"ZZ-server-text"}}"#,
            "not json at all",
            r#"{"ok":true}"#,
            "",
        ] {
            assert_eq!(parse_reject_code(body), None, "{body}");
        }
    }

    #[test]
    fn a_rejection_reads_as_writs_own_sentence() {
        let refused = ChatError::ProviderRejected {
            provider: "deepseek".to_string(),
            status: 400,
            code: Some(RejectCode::ModelNotFound),
        };
        assert_eq!(
            refused.to_string(),
            "DeepSeek rejected the request (400): the model does not exist."
        );

        // With no code in the allowlist the status stands alone.
        let bare = ChatError::ProviderRejected {
            provider: "deepseek".to_string(),
            status: 503,
            code: None,
        };
        assert_eq!(bare.to_string(), "DeepSeek rejected the request (503).");

        assert_eq!(
            ChatError::ModelUnavailable {
                model: "qwen2.5-coder:0.5b".to_string(),
                provider: "deepseek".to_string(),
            }
            .to_string(),
            "qwen2.5-coder:0.5b is not available on DeepSeek."
        );
        assert_eq!(
            ChatError::EmptyModelList {
                provider: "lmstudio".to_string(),
            }
            .to_string(),
            "LM Studio listed no models."
        );
        assert_eq!(
            ChatError::LocalServerOffline {
                runtime: "ollama".to_string(),
                host_port: "localhost:11434".to_string(),
            }
            .to_string(),
            "Ollama is not running at localhost:11434."
        );
    }
}
