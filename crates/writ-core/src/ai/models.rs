//! Reading a provider's model list, one parser per wire family.
//!
//! The fetch itself belongs to the Tauri side, which owns the HTTP client;
//! everything that decides what a body means lives here, so every family is
//! tested against a recorded answer without a network. Failures are typed
//! rather than described, because the frontend shows its own sentence for each
//! one and never the provider's text (ADR-031 rule 5.3).

use serde::{Deserialize, Serialize};

/// Substrings that mark an OpenAI model id as something other than a chat
/// model. Without them the dropdown is mostly embeddings and speech.
///
/// `audio` is deliberately absent: `gpt-4o-audio-preview` answers
/// `chat/completions` like any other chat model, and a marker for it would
/// take that row out of the picker.
const NON_CHAT_MARKERS: &[&str] = &[
    "embedding",
    "tts",
    "whisper",
    "dall-e",
    "moderation",
    "realtime",
    "transcribe",
    "image",
];

/// The body shape a provider answers its model list in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListFamily {
    /// `{ "data": [{ "id": … }] }`.
    OpenAi,
    /// `{ "data": [{ "id": … }], "has_more": …, "last_id": … }`, paginated.
    Anthropic,
    /// `{ "models": [{ "name": "models/…", "supportedGenerationMethods": […] }] }`.
    Gemini,
    /// Ollama's `/api/tags`: `{ "models": [{ "name": … }] }`.
    Ollama,
}

impl ListFamily {
    /// The family a provider id answers in. Everything the table does not
    /// name otherwise speaks the OpenAI shape, `custom` included.
    pub fn for_provider(id: &str) -> Self {
        match id {
            "anthropic" => Self::Anthropic,
            "gemini" => Self::Gemini,
            "ollama" => Self::Ollama,
            _ => Self::OpenAi,
        }
    }
}

/// Why a model list could not be read.
///
/// Serialises as `{ "kind": … }`, with the HTTP code alongside for
/// [`ModelListError::Status`], so the frontend matches on the kind and shows a
/// sentence of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ModelListError {
    /// Nothing answered at the address.
    Unreachable,
    /// The request ran out of time.
    Timeout,
    /// The provider rejected the credential.
    Unauthorized,
    /// The provider answered with another error status.
    Status {
        /// The HTTP status code.
        code: u16,
    },
    /// The answer was not a model list.
    Malformed,
    /// The host has not been allowed yet, so nothing was sent. The list
    /// carries the key, which makes it a send like any other (ADR-031 rule
    /// 2.2), and every send waits for Allow.
    ConsentRequired,
}

impl std::fmt::Display for ModelListError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreachable => write!(f, "Nothing answered at that address."),
            Self::Timeout => write!(f, "The provider did not answer in time."),
            Self::Unauthorized => write!(f, "The provider rejected the request."),
            Self::Status { code } => write!(f, "The provider answered with status {code}."),
            Self::Malformed => write!(f, "The answer was not a model list."),
            Self::ConsentRequired => write!(f, "Allow this host first."),
        }
    }
}

impl std::error::Error for ModelListError {}

/// One page of Anthropic's model list, with the cursor for the next one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnthropicPage {
    /// The model ids on this page, sorted.
    pub ids: Vec<String>,
    /// The `after_id` the next request carries, absent on the last page.
    pub next_cursor: Option<String>,
}

#[derive(Deserialize)]
struct OpenAiList {
    data: Vec<OpenAiEntry>,
}

#[derive(Deserialize)]
struct OpenAiEntry {
    id: String,
}

#[derive(Deserialize)]
struct AnthropicList {
    data: Vec<OpenAiEntry>,
    #[serde(default)]
    has_more: bool,
    #[serde(default)]
    last_id: Option<String>,
}

