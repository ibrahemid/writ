//! Reading a provider's model list, one parser per wire family.
//!
//! The fetch itself belongs to the Tauri side, which owns the HTTP client;
//! everything that decides what a body means lives here, so every family is
//! tested against a recorded answer without a network. Failures are typed
//! rather than described, because the frontend shows its own sentence for each
//! one and never the provider's text (ADR-031 rule 5.3).

use serde::{Deserialize, Serialize};

/// The OpenAI families that answer `chat/completions`, named by the first
/// `-`-separated segment of an id. Anything else OpenAI lists (`babbage-002`,
/// `davinci-002`, `computer-use-preview`, `text-embedding-3-small`,
/// `whisper-1`, `tts-1`, `omni-moderation-latest`, `dall-e-3`) is left out.
///
/// An allowlist rather than a list of exceptions, because the ids that break a
/// first send are the ones nobody thought to exclude: a completion-only id
/// sorted first and became the model. Checked against the documented
/// Chat Completions support of every model OpenAI lists
/// (`tests/fixtures/ai-models/`).
const OPENAI_CHAT_FAMILIES: &[&str] = &["gpt", "chatgpt", "chat", "o1", "o3", "o4"];

/// Segments that take an id in one of [`OPENAI_CHAT_FAMILIES`] out of the
/// picker: the Responses-only variants (`-pro`, `-codex`, `-deep-research`),
/// speech, images, realtime and live audio, and the open-weight and
/// restricted-access models.
///
/// `audio` and `search` are deliberately absent: `gpt-4o-audio-preview` and
/// `gpt-4o-search-preview` answer `chat/completions` like any other chat
/// model, and the picker is the only place a person can reach them.
const OPENAI_NOT_CHAT_SEGMENTS: &[&str] = &[
    "pro",
    "codex",
    "deep",
    "research",
    "realtime",
    "live",
    "transcribe",
    "tts",
    "image",
    "oss",
    "cyber",
    "daybreak",
    "rosalind",
    "embedding",
    "moderation",
    "whisper",
];

/// OpenAI models in a chat family whose own page says they do not answer
/// `chat/completions`, each taken out with its dated snapshots. The page of
/// `gpt-3.5-turbo-instruct`, read on 2026-10-13, says it is "Compatible with
/// legacy Completions endpoint and not Chat Completions."
const OPENAI_NOT_CHAT_MODELS: &[&str] = &["gpt-3.5-turbo-instruct"];

/// The `type` values an OpenAI-shaped list entry carries when it is not a
/// text model. Together types every entry this way; a list whose entries carry
/// no `type`, or another meaning of it (Mistral's `base`), keeps them all.
const NON_TEXT_KINDS: &[&str] = &[
    "image",
    "embedding",
    "moderation",
    "rerank",
    "audio",
    "transcribe",
    "video",
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

/// Where the ids in a [`ModelCatalog`] came from.
///
/// The distinction is what a send may be judged against: only `Live` is the
/// account's own inventory, so only `Live` can say a model does not exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CatalogSource {
    /// Read from the provider, for this key.
    Live,
    /// The provider table's suggestions, because the list could not be read.
    Curated,
    /// Neither: the row carries no suggestions and nothing answered.
    None,
}

/// The models on offer for one provider, stamped with the provider they were
/// read for.
///
/// The stamp is the point: a list answered for one provider must not survive a
/// change of provider, and an answer that arrives after the change is dropped
/// by comparing this field rather than by hoping the requests stay in order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ModelCatalog {
    /// The provider id this list describes.
    pub provider: String,
    /// The ids on offer, in the order the picker shows them.
    pub models: Vec<String>,
    /// Whether the ids are the provider's own or the table's suggestions.
    pub source: CatalogSource,
    /// Why the provider's own list could not be read, when it could not.
    pub error: Option<ModelListError>,
}

impl ModelCatalog {
    /// What the provider itself listed.
    pub fn live(provider: &str, models: Vec<String>) -> Self {
        Self {
            provider: provider.to_string(),
            models,
            source: CatalogSource::Live,
            error: None,
        }
    }

    /// The table's suggestions, and the failure that made them the answer.
    ///
    /// A row with no suggestions answers `None` rather than an empty `Curated`
    /// list, so "nothing to offer" and "offering guesses" stay apart.
    pub fn fallback(provider: &str, error: ModelListError) -> Self {
        let curated: Vec<String> = super::providers::provider(provider)
            .map(|row| row.curated_models.iter().map(|id| id.to_string()).collect())
            .unwrap_or_default();
        let source = if curated.is_empty() {
            CatalogSource::None
        } else {
            CatalogSource::Curated
        };
        Self {
            provider: provider.to_string(),
            models: curated,
            source,
            error: Some(error),
        }
    }

