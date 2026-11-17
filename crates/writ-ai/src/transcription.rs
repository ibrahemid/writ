//! Dictation over the network: one segment of speech to an OpenAI-compatible
//! `audio/transcriptions` endpoint, and the clean-up pass a transcript can take
//! through the AI connection.
//!
//! The same invariants as [`crate::connection`] hold here: the endpoint guard
//! runs before any byte leaves, a hosted endpoint waits for consent to its
//! host (audio has its own list, [`is_audio_consented`]), keys stay out of
//! logs and messages, and only the session's sizes and status codes are
//! logged. Audio and transcripts never are.

use std::time::Duration;

use futures_util::StreamExt;
use reqwest::multipart::{Form, Part};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use writ_core::ai::on_device;
use writ_core::config::AiConfig;
use writ_core::dictation::{
    clean_transcript, key_account, presets, DictationConfig, DictationEngine, DictationError,
    CLEAN_UP_INSTRUCTION,
};
use writ_core::polish::{self, PolishAction, PolishError};

use crate::connection::{
    prepare_action, prepare_on_device_action, run_rewrite_stream, sanitize_ai_error,
    stored_key_for, url_for_debug, OnDeviceRequest, PreparedRequest, StreamEvent,
};
use crate::keys::AiKeyState;

/// The wait before the one retry a transient failure gets.
const RETRY_DELAY: Duration = Duration::from_secs(1);
/// The most of a transcription reply that is read.
const MAX_REPLY_BYTES: usize = 1024 * 1024;
/// The most of a refusal that is read while looking for its reason.
const MAX_REFUSAL_BYTES: usize = 8 * 1024;
/// The longest server reason a refusal message carries, in characters.
const MAX_DETAIL_CHARS: usize = 200;
/// The longest one attempt at a segment may take; the retry gets as long
/// again. A 30-second segment on a slow uplink fits.
const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(180);

/// A transcription request resolved from config and checked: the endpoint is
/// allowed, the model named, the host consented to and the key found.
#[derive(Clone, PartialEq, Eq)]
pub struct PreparedTranscription {
    /// The `audio/transcriptions` URL.
    pub endpoint: String,
    /// The endpoint host, which errors and consent are named by.
    pub host: String,
    /// The transcription model.
    pub model: String,
    /// The ISO-639-1 code sent with each segment; `None` lets the service
    /// detect the language per segment.
    pub language: Option<String>,
    api_key: Option<String>,
    is_private_network: bool,
    /// The longest one attempt may take.
    deadline: Duration,
}

impl std::fmt::Debug for PreparedTranscription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedTranscription")
            .field("endpoint", &url_for_debug(&self.endpoint))
            .field("host", &self.host)
            .field("model", &self.model)
            .field("language", &self.language)
            .field("has_key", &self.api_key.is_some())
            .field("is_private_network", &self.is_private_network)
            .field("deadline", &self.deadline)
            .finish()
    }
}

impl PreparedTranscription {
    /// Whether the requests carry a key.
    pub fn has_key(&self) -> bool {
        self.api_key.is_some()
    }

    /// Whether the host is an address on a private network, which
    /// [`crate::connection::build_client_for`] reaches without a proxy.
    pub fn is_private_network(&self) -> bool {
        self.is_private_network
    }
}

/// Whether the person allowed audio to go to `host`. Exact membership in the
/// dictation list: a host allowed for text is not thereby allowed for audio.
pub fn is_audio_consented(d: &DictationConfig, host: &str) -> bool {
    d.consented_hosts.iter().any(|h| h == host)
}

/// Adds `host` to the hosts audio may go to. False when it was already there.
pub fn record_audio_consent(d: &mut DictationConfig, host: &str) -> bool {
    if is_audio_consented(d, host) {
        return false;
    }
    d.consented_hosts.push(host.to_string());
    d.consented_hosts.sort();
    d.consented_hosts.dedup();
    true
}

/// Where the transcription endpoint points and what it still needs, so the
/// settings page asks for consent or a key before a session starts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DictationEndpointState {
    /// The endpoint host; empty when the address does not parse.
    pub host: String,
    /// The endpoint leaves this machine, so audio waits for consent.
    pub is_hosted: bool,
    /// The scheme and host pass the outbound guard.
    pub is_allowed: bool,
    /// Audio may go to this host: it is local, or the person allowed it.
    pub is_consented: bool,
    /// A request cannot go without a key.
    pub needs_key: bool,
    /// The key is the AI connection's, read here and changed only there.
    pub key_shared_with_ai: bool,
    /// Whether a key is stored under the account dictation reads.
    pub key: AiKeyState,
    /// The page where the provider issues keys, for a preset.
    pub key_page: Option<String>,
}