#[derive(Deserialize)]
struct GeminiList {
    models: Vec<GeminiEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiEntry {
    name: String,
    #[serde(default)]
    supported_generation_methods: Vec<String>,
}

#[derive(Deserialize)]
struct OllamaList {
    models: Vec<OllamaEntry>,
}

#[derive(Deserialize)]
struct OllamaEntry {
    name: String,
}

/// The model ids in a response body, sorted by id.
///
/// The OpenAI family is returned whole; [`filter_openai_ids`] drops what
/// cannot answer a chat request, which is worth doing for OpenAI itself and
/// wrong for a provider whose ids happen to carry the same words. The
/// Anthropic family returns one page; [`parse_anthropic_page`] carries the
/// cursor for the rest.
pub fn parse_model_list(
    wire_family: ListFamily,
    body: &str,
) -> Result<Vec<String>, ModelListError> {
    match wire_family {
        ListFamily::OpenAi => {
            let list: OpenAiList = parse(body)?;
            Ok(sort_ids(list.data.into_iter().map(|e| e.id).collect()))
        }
        ListFamily::Anthropic => Ok(parse_anthropic_page(body)?.ids),
        ListFamily::Gemini => {
            let list: GeminiList = parse(body)?;
            let ids = list
                .models
                .into_iter()
                .filter(|m| {
                    m.supported_generation_methods
                        .iter()
                        .any(|method| method == "generateContent")
                })
                .map(|m| m.name.trim_start_matches("models/").to_string())
                .collect();
            Ok(sort_ids(ids))
        }
        ListFamily::Ollama => {
            let list: OllamaList = parse(body)?;
            Ok(sort_ids(list.models.into_iter().map(|e| e.name).collect()))
        }
    }
}

/// One page of Anthropic's model list and the cursor that follows it.
pub fn parse_anthropic_page(body: &str) -> Result<AnthropicPage, ModelListError> {
    let list: AnthropicList = parse(body)?;
    let next_cursor = if list.has_more { list.last_id } else { None };
    Ok(AnthropicPage {
        ids: sort_ids(list.data.into_iter().map(|e| e.id).collect()),
        next_cursor,
    })
}

/// Drops the ids that cannot answer a chat request: embeddings, speech,
/// images and moderation. The rest keeps its order, which is by id.
pub fn filter_openai_ids(ids: Vec<String>) -> Vec<String> {
    let kept = ids
        .into_iter()
        .filter(|id| {
            let id = id.to_ascii_lowercase();
            !NON_CHAT_MARKERS.iter().any(|marker| id.contains(marker))
        })
        .collect();
    sort_ids(kept)
}

/// Sorts model ids the way the dropdown shows them.
pub fn sort_ids(mut ids: Vec<String>) -> Vec<String> {
    ids.sort();
    ids
}

fn parse<T: serde::de::DeserializeOwned>(body: &str) -> Result<T, ModelListError> {
    serde_json::from_str(body).map_err(|_| ModelListError::Malformed)
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPENAI_BODY: &str = r#"{
      "object": "list",
      "data": [
        { "id": "gpt-4o-mini", "object": "model", "owned_by": "system" },
        { "id": "text-embedding-3-small", "object": "model", "owned_by": "system" },
        { "id": "gpt-5-mini", "object": "model", "owned_by": "system" },
        { "id": "gpt-4o-mini-tts", "object": "model", "owned_by": "system" },
        { "id": "whisper-1", "object": "model", "owned_by": "openai-internal" },
        { "id": "dall-e-3", "object": "model", "owned_by": "system" },
        { "id": "omni-moderation-latest", "object": "model", "owned_by": "system" },
        { "id": "gpt-4o-realtime-preview", "object": "model", "owned_by": "system" },
        { "id": "gpt-4o-audio-preview", "object": "model", "owned_by": "system" },
        { "id": "gpt-4o-transcribe", "object": "model", "owned_by": "system" },
        { "id": "gpt-image-1", "object": "model", "owned_by": "system" },
        { "id": "chatgpt-4o-latest", "object": "model", "owned_by": "system" }
      ]
    }"#;