    /// Whether `model` can be judged against this catalog and is absent from
    /// it. Only a live list is the account's inventory; suggestions decide
    /// nothing.
    pub fn refuses(&self, model: &str) -> bool {
        self.source == CatalogSource::Live && !self.models.iter().any(|id| id == model)
    }
}

/// The model a provider starts from: the row's `default_model` when the
/// provider's own list holds it, else the first of the row's
/// `curated_models` that the list holds, else, on a runtime on this machine
/// whose list holds one model, that model, and nothing otherwise, which
/// leaves the picker open (ADR-040 section 12).
///
/// Only an id that is both the table's choice and the account's inventory is
/// seeded. The list alone is not a choice: sorted, its first id is whatever
/// sorts first, which on OpenAI's list is a completion-only model, on
/// Anthropic's the dearest Claude, and on OpenRouter's an arbitrary vendor's.
/// The table alone is not an inventory: DeepSeek stopped serving
/// `deepseek-chat` while the table still named it, so an id the list does not
/// hold is a send that fails for a reason nobody chose. An empty model is a
/// state both surfaces already hold: nothing is sent and each one asks for a
/// model. A local runtime lists only what its owner installed, so a list of
/// one is a choice already made; with several installed and none suggested,
/// the first sorted is as likely an embedding model as a chat model.
///
/// The ids themselves are data in the provider table, read against the dated
/// fixtures, so a retired one is corrected there in a release without touching
/// this rule, and until it is, the rule falls through to the next suggestion
/// or to the picker.
///
/// The list counts only while it names this provider, so an answer that
/// arrives after a provider change never seeds the row that replaced it.
pub fn seed_model(provider: &str, catalog: Option<&ModelCatalog>) -> String {
    let Some(held) = catalog else {
        return String::new();
    };
    if held.provider != provider || held.source != CatalogSource::Live {
        return String::new();
    }
    let Some(row) = super::providers::provider(provider) else {
        return String::new();
    };
    let listed = |id: &str| held.models.iter().any(|model| model == id);
    if !row.default_model.is_empty() && listed(row.default_model) {
        return row.default_model.to_string();
    }
    if let Some(suggested) = row.curated_models.iter().find(|id| listed(id)) {
        return suggested.to_string();
    }
    match held.models.as_slice() {
        [only] if row.group == super::providers::ProviderGroup::Local => only.clone(),
        _ => String::new(),
    }
}

/// One page of Anthropic's model list, with the cursor for the next one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnthropicPage {
    /// The model ids on this page, sorted.
    pub ids: Vec<String>,
    /// The `after_id` the next request carries, absent on the last page.
    pub next_cursor: Option<String>,
}

/// An OpenAI-shaped list: `{ "data": [...] }` as OpenAI and most hosts answer
/// it, or the bare array Together answers.
#[derive(Deserialize)]
#[serde(untagged)]
enum OpenAiList {
    Wrapped { data: Vec<OpenAiEntry> },
    Bare(Vec<OpenAiEntry>),
}

impl OpenAiList {
    fn entries(self) -> Vec<OpenAiEntry> {
        match self {
            Self::Wrapped { data } => data,
            Self::Bare(entries) => entries,
        }
    }
}

#[derive(Deserialize)]
struct OpenAiEntry {
    id: String,
    /// What kind of model the entry is, where the host says.
    #[serde(default, rename = "type")]
    kind: Option<String>,
}

impl OpenAiEntry {
    fn is_text(&self) -> bool {
        self.kind
            .as_deref()
            .is_none_or(|kind| !NON_TEXT_KINDS.contains(&kind))
    }
}

#[derive(Deserialize)]
struct AnthropicEntry {
    id: String,
}