/// The endpoint state for `d`. `key_state` is asked about the account
/// [`key_account`] names, and only for an endpoint that takes a key: on
/// macOS a keychain read alone can raise a password prompt.
pub fn endpoint_state(
    d: &DictationConfig,
    ai: &AiConfig,
    key_state: impl FnOnce(&str) -> AiKeyState,
) -> DictationEndpointState {
    let account = key_account(d, ai);
    let key_page = presets::key_page(d.provider).map(str::to_string);
    let no_key = AiKeyState {
        is_set: false,
        memory_only: false,
    };
    match polish::resolve_endpoint(&d.effective_base_url()) {
        Ok(target) => DictationEndpointState {
            is_consented: !target.is_hosted || is_audio_consented(d, &target.host),
            needs_key: target.needs_key(),
            key: if target.takes_key() {
                key_state(&account.account)
            } else {
                no_key
            },
            host: target.host,
            is_hosted: target.is_hosted,
            is_allowed: target.is_allowed,
            key_shared_with_ai: account.shared_with_ai,
            key_page,
        },
        Err(_) => DictationEndpointState {
            host: String::new(),
            is_hosted: false,
            is_allowed: false,
            is_consented: false,
            needs_key: false,
            key_shared_with_ai: account.shared_with_ai,
            key: no_key,
            key_page,
        },
    }
}

/// The `audio/transcriptions` URL under `base_url`. The route goes on the
/// path, before any query the address carries.
fn transcription_endpoint(base_url: &str) -> String {
    let base = base_url.trim();
    match reqwest::Url::parse(base) {
        Ok(mut url) => {
            let path = format!("{}/audio/transcriptions", url.path().trim_end_matches('/'));
            url.set_path(&path);
            url.set_fragment(None);
            url.to_string()
        }
        Err(_) => format!("{}/audio/transcriptions", base.trim_end_matches('/')),
    }
}

/// Checks `d` and resolves the request every segment of a session uses.
///
/// In order: the session runs on the API engine (an on-device session never
/// sends audio, and its endpoint is refused as not allowed), the endpoint
/// passes the guard, a model is named, a hosted host has audio consent, and a
/// key is found under the account [`key_account`] names. A host on a private
/// network takes a key when one is stored and goes without one otherwise.
/// `lookup_key` is called at most once, only for a host that takes a key.
pub fn prepare_transcription(
    d: &DictationConfig,
    ai: &AiConfig,
    on_device_supported: bool,
    lookup_key: impl FnOnce(&str) -> Option<String>,
) -> Result<PreparedTranscription, DictationError> {
    if d.effective_engine(on_device_supported) != DictationEngine::Api {
        return Err(DictationError::EndpointNotAllowed);
    }
    let base_url = d.effective_base_url();
    let target =
        polish::resolve_endpoint(&base_url).map_err(|_| DictationError::EndpointNotAllowed)?;
    if !target.is_allowed {
        return Err(DictationError::EndpointNotAllowed);
    }
    let model = d.model.trim();
    if model.is_empty() {
        return Err(DictationError::ModelRequired);
    }

    let api_key = if target.is_hosted {
        if !is_audio_consented(d, &target.host) {
            return Err(DictationError::ConsentRequired {
                host: target.host.clone(),
            });
        }
        let account = key_account(d, ai);
        match stored_key_for(&target, &account.account, lookup_key) {
            Some(key) => Some(key),
            None if target.needs_key() => {
                return Err(DictationError::KeyRequired {
                    host: target.host.clone(),
                })
            }
            None => None,
        }
    } else {
        None
    };

    let language = Some(d.language.trim().to_string()).filter(|code| !code.is_empty());
    Ok(PreparedTranscription {
        endpoint: transcription_endpoint(&base_url),
        host: target.host,
        model: model.to_string(),
        language,
        api_key,
        is_private_network: target.is_private_network,
        deadline: ATTEMPT_TIMEOUT,
    })
}

#[derive(Deserialize)]
struct TranscriptionReply {
    text: String,
}