    const ANTHROPIC_BODY: &str = r#"{
      "data": [
        { "type": "model", "id": "claude-sonnet-5", "display_name": "Claude Sonnet 5" },
        { "type": "model", "id": "claude-opus-5", "display_name": "Claude Opus 5" }
      ],
      "has_more": true,
      "first_id": "claude-sonnet-5",
      "last_id": "claude-opus-5"
    }"#;

    const ANTHROPIC_LAST_PAGE: &str = r#"{
      "data": [{ "type": "model", "id": "claude-haiku-4-5", "display_name": "Claude Haiku 4.5" }],
      "has_more": false,
      "first_id": "claude-haiku-4-5",
      "last_id": "claude-haiku-4-5"
    }"#;

    const GEMINI_BODY: &str = r#"{
      "models": [
        {
          "name": "models/gemini-2.5-flash",
          "displayName": "Gemini 2.5 Flash",
          "supportedGenerationMethods": ["generateContent", "countTokens"]
        },
        {
          "name": "models/text-embedding-004",
          "displayName": "Text Embedding 004",
          "supportedGenerationMethods": ["embedContent"]
        },
        {
          "name": "models/gemini-2.5-pro",
          "displayName": "Gemini 2.5 Pro",
          "supportedGenerationMethods": ["generateContent", "countTokens"]
        }
      ]
    }"#;

    const OLLAMA_BODY: &str = r#"{
      "models": [
        { "name": "llama3.2:latest", "size": 2019393189, "digest": "a80c4f17acd5" },
        { "name": "qwen2.5-coder:7b", "size": 4683087332, "digest": "2b0496514337" }
      ]
    }"#;

    #[test]
    fn an_openai_list_is_parsed_whole_and_sorted() {
        let ids = parse_model_list(ListFamily::OpenAi, OPENAI_BODY).unwrap();
        assert_eq!(ids.len(), 12);
        assert_eq!(ids.first().unwrap(), "chatgpt-4o-latest");
        assert!(ids.contains(&"text-embedding-3-small".to_string()));
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted);
    }

    #[test]
    fn the_openai_filter_keeps_only_what_answers_chat() {
        let ids = filter_openai_ids(parse_model_list(ListFamily::OpenAi, OPENAI_BODY).unwrap());
        assert_eq!(
            ids,
            vec![
                "chatgpt-4o-latest".to_string(),
                // Audio in, audio out, but it is a chat model and the picker
                // is the only place a person can reach it.
                "gpt-4o-audio-preview".to_string(),
                "gpt-4o-mini".to_string(),
                "gpt-5-mini".to_string(),
            ]
        );

        for dropped in [
            "gpt-4o-mini-tts",
            "gpt-4o-realtime-preview",
            "gpt-4o-transcribe",
            "gpt-image-1",
            "text-embedding-3-small",
            "whisper-1",
            "omni-moderation-latest",
            "dall-e-3",
        ] {
            assert!(
                !ids.iter().any(|id| id == dropped),
                "{dropped} cannot answer a chat request"
            );
        }
    }

    #[test]
    fn an_anthropic_page_carries_its_cursor() {
        let page = parse_anthropic_page(ANTHROPIC_BODY).unwrap();
        assert_eq!(
            page.ids,
            vec!["claude-opus-5".to_string(), "claude-sonnet-5".to_string()]
        );
        assert_eq!(page.next_cursor.as_deref(), Some("claude-opus-5"));

        let last = parse_anthropic_page(ANTHROPIC_LAST_PAGE).unwrap();
        assert_eq!(last.next_cursor, None);
        assert_eq!(
            parse_model_list(ListFamily::Anthropic, ANTHROPIC_BODY).unwrap(),
            page.ids
        );
    }

    #[test]
    fn a_gemini_list_keeps_what_generates_content_and_drops_the_prefix() {
        let ids = parse_model_list(ListFamily::Gemini, GEMINI_BODY).unwrap();
        assert_eq!(
            ids,
            vec!["gemini-2.5-flash".to_string(), "gemini-2.5-pro".to_string()]
        );
    }

    #[test]
    fn an_ollama_list_is_the_installed_names() {
        let ids = parse_model_list(ListFamily::Ollama, OLLAMA_BODY).unwrap();
        assert_eq!(
            ids,
            vec![
                "llama3.2:latest".to_string(),
                "qwen2.5-coder:7b".to_string()
            ]
        );
    }

    #[test]
    fn a_body_that_is_not_the_expected_shape_is_malformed() {
        for family in [
            ListFamily::OpenAi,
            ListFamily::Anthropic,
            ListFamily::Gemini,
            ListFamily::Ollama,
        ] {
            assert_eq!(
                parse_model_list(family, "<html>502 Bad Gateway</html>").unwrap_err(),
                ModelListError::Malformed
            );
            assert_eq!(
                parse_model_list(family, "{\"error\":{\"message\":\"nope\"}}").unwrap_err(),
                ModelListError::Malformed
            );
        }
        assert_eq!(
            parse_anthropic_page("{\"data\":\"not a list\"}").unwrap_err(),
            ModelListError::Malformed
        );
    }

    #[test]
    fn an_empty_list_is_not_an_error() {
        assert!(parse_model_list(ListFamily::OpenAi, "{\"data\":[]}")
            .unwrap()
            .is_empty());
        assert!(parse_model_list(ListFamily::Ollama, "{\"models\":[]}")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn a_family_is_chosen_by_provider_id() {
        assert_eq!(ListFamily::for_provider("anthropic"), ListFamily::Anthropic);
        assert_eq!(ListFamily::for_provider("gemini"), ListFamily::Gemini);
        assert_eq!(ListFamily::for_provider("ollama"), ListFamily::Ollama);
        for id in ["openai", "lmstudio", "groq", "custom", "nonsense"] {
            assert_eq!(ListFamily::for_provider(id), ListFamily::OpenAi, "{id}");
        }
    }

    #[test]
    fn every_failure_serialises_with_its_kind() {
        assert_eq!(
            serde_json::to_string(&ModelListError::Unreachable).unwrap(),
            "{\"kind\":\"unreachable\"}"
        );
        assert_eq!(
            serde_json::to_string(&ModelListError::Timeout).unwrap(),
            "{\"kind\":\"timeout\"}"
        );
        assert_eq!(
            serde_json::to_string(&ModelListError::Unauthorized).unwrap(),
            "{\"kind\":\"unauthorized\"}"
        );
        assert_eq!(
            serde_json::to_string(&ModelListError::Malformed).unwrap(),
            "{\"kind\":\"malformed\"}"
        );
        assert_eq!(
            serde_json::to_string(&ModelListError::Status { code: 401 }).unwrap(),
            "{\"kind\":\"status\",\"code\":401}"
        );
        assert_eq!(
            serde_json::to_string(&ModelListError::ConsentRequired).unwrap(),
            "{\"kind\":\"consent_required\"}"
        );
    }

    #[test]
    fn a_failure_reads_as_a_sentence_and_names_no_secret() {
        for e in [
            ModelListError::Unreachable,
            ModelListError::Timeout,
            ModelListError::Unauthorized,
            ModelListError::Malformed,
            ModelListError::ConsentRequired,
            ModelListError::Status { code: 500 },
        ] {
            let sentence = e.to_string();
            assert!(sentence.ends_with('.'), "{sentence}");
            assert!(!sentence.to_lowercase().contains("key"), "{sentence}");
        }
        assert!(ModelListError::Status { code: 500 }
            .to_string()
            .contains("500"));
    }

    #[test]
    fn sorting_is_by_id() {
        let ids = sort_ids(vec![
            "b".to_string(),
            "a".to_string(),
            "a".to_string(),
            "C".to_string(),
        ]);
        assert_eq!(
            ids,
            vec![
                "C".to_string(),
                "a".to_string(),
                "a".to_string(),
                "b".to_string()
            ]
        );
    }
}