#[derive(Deserialize)]
struct AnthropicList {
    data: Vec<AnthropicEntry>,
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
/// The OpenAI family is returned whole but for entries the host itself types
/// as something other than a text model; [`filter_openai_ids`] drops what
/// cannot answer a chat request on OpenAI's own list, which is worth doing for
/// OpenAI itself and wrong for a provider whose ids happen to carry the same
/// words. The Anthropic family returns one page; [`parse_anthropic_page`]
/// carries the cursor for the rest.
pub fn parse_model_list(
    wire_family: ListFamily,
    body: &str,
) -> Result<Vec<String>, ModelListError> {
    match wire_family {
        ListFamily::OpenAi => {
            let list: OpenAiList = parse(body)?;
            let ids = list
                .entries()
                .into_iter()
                .filter(OpenAiEntry::is_text)
                .map(|entry| entry.id)
                .collect();
            Ok(sort_ids(ids))
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

/// Keeps the OpenAI ids that answer a chat request: the chat families of
/// [`OPENAI_CHAT_FAMILIES`] without the segments of
/// [`OPENAI_NOT_CHAT_SEGMENTS`] and the models of [`OPENAI_NOT_CHAT_MODELS`].
/// The rest keeps its order, which is by id.
pub fn filter_openai_ids(ids: Vec<String>) -> Vec<String> {
    let kept = ids.into_iter().filter(|id| answers_chat(id)).collect();
    sort_ids(kept)
}

/// Whether an OpenAI id answers `chat/completions`.
///
/// A fine-tuned model (`ft:gpt-4o-mini-2024-07-18:org:suffix:id`) is judged
/// by the model it was tuned from; the organisation and suffix after it are
/// the account's own words and decide nothing.
fn answers_chat(id: &str) -> bool {
    let id = id.to_ascii_lowercase();
    let base = id
        .strip_prefix("ft:")
        .unwrap_or(&id)
        .split(':')
        .next()
        .unwrap_or_default();
    let mut segments = base.split('-');
    let family = segments.next().unwrap_or_default();
    OPENAI_CHAT_FAMILIES.contains(&family)
        && !segments.any(|segment| OPENAI_NOT_CHAT_SEGMENTS.contains(&segment))
        && !OPENAI_NOT_CHAT_MODELS
            .iter()
            .any(|model| is_model_or_snapshot(base, model))
}

/// Whether `id` is `model` or one of its dated snapshots, which OpenAI names
/// by a `-` and a number after the model (`gpt-3.5-turbo-0125`).
fn is_model_or_snapshot(id: &str, model: &str) -> bool {
    id.strip_prefix(model).is_some_and(|rest| {
        rest.is_empty()
            || rest
                .strip_prefix('-')
                .is_some_and(|date| date.starts_with(|c: char| c.is_ascii_digit()))
    })
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

#[cfg(test)]
mod catalog_tests {
    use super::super::providers::ProviderGroup;
    use super::*;

    #[test]
    fn a_failed_list_falls_back_to_the_table_and_keeps_the_reason() {
        let catalog = ModelCatalog::fallback("deepseek", ModelListError::Unauthorized);
        assert_eq!(catalog.provider, "deepseek");
        assert_eq!(catalog.source, CatalogSource::Curated);
        assert_eq!(catalog.models, ["deepseek-flash", "deepseek-v4-pro"]);
        assert_eq!(catalog.error, Some(ModelListError::Unauthorized));

        // A row with nothing to suggest says so rather than offering an empty
        // list of guesses.
        let nothing = ModelCatalog::fallback("custom", ModelListError::Unreachable);
        assert_eq!(nothing.source, CatalogSource::None);
        assert!(nothing.models.is_empty());
    }

    #[test]
    fn only_a_live_list_can_refuse_a_model() {
        let live = ModelCatalog::live("ollama", vec!["qwen3:4b".to_string()]);
        assert!(live.refuses("deepseek-flash"));
        assert!(!live.refuses("qwen3:4b"));

        // Suggestions are not the account's inventory, so they decide nothing.
        let curated = ModelCatalog::fallback("deepseek", ModelListError::Timeout);
        assert!(!curated.refuses("a-model-nobody-listed"));
    }

    #[test]
    fn the_source_serialises_as_the_ipc_strings() {
        assert_eq!(
            serde_json::to_string(&CatalogSource::Live).unwrap(),
            "\"live\""
        );
        assert_eq!(
            serde_json::to_string(&CatalogSource::Curated).unwrap(),
            "\"curated\""
        );
        assert_eq!(
            serde_json::to_string(&CatalogSource::None).unwrap(),
            "\"none\""
        );
    }

    #[test]
    fn a_live_list_seeds_the_rows_default_when_it_holds_it() {
        for row in super::super::providers::PROVIDERS
            .iter()
            .filter(|row| !row.default_model.is_empty())
        {
            // Sorted first on purpose: the order of the list decides nothing.
            let live = ModelCatalog::live(
                row.id,
                vec!["0-sorts-first".to_string(), row.default_model.to_string()],
            );
            assert_eq!(
                seed_model(row.id, Some(&live)),
                row.default_model,
                "{}",
                row.id
            );
        }
    }

    #[test]
    fn without_its_default_a_list_seeds_the_first_suggestion_it_holds() {
        let row = super::super::providers::provider("openai").expect("a row");
        let second = row.curated_models[1].to_string();
        let third = row.curated_models[2].to_string();
        let live = ModelCatalog::live("openai", vec![third, second.clone()]);
        assert_eq!(seed_model("openai", Some(&live)), second);
    }

    #[test]
    fn a_list_holding_none_of_the_rows_ids_seeds_nothing() {
        // Every row: an id the provider answered but nobody chose is not a
        // model to send to, so the row is left for the picker. A runtime on
        // this machine that lists one model seeds it, so the local rows are
        // given two.
        for row in super::super::providers::PROVIDERS {
            let mut answered = vec!["answered-id".to_string()];
            if row.group == ProviderGroup::Local {
                answered.push("another-id".to_string());
            }
            let live = ModelCatalog::live(row.id, answered);
            assert_eq!(seed_model(row.id, Some(&live)), "", "{}", row.id);
        }
    }

    #[test]
    fn a_local_runtime_that_lists_one_model_seeds_it() {
        for id in ["ollama", "lmstudio"] {
            let one = ModelCatalog::live(id, vec!["the-only-model".to_string()]);
            assert_eq!(seed_model(id, Some(&one)), "the-only-model", "{id}");
        }
        // One id a hosted or typed endpoint answers is still not a choice
        // anyone made.
        for id in ["deepseek", "custom"] {
            let one = ModelCatalog::live(id, vec!["the-only-model".to_string()]);
            assert_eq!(seed_model(id, Some(&one)), "", "{id}");
        }
        // The one model counts only from the runtime's own list.
        let fallback = ModelCatalog::fallback("lmstudio", ModelListError::Unreachable);
        assert_eq!(seed_model("lmstudio", Some(&fallback)), "");
    }

    #[test]
    fn a_local_runtime_that_lists_several_models_and_no_suggestion_seeds_nothing() {
        let ollama = ModelCatalog::live(
            "ollama",
            vec!["llama3.1:8b".to_string(), "mistral:latest".to_string()],
        );
        assert_eq!(seed_model("ollama", Some(&ollama)), "");
        let lmstudio = ModelCatalog::live(
            "lmstudio",
            vec!["model-a".to_string(), "model-b".to_string()],
        );
        assert_eq!(seed_model("lmstudio", Some(&lmstudio)), "");
    }

    #[test]
    fn a_local_runtime_seeds_an_installed_suggestion_and_nothing_else() {
        let installed = ModelCatalog::live(
            "ollama",
            vec!["mistral:latest".to_string(), "qwen3:8b".to_string()],
        );
        assert_eq!(seed_model("ollama", Some(&installed)), "qwen3:8b");

        // An embedding model sorts first as readily as a chat model does.
        let unknown = ModelCatalog::live(
            "ollama",
            vec![
                "all-minilm:latest".to_string(),
                "mistral:latest".to_string(),
            ],
        );
        assert_eq!(seed_model("ollama", Some(&unknown)), "");
    }

    #[test]
    fn a_bare_list_is_read_and_entries_typed_as_other_kinds_are_dropped() {
        let body = r#"[
          { "id": "openai/gpt-oss-120b", "object": "model", "type": "chat" },
          { "id": "Qwen/Qwen3.5-9B", "object": "model", "type": "language" },
          { "id": "BAAI/bge-large-en-v1.5", "object": "model", "type": "embedding" },
          { "id": "black-forest-labs/FLUX.2-pro", "object": "model", "type": "image" }
        ]"#;
        assert_eq!(
            parse_model_list(ListFamily::OpenAi, body).unwrap(),
            vec![
                "Qwen/Qwen3.5-9B".to_string(),
                "openai/gpt-oss-120b".to_string()
            ]
        );

        // A host whose `type` means something else keeps every entry.
        let mistral = r#"{ "object": "list", "data": [
          { "id": "mistral-small-latest", "type": "base" },
          { "id": "ft:open-mistral-7b:acme", "type": "fine-tuned" }
        ] }"#;
        assert_eq!(
            parse_model_list(ListFamily::OpenAi, mistral).unwrap().len(),
            2
        );
    }

    #[test]
    fn a_list_that_could_not_be_read_seeds_nothing() {
        let fallback = ModelCatalog::fallback("deepseek", ModelListError::Unauthorized);
        assert_eq!(fallback.source, CatalogSource::Curated);
        assert_eq!(seed_model("deepseek", Some(&fallback)), "");
        assert_eq!(seed_model("deepseek", None), "");
    }

    #[test]
    fn a_list_read_for_another_provider_seeds_nothing() {
        let live = ModelCatalog::live("ollama", vec!["qwen3:4b".to_string()]);
        assert_eq!(seed_model("deepseek", Some(&live)), "");
    }

    #[test]
    fn a_provider_that_lists_nothing_seeds_nothing() {
        let empty = ModelCatalog::live("deepseek", Vec::new());
        assert_eq!(seed_model("deepseek", Some(&empty)), "");
    }
}