/// Sends one segment, `wav`, and returns its transcript.
///
/// `prompt` is the end of the text already transcribed, which keeps names and
/// spelling steady across segments; it is sent only when it has text. A
/// network failure, a 429 or a 5xx is retried once after a second. An
/// attempt that takes longer than its deadline is a network failure, so it
/// too gets the one retry, with a deadline of its own.
/// `Ok(None)` when the transcript has no words, or when `cancel` fires: a stop
/// ends the request wherever it is waiting and nothing is returned.
pub async fn transcribe(
    client: &reqwest::Client,
    p: &PreparedTranscription,
    wav: Vec<u8>,
    prompt: Option<&str>,
    cancel: &CancellationToken,
) -> Result<Option<String>, DictationError> {
    let prompt = prompt.map(str::trim).filter(|text| !text.is_empty());
    attempt_twice(client, p, &wav, prompt, cancel).await
}

/// One attempt, and a second after [`RETRY_DELAY`] when the first failed in a
/// way that may pass.
async fn attempt_twice(
    client: &reqwest::Client,
    p: &PreparedTranscription,
    wav: &[u8],
    prompt: Option<&str>,
    cancel: &CancellationToken,
) -> Result<Option<String>, DictationError> {
    match attempt(client, p, wav, prompt, cancel).await {
        Err(error) if error.is_transient() => {
            tracing::info!(
                code = ?error.code(),
                bytes = wav.len(),
                "dictation: retrying a transcription"
            );
            tokio::select! {
                biased;
                () = cancel.cancelled() => return Ok(None),
                () = tokio::time::sleep(RETRY_DELAY) => {}
            }
            attempt(client, p, wav, prompt, cancel).await
        }
        other => other,
    }
}

async fn attempt(
    client: &reqwest::Client,
    p: &PreparedTranscription,
    wav: &[u8],
    prompt: Option<&str>,
    cancel: &CancellationToken,
) -> Result<Option<String>, DictationError> {
    if cancel.is_cancelled() {
        return Ok(None);
    }
    tokio::select! {
        biased;
        () = cancel.cancelled() => Ok(None),
        result = tokio::time::timeout(p.deadline, send(client, p, wav, prompt)) => match result {
            Ok(result) => result,
            Err(_) => {
                tracing::warn!(
                    bytes = wav.len(),
                    deadline_ms = u64::try_from(p.deadline.as_millis()).unwrap_or(u64::MAX),
                    "dictation: a transcription attempt ran out of time"
                );
                Err(DictationError::Network {
                    host: p.host.clone(),
                })
            }
        },
    }
}

async fn send(
    client: &reqwest::Client,
    p: &PreparedTranscription,
    wav: &[u8],
    prompt: Option<&str>,
) -> Result<Option<String>, DictationError> {
    let network = || DictationError::Network {
        host: p.host.clone(),
    };
    let file = Part::bytes(wav.to_vec())
        .file_name("segment.wav")
        .mime_str("audio/wav")
        .map_err(|_| network())?;
    let mut form = Form::new()
        .part("file", file)
        .text("model", p.model.clone())
        .text("response_format", "json");
    if let Some(language) = &p.language {
        form = form.text("language", language.clone());
    }
    if let Some(prompt) = prompt {
        form = form.text("prompt", prompt.to_string());
    }
    let mut request = client.post(&p.endpoint).multipart(form);
    if let Some(key) = &p.api_key {
        request = request.bearer_auth(key);
    }

    let response = request.send().await.map_err(|error| {
        tracing::warn!(
            connect = error.is_connect(),
            timeout = error.is_timeout(),
            bytes = wav.len(),
            "dictation: the transcription request failed"
        );
        network()
    })?;
    let status = response.status().as_u16();
    tracing::info!(
        status,
        bytes = wav.len(),
        "dictation: transcription answered"
    );
    match status {
        200..=299 => {
            let body = read_capped(response, MAX_REPLY_BYTES)
                .await
                .map_err(|_| network())?;
            let reply: TranscriptionReply =
                serde_json::from_slice(&body).map_err(|_| DictationError::Server {
                    host: p.host.clone(),
                    status,
                })?;
            Ok(clean_transcript(&reply.text))
        }
        401 | 403 => Err(DictationError::Unauthorized {
            host: p.host.clone(),
        }),
        404 => Err(DictationError::ModelNotFound {
            host: p.host.clone(),
            model: p.model.clone(),
        }),
        400 | 422 => {
            let body = read_capped(response, MAX_REFUSAL_BYTES)
                .await
                .unwrap_or_default();
            Err(DictationError::Refused {
                host: p.host.clone(),
                detail: refusal_detail(&body, status, p.api_key.as_deref()),
            })
        }
        429 => Err(DictationError::RateLimited {
            host: p.host.clone(),
        }),
        _ => Err(DictationError::Server {
            host: p.host.clone(),
            status,
        }),
    }
}

/// Reads at most `cap` bytes of a response body.
async fn read_capped(response: reqwest::Response, cap: usize) -> Result<Vec<u8>, reqwest::Error> {
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        body.extend_from_slice(&chunk?);
        if body.len() >= cap {
            body.truncate(cap);
            break;
        }
    }
    Ok(body)
}

/// The reason a refusal gives, as one short line with any URL, the key, and
/// control and bidirectional characters taken out; the status when the body
/// names no reason.
fn refusal_detail(body: &[u8], status: u16, api_key: Option<&str>) -> String {
    let parsed: Option<serde_json::Value> = serde_json::from_slice(body).ok();
    let reason = parsed.as_ref().and_then(|value| {
        let error = &value["error"];
        error["message"]
            .as_str()
            .or_else(|| error.as_str())
            .or_else(|| value["message"].as_str())
            .or_else(|| value["detail"].as_str())
            .map(plain_line)
    });
    let Some(mut reason) = reason.filter(|text| !text.trim().is_empty()) else {
        return format!("HTTP {status}.");
    };
    if let Some(key) = api_key.filter(|key| !key.is_empty()) {
        if !reason.contains(key) {
            let unmarked: String = reason.chars().filter(|c| !is_direction_mark(*c)).collect();
            if unmarked.contains(key) {
                reason = unmarked;
            }
        }
        reason = reason.replace(key, "<redacted-key>");
    }
    let line = sanitize_ai_error(&reason);
    match line.char_indices().nth(MAX_DETAIL_CHARS) {
        Some((cut, _)) => format!("{}...", line[..cut].trim_end()),
        None => line,
    }
}

/// `text` with each control character and line or paragraph separator turned
/// into a space and every bidirectional embedding, override and isolate
/// removed, so a server's words cannot reorder or break the line they are
/// shown in. The plain direction marks (LRM, RLM, ALM) stay: they only place
/// neutral characters in right-to-left text.
fn plain_line(text: &str) -> String {
    text.chars()
        .filter(|c| !is_bidi_control(*c))
        .map(|c| {
            if c.is_control() || matches!(c, '\u{2028}' | '\u{2029}') {
                ' '
            } else {
                c
            }
        })
        .collect()
}

/// A bidirectional embedding, override or isolate.
fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

/// A plain direction mark: LRM, RLM or ALM.
fn is_direction_mark(c: char) -> bool {
    matches!(c, '\u{061C}' | '\u{200E}' | '\u{200F}')
}

/// The clean-up pass for one transcript, ready for the AI connection.
#[derive(Debug, Clone, PartialEq)]
pub enum CleanUpRequest {
    /// A request to the connection's endpoint.
    Hosted(PreparedRequest),
    /// A request to Apple's on-device model.
    OnDevice(OnDeviceRequest),
}

/// Prepares the clean-up of `text` with [`CLEAN_UP_INSTRUCTION`]. It does not
/// need the Rewrite switch on; it does need the connection's endpoint,
/// consent and key exactly as a rewrite does.
pub fn prepare_clean_up(
    ai: &AiConfig,
    text: &str,
    lookup_key: impl FnOnce(&str) -> Option<String>,
) -> Result<CleanUpRequest, PolishError> {
    let action = PolishAction::Custom {
        instruction: CLEAN_UP_INSTRUCTION.to_string(),
    };
    if on_device::is_on_device(&ai.provider) {
        return prepare_on_device_action(&action, text).map(CleanUpRequest::OnDevice);
    }
    prepare_action(ai, &action, text, lookup_key).map(CleanUpRequest::Hosted)
}

/// Runs a hosted clean-up to the end and returns the cleaned text.
///
/// `Err` carries a plain message when the request failed, was stopped, or
/// came back without words; the caller then inserts the transcript as heard.
pub async fn run_clean_up(
    client: &reqwest::Client,
    request: &PreparedRequest,
    cancel: &CancellationToken,
) -> Result<String, String> {
    let mut text = String::new();
    let mut ended: Option<Result<(), String>> = None;
    run_rewrite_stream(client, request, cancel, |event| match event {
        StreamEvent::Chunk(chunk) => text.push_str(&chunk),
        StreamEvent::Done => ended = Some(Ok(())),
        StreamEvent::Error(message) => ended = Some(Err(message)),
    })
    .await;
    match ended {
        Some(Ok(())) => {
            clean_transcript(&text).ok_or_else(|| "The clean-up returned no text.".to_string())
        }
        Some(Err(message)) => Err(message),
        None => Err("The clean-up was stopped.".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    use super::*;

    const KEY: &str = "ZZ-dictation-key";

    /// A request with a key to a local socket, which a resolved endpoint never
    /// produces, so the header can be read on the wire.
    fn keyed(base: &str) -> PreparedTranscription {
        PreparedTranscription {
            endpoint: transcription_endpoint(base),
            host: "127.0.0.1".to_string(),
            model: "whisper-1".to_string(),
            language: None,
            api_key: Some(KEY.to_string()),
            is_private_network: false,
            deadline: ATTEMPT_TIMEOUT,
        }
    }

    fn recording_mock(
        status_line: &'static str,
        body: &'static str,
    ) -> (String, Arc<Mutex<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let seen = Arc::new(Mutex::new(String::new()));
        let recorder = Arc::clone(&seen);
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 16 * 1024];
                let mut read = 0;
                while let Ok(n) = stream.read(&mut buf[read..]) {
                    read += n;
                    let text = String::from_utf8_lossy(&buf[..read]);
                    if n == 0 || text.contains("--\r\n") || read == buf.len() {
                        break;
                    }
                }
                *recorder.lock().expect("recorder") =
                    String::from_utf8_lossy(&buf[..read]).to_string();
                let response = format!(
                    "{status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        (format!("http://127.0.0.1:{port}/v1"), seen)
    }

    fn run(p: &PreparedTranscription) -> Result<Option<String>, DictationError> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime")
            .block_on(async {
                let client = crate::connection::build_client().expect("client");
                transcribe(&client, p, vec![1, 2, 3], None, &CancellationToken::new()).await
            })
    }

    #[test]
    fn a_stored_key_goes_as_a_bearer_header() {
        let (base, seen) = recording_mock("HTTP/1.1 200 OK", r#"{"text":"hi"}"#);
        assert_eq!(run(&keyed(&base)), Ok(Some("hi".to_string())));
        let request = seen.lock().expect("seen").to_ascii_lowercase();
        assert!(
            request.contains(&format!(
                "authorization: bearer {}",
                KEY.to_ascii_lowercase()
            )),
            "{request}"
        );
    }

    #[test]
    fn a_refusal_never_echoes_the_key() {
        let (base, _) = recording_mock(
            "HTTP/1.1 400 Bad Request",
            r#"{"error":{"message":"Bad key ZZ-dictation-key for https://api.example.com/v1?token=1"}}"#,
        );
        let Err(DictationError::Refused { detail, .. }) = run(&keyed(&base)) else {
            panic!("expected a refusal");
        };
        assert!(!detail.contains(KEY), "{detail}");
        assert!(!detail.contains("example.com"), "{detail}");
        assert!(detail.starts_with("Bad key"), "{detail}");
    }

    #[test]
    fn the_debug_form_leaves_the_key_out() {
        let printed = format!("{:?}", keyed("http://127.0.0.1:1/v1"));
        assert!(!printed.contains(KEY), "{printed}");
        assert!(printed.contains("has_key: true"), "{printed}");
    }

    #[test]
    fn the_debug_form_leaves_out_url_credentials() {
        let printed = format!(
            "{:?}",
            keyed("https://user:pass-zz@api.example.com/v1?token=tok-zz")
        );
        for secret in ["pass-zz", "user", "tok-zz"] {
            assert!(!printed.contains(secret), "{secret} in {printed}");
        }
        assert!(
            printed.contains("https://api.example.com/v1/audio/transcriptions"),
            "{printed}"
        );
    }

    #[test]
    fn the_route_goes_before_the_query() {
        assert_eq!(
            transcription_endpoint("https://example.com/openai/v1?api-version=2025-01"),
            "https://example.com/openai/v1/audio/transcriptions?api-version=2025-01"
        );
        assert_eq!(
            transcription_endpoint(" https://api.openai.com/v1/ "),
            "https://api.openai.com/v1/audio/transcriptions"
        );
        assert_eq!(
            transcription_endpoint("http://127.0.0.1:8000"),
            "http://127.0.0.1:8000/audio/transcriptions"
        );
    }

    #[test]
    fn a_reason_keeps_its_direction_marks_and_loses_separators() {
        assert_eq!(
            plain_line("\u{200f}\u{0645}\u{0631}\u{062d}\u{0628}\u{0627} (1)\u{200e}\u{061c}"),
            "\u{200f}\u{0645}\u{0631}\u{062d}\u{0628}\u{0627} (1)\u{200e}\u{061c}"
        );
        assert_eq!(plain_line("one\u{2028}two\u{2029}three"), "one two three");
        let body = "{\"error\":{\"message\":\"Bad \\u200fvalue\\u200e (2)\"}}";
        assert_eq!(
            refusal_detail(body.as_bytes(), 400, None),
            "Bad \u{200f}value\u{200e} (2)"
        );
    }

    #[test]
    fn a_reason_loses_its_control_and_bidirectional_characters() {
        let body = "{\"error\":{\"message\":\"Bad\\u202e input\\u0007\\nfile\\u2066 ZZ-dict\\u200eation-key\"}}";
        assert_eq!(
            refusal_detail(body.as_bytes(), 400, Some(KEY)),
            "Bad input file <redacted-key>"
        );
        assert_eq!(
            refusal_detail(br#"{"message":"\u202e\u2069"}"#, 422, None),
            "HTTP 422."
        );
    }

    /// A server that answers 200 and then sends its body one byte every 50 ms.
    fn trickling_mock() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 16 * 1024];
                let _ = stream.read(&mut buf);
                let head = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 4000\r\n\r\n";
                if stream.write_all(head.as_bytes()).is_err() {
                    return;
                }
                for _ in 0..4000 {
                    if stream
                        .write_all(b" ")
                        .and_then(|()| stream.flush())
                        .is_err()
                    {
                        return;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }
        });
        format!("http://127.0.0.1:{port}/v1")
    }

    #[test]
    fn a_trickling_server_runs_out_of_time() {
        let mut p = keyed(&trickling_mock());
        p.deadline = Duration::from_millis(400);
        let started = std::time::Instant::now();
        assert_eq!(
            run(&p),
            Err(DictationError::Network {
                host: "127.0.0.1".to_string()
            })
        );
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "{:?}",
            started.elapsed()
        );
    }

    /// A server that never answers its first request and answers the second.
    fn stalls_once_mock() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        std::thread::spawn(move || {
            let mut stalled = Vec::new();
            for (n, stream) in listener.incoming().enumerate().take(2) {
                let Ok(mut stream) = stream else { return };
                let mut buf = [0u8; 16 * 1024];
                let _ = stream.read(&mut buf);
                if n == 0 {
                    stalled.push(stream);
                    continue;
                }
                let body = r#"{"text":"second"}"#;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes());
                let _ = stream.flush();
            }
        });
        format!("http://127.0.0.1:{port}/v1")
    }

    #[test]
    fn the_retry_gets_a_deadline_of_its_own() {
        let mut p = keyed(&stalls_once_mock());
        p.deadline = Duration::from_millis(400);
        assert_eq!(run(&p), Ok(Some("second".to_string())));
    }

    #[test]
    fn a_success_status_without_json_is_a_server_reply() {
        let (base, _) = recording_mock("HTTP/1.1 200 OK", "<html>ok</html>");
        assert_eq!(
            run(&keyed(&base)),
            Err(DictationError::Server {
                host: "127.0.0.1".to_string(),
                status: 200
            })
        );
    }

    #[test]
    fn a_refusal_without_a_reason_names_the_status() {
        assert_eq!(refusal_detail(b"<html>", 422, None), "HTTP 422.");
        assert_eq!(refusal_detail(br#"{"error":"  "}"#, 400, None), "HTTP 400.");
        assert_eq!(
            refusal_detail(br#"{"detail":"Audio file is too short."}"#, 400, None),
            "Audio file is too short."
        );
        let long = format!(r#"{{"message":"{}"}}"#, "a".repeat(400));
        let detail = refusal_detail(long.as_bytes(), 400, None);
        assert_eq!(detail.chars().count(), MAX_DETAIL_CHARS + 3);
    }
}
