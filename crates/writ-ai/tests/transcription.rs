use std::cell::RefCell;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio_util::sync::CancellationToken;
use writ_ai::connection::{build_client, prepare_request};
use writ_ai::keys::AiKeyState;
use writ_ai::transcription::{
    endpoint_state, is_audio_consented, prepare_clean_up, prepare_transcription,
    record_audio_consent, run_clean_up, transcribe, CleanUpRequest, DictationEndpointState,
    PreparedTranscription,
};
use writ_core::config::{AiChatConfig, AiConfig, AiRewriteConfig};
use writ_core::dictation::{
    DictationConfig, DictationEngine, DictationError, DictationProvider, CLEAN_UP_INSTRUCTION,
};
use writ_core::polish::PolishError;

fn ai(provider: &str, base_url: &str, rewrite: bool) -> AiConfig {
    AiConfig {
        provider: provider.to_string(),
        base_url: base_url.to_string(),
        model: "llama3".to_string(),
        consented_hosts: Vec::new(),
        rewrite: AiRewriteConfig { enabled: rewrite },
        chat: AiChatConfig::default(),
    }
}

fn dictation(provider: DictationProvider, base_url: &str) -> DictationConfig {
    DictationConfig {
        enabled: true,
        engine: Some(DictationEngine::Api),
        provider,
        base_url: base_url.to_string(),
        model: "whisper-1".to_string(),
        ..DictationConfig::default()
    }
}

fn consented(mut d: DictationConfig, host: &str) -> DictationConfig {
    d.consented_hosts.push(host.to_string());
    d
}

fn local(base: &str) -> PreparedTranscription {
    prepare_transcription(
        &dictation(DictationProvider::Custom, base),
        &ai("ollama", "", true),
        false,
        |_| panic!("a local endpoint reads no key"),
    )
    .expect("a local endpoint prepares")
}

#[test]
fn an_on_device_session_never_prepares_a_request() {
    let base = "http://127.0.0.1:8000/v1";
    let mut d = dictation(DictationProvider::Custom, base);
    d.engine = Some(DictationEngine::OnDevice);
    let none = |_: &str| None;
    assert_eq!(
        prepare_transcription(&d, &ai("ollama", "", true), true, none),
        Err(DictationError::EndpointNotAllowed)
    );
    assert_eq!(
        prepare_transcription(&d, &ai("ollama", "", true), false, none),
        Err(DictationError::EndpointNotAllowed)
    );
    d.engine = None;
    assert_eq!(
        prepare_transcription(&d, &ai("ollama", "", true), true, none),
        Err(DictationError::EndpointNotAllowed)
    );
    assert!(prepare_transcription(&d, &ai("ollama", "", true), false, none).is_ok());
}

#[test]
fn an_endpoint_the_guard_refuses_is_not_allowed() {
    for base in ["http://example.com/v1", "not a url", ""] {
        let d = consented(dictation(DictationProvider::Custom, base), "example.com");
        assert_eq!(
            prepare_transcription(&d, &ai("ollama", "", true), false, |_| None),
            Err(DictationError::EndpointNotAllowed),
            "{base}"
        );
    }
}

#[test]
fn a_blank_model_is_required() {
    let mut d = dictation(DictationProvider::Custom, "http://127.0.0.1:8000/v1");
    d.model = "  ".to_string();
    assert_eq!(
        prepare_transcription(&d, &ai("ollama", "", true), false, |_| None),
        Err(DictationError::ModelRequired)
    );
}

#[test]
fn a_hosted_service_waits_for_audio_consent() {
    let d = dictation(DictationProvider::OpenAi, "");
    let mut text_allowed = ai("openai", "", true);
    text_allowed
        .consented_hosts
        .push("api.openai.com".to_string());
    assert_eq!(
        prepare_transcription(&d, &text_allowed, false, |_| Some("k".to_string())),
        Err(DictationError::ConsentRequired {
            host: "api.openai.com".to_string()
        })
    );
}

#[test]
fn audio_consent_reads_the_dictation_list_only() {
    let d = consented(DictationConfig::default(), "api.groq.com");
    assert!(is_audio_consented(&d, "api.groq.com"));
    assert!(!is_audio_consented(&d, "groq.com"));
    assert!(!is_audio_consented(
        &DictationConfig::default(),
        "api.groq.com"
    ));
}

#[test]
fn a_consented_service_without_a_key_needs_one() {
    let d = consented(dictation(DictationProvider::Groq, ""), "api.groq.com");
    assert_eq!(
        prepare_transcription(&d, &ai("ollama", "", true), false, |_| None),
        Err(DictationError::KeyRequired {
            host: "api.groq.com".to_string()
        })
    );
}

#[test]
fn a_private_network_server_over_http_goes_without_a_key() {
    let base = "http://192.168.1.20:8000/v1";
    let d = consented(dictation(DictationProvider::Custom, base), "192.168.1.20");
    let p = prepare_transcription(&d, &ai("ollama", "", true), false, |_| {
        panic!("a key never goes over plain http")
    })
    .expect("prepares");
    assert!(!p.has_key());
    assert!(p.is_private_network());
    assert_eq!(
        p.endpoint,
        "http://192.168.1.20:8000/v1/audio/transcriptions"
    );
    let unconsented = dictation(DictationProvider::Custom, base);
    assert_eq!(
        prepare_transcription(&unconsented, &ai("ollama", "", true), false, |_| None),
        Err(DictationError::ConsentRequired {
            host: "192.168.1.20".to_string()
        })
    );
}

#[test]
fn the_key_comes_from_the_ai_account_only_when_both_name_the_same_service() {
    let d = consented(dictation(DictationProvider::OpenAi, ""), "api.openai.com");
    let asked = RefCell::new(Vec::new());
    let shared = prepare_transcription(&d, &ai("openai", "", true), false, |account| {
        asked.borrow_mut().push(account.to_string());
        Some("k".to_string())
    })
    .expect("prepares");
    assert!(shared.has_key());
    assert!(!shared.is_private_network());
    assert_eq!(shared.host, "api.openai.com");
    assert!(shared.endpoint.starts_with("https://api.openai.com/"));
    assert!(shared.endpoint.ends_with("/audio/transcriptions"));
    prepare_transcription(&d, &ai("groq", "", true), false, |account| {
        asked.borrow_mut().push(account.to_string());
        Some("k".to_string())
    })
    .expect("prepares");
    assert_eq!(*asked.borrow(), ["openai", "dictation.openai"]);
}

#[test]
fn the_language_goes_only_when_one_is_chosen() {
    let base = "http://127.0.0.1:8000/v1";
    assert_eq!(local(base).language, None);
    let mut d = dictation(DictationProvider::Custom, base);
    d.language = "ar".to_string();
    let p = prepare_transcription(&d, &ai("ollama", "", true), false, |_| None).unwrap();
    assert_eq!(p.language.as_deref(), Some("ar"));
    assert_eq!(p.model, "whisper-1");
}

#[test]
fn consent_is_recorded_once_and_kept_sorted() {
    let mut d = DictationConfig::default();
    assert!(record_audio_consent(&mut d, "api.openai.com"));
    assert!(record_audio_consent(&mut d, "api.groq.com"));
    assert!(!record_audio_consent(&mut d, "api.openai.com"));
    assert_eq!(d.consented_hosts, ["api.groq.com", "api.openai.com"]);
    assert!(is_audio_consented(&d, "api.openai.com"));
}

const NO_KEY: AiKeyState = AiKeyState {
    is_set: false,
    memory_only: false,
};
const STORED: AiKeyState = AiKeyState {
    is_set: true,
    memory_only: false,
};

#[test]
fn a_hosted_preset_reports_consent_key_and_key_page() {
    let d = dictation(DictationProvider::OpenAi, "");
    let asked = RefCell::new(Vec::new());
    let state = endpoint_state(&d, &ai("ollama", "", true), |account| {
        asked.borrow_mut().push(account.to_string());
        STORED
    });
    assert_eq!(
        state,
        DictationEndpointState {
            host: "api.openai.com".to_string(),
            is_hosted: true,
            is_allowed: true,
            is_consented: false,
            needs_key: true,
            key_shared_with_ai: false,
            key: STORED,
            key_page: Some("https://platform.openai.com/api-keys".to_string()),
        }
    );
    assert_eq!(*asked.borrow(), ["dictation.openai"]);

    let allowed = consented(d, "api.openai.com");
    let state = endpoint_state(&allowed, &ai("openai", "", true), |_| NO_KEY);
    assert!(state.is_consented);
    assert!(state.key_shared_with_ai);
    assert_eq!(state.key, NO_KEY);
}

#[test]
fn a_local_server_needs_nothing_and_reads_no_key() {
    let d = dictation(DictationProvider::Custom, "http://127.0.0.1:8000/v1");
    let state = endpoint_state(&d, &ai("ollama", "", true), |_| {
        panic!("a local endpoint reads no key")
    });
    assert_eq!(
        state,
        DictationEndpointState {
            host: "127.0.0.1".to_string(),
            is_hosted: false,
            is_allowed: true,
            is_consented: true,
            needs_key: false,
            key_shared_with_ai: false,
            key: NO_KEY,
            key_page: None,
        }
    );
}

#[test]
fn an_address_that_does_not_parse_reports_nothing_allowed() {
    let d = dictation(DictationProvider::Custom, "not a url");
    let state = endpoint_state(&d, &ai("ollama", "", true), |_| {
        panic!("an unresolved endpoint reads no key")
    });
    assert_eq!(state.host, "");
    assert!(!state.is_allowed);
    assert!(!state.is_consented);
    assert!(!state.needs_key);
    assert_eq!(state.key, NO_KEY);
}

#[test]
fn the_endpoint_state_serializes_as_the_settings_page_reads_it() {
    let d = dictation(DictationProvider::Groq, "");
    let value = serde_json::to_value(endpoint_state(&d, &ai("ollama", "", true), |_| NO_KEY))
        .expect("serializes");
    let mut keys: Vec<&str> = value
        .as_object()
        .expect("an object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "host",
            "is_allowed",
            "is_consented",
            "is_hosted",
            "key",
            "key_page",
            "key_shared_with_ai",
            "needs_key"
        ]
    );
    assert_eq!(value["key"]["is_set"], false);
    assert_eq!(value["key_page"], "https://console.groq.com/keys");
}

/// One answer a mock connection gives.
#[derive(Clone, Copy)]
enum Reply {
    Respond(&'static str, &'static str),
    Hang,
}

/// A server on a local port that answers one connection per entry of
/// `replies`, in order, and keeps every request it read.
fn mock(replies: Vec<Reply>) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let recorder = Arc::clone(&seen);
    std::thread::spawn(move || {
        for reply in replies {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let request = read_request(&mut stream);
            recorder.lock().expect("recorder").push(request);
            match reply {
                Reply::Respond(status_line, body) => {
                    let response = format!(
                        "{status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = stream.write_all(response.as_bytes());
                    let _ = stream.flush();
                }
                Reply::Hang => std::thread::sleep(Duration::from_secs(10)),
            }
        }
    });
    (format!("http://127.0.0.1:{port}/v1"), seen)
}

fn read_request(stream: &mut TcpStream) -> String {
    let mut data = Vec::new();
    let mut buf = [0u8; 4096];
    while let Ok(n) = stream.read(&mut buf) {
        if n == 0 {
            break;
        }
        data.extend_from_slice(&buf[..n]);
        let text = String::from_utf8_lossy(&data).to_string();
        if let Some(end) = text.find("\r\n\r\n") {
            let length = text[..end]
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())?
                })
                .unwrap_or(0);
            if data.len() >= end + 4 + length {
                break;
            }
        }
    }
    String::from_utf8_lossy(&data).to_string()
}

fn run_transcribe(
    p: &PreparedTranscription,
    prompt: Option<&str>,
    cancel: CancellationToken,
) -> Result<Option<String>, DictationError> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
        .block_on(async {
            let client = build_client().expect("client");
            transcribe(&client, p, b"RIFF-segment".to_vec(), prompt, &cancel).await
        })
}

fn field<'a>(request: &'a str, name: &str) -> Option<&'a str> {
    let marker = format!("name=\"{name}\"");
    let start = request.find(&marker)? + marker.len();
    let rest = &request[start..];
    let body = rest.find("\r\n\r\n")? + 4;
    let end = rest[body..].find("\r\n--")?;
    Some(&rest[body..body + end])
}

#[test]
fn a_segment_goes_as_a_wav_file_with_the_model_and_json_reply() {
    let (base, seen) = mock(vec![Reply::Respond(
        "HTTP/1.1 200 OK",
        r#"{"text":"  Hello there.  "}"#,
    )]);
    let result = run_transcribe(&local(&base), None, CancellationToken::new());
    assert_eq!(result, Ok(Some("Hello there.".to_string())));
    let requests = seen.lock().expect("seen");
    let request = &requests[0];
    assert!(
        request.starts_with("POST /v1/audio/transcriptions "),
        "the request is not a POST to the transcription path"
    );
    assert!(
        request.contains("name=\"file\"; filename=\"segment.wav\"\r\nContent-Type: audio/wav"),
        "the request has no WAV file part"
    );
    assert_eq!(field(request, "file"), Some("RIFF-segment"));
    assert_eq!(field(request, "model"), Some("whisper-1"));
    assert_eq!(field(request, "response_format"), Some("json"));
    assert_eq!(field(request, "language"), None);
    assert_eq!(field(request, "prompt"), None);
    assert!(
        !request.contains("languages"),
        "the request names a languages field"
    );
    assert!(
        !request.to_ascii_lowercase().contains("authorization:"),
        "the request carries an authorization header"
    );
}

#[test]
fn language_and_prompt_go_when_set() {
    let (base, seen) = mock(vec![
        Reply::Respond("HTTP/1.1 200 OK", r#"{"text":"مرحبا"}"#),
        Reply::Respond("HTTP/1.1 200 OK", r#"{"text":"again"}"#),
    ]);
    let mut d = dictation(DictationProvider::Custom, &base);
    d.language = "ar".to_string();
    let p = prepare_transcription(&d, &ai("ollama", "", true), false, |_| None).unwrap();
    assert_eq!(
        run_transcribe(&p, Some("Earlier words."), CancellationToken::new()),
        Ok(Some("مرحبا".to_string()))
    );
    assert_eq!(
        run_transcribe(&p, Some("   "), CancellationToken::new()),
        Ok(Some("again".to_string()))
    );
    let requests = seen.lock().expect("seen");
    assert_eq!(field(&requests[0], "language"), Some("ar"));
    assert_eq!(field(&requests[0], "prompt"), Some("Earlier words."));
    assert_eq!(field(&requests[1], "prompt"), None);
    assert!(requests.iter().all(|r| !r.contains("languages")));
}

#[test]
fn a_groq_reply_reads_the_same_as_an_openai_one() {
    let (base, _) = mock(vec![Reply::Respond(
        "HTTP/1.1 200 OK",
        r#"{"text":" Two words.","x_groq":{"id":"req_01"}}"#,
    )]);
    assert_eq!(
        run_transcribe(&local(&base), None, CancellationToken::new()),
        Ok(Some("Two words.".to_string()))
    );
}

#[test]
fn a_transcript_without_words_is_nothing() {
    for (i, body) in [r#"{"text":"   "}"#, r#"{"text":" ... "}"#]
        .into_iter()
        .enumerate()
    {
        let (base, _) = mock(vec![Reply::Respond("HTTP/1.1 200 OK", body)]);
        assert_eq!(
            run_transcribe(&local(&base), None, CancellationToken::new()),
            Ok(None),
            "reply #{i}"
        );
    }
}

#[test]
fn statuses_map_to_their_errors() {
    let (base, _) = mock(vec![Reply::Respond("HTTP/1.1 401 Unauthorized", "{}")]);
    assert_eq!(
        run_transcribe(&local(&base), None, CancellationToken::new()),
        Err(DictationError::Unauthorized {
            host: "127.0.0.1".to_string()
        })
    );
    let (base, _) = mock(vec![Reply::Respond("HTTP/1.1 404 Not Found", "{}")]);
    assert_eq!(
        run_transcribe(&local(&base), None, CancellationToken::new()),
        Err(DictationError::ModelNotFound {
            host: "127.0.0.1".to_string(),
            model: "whisper-1".to_string()
        })
    );
    let (base, _) = mock(vec![Reply::Respond(
        "HTTP/1.1 400 Bad Request",
        r#"{"error":{"message":"Invalid file format. See https://platform.example.com/docs?key=1 for help."}}"#,
    )]);
    let Err(DictationError::Refused { host, detail }) =
        run_transcribe(&local(&base), None, CancellationToken::new())
    else {
        panic!("expected a refusal");
    };
    assert_eq!(host, "127.0.0.1");
    assert!(
        detail.starts_with("Invalid file format."),
        "the refusal detail lost the service's message"
    );
    assert!(
        !detail.contains("example.com"),
        "the refusal detail carries a link"
    );
}

#[test]
fn rate_limits_and_server_errors_are_retried_once() {
    for (status_line, expected) in [
        (
            "HTTP/1.1 429 Too Many Requests",
            DictationError::RateLimited {
                host: "127.0.0.1".to_string(),
            },
        ),
        (
            "HTTP/1.1 500 Internal Server Error",
            DictationError::Server {
                host: "127.0.0.1".to_string(),
                status: 500,
            },
        ),
    ] {
        let (base, seen) = mock(vec![
            Reply::Respond(status_line, "{}"),
            Reply::Respond(status_line, "{}"),
        ]);
        assert_eq!(
            run_transcribe(&local(&base), None, CancellationToken::new()),
            Err(expected)
        );
        assert_eq!(seen.lock().expect("seen").len(), 2, "{status_line}");
    }
    let (base, seen) = mock(vec![
        Reply::Respond("HTTP/1.1 503 Service Unavailable", "{}"),
        Reply::Respond("HTTP/1.1 200 OK", r#"{"text":"made it"}"#),
    ]);
    assert_eq!(
        run_transcribe(&local(&base), None, CancellationToken::new()),
        Ok(Some("made it".to_string()))
    );
    assert_eq!(seen.lock().expect("seen").len(), 2);
}

#[test]
fn a_refusal_is_not_retried() {
    let (base, seen) = mock(vec![
        Reply::Respond("HTTP/1.1 401 Unauthorized", "{}"),
        Reply::Respond("HTTP/1.1 200 OK", r#"{"text":"no"}"#),
    ]);
    assert!(run_transcribe(&local(&base), None, CancellationToken::new()).is_err());
    assert_eq!(seen.lock().expect("seen").len(), 1);
}

#[test]
fn a_closed_port_is_a_network_failure() {
    let port = {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        listener.local_addr().expect("addr").port()
    };
    assert_eq!(
        run_transcribe(
            &local(&format!("http://127.0.0.1:{port}/v1")),
            None,
            CancellationToken::new()
        ),
        Err(DictationError::Network {
            host: "127.0.0.1".to_string()
        })
    );
}

#[test]
fn a_stop_ends_a_request_the_server_has_not_answered() {
    let (base, _) = mock(vec![Reply::Hang]);
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        trigger.cancel();
    });
    let started = Instant::now();
    assert_eq!(run_transcribe(&local(&base), None, cancel), Ok(None));
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[test]
fn the_clean_up_runs_with_rewrite_off() {
    let request = prepare_clean_up(
        &ai("custom", "http://127.0.0.1:11434/v1", false),
        "hello world",
        |_| None,
    )
    .expect("prepares");
    assert!(matches!(request, CleanUpRequest::Hosted(_)));
    assert_eq!(
        prepare_request(
            &ai("custom", "http://127.0.0.1:11434/v1", false),
            "proofread",
            "hello world",
            None,
            |_| None
        ),
        Err(PolishError::Disabled)
    );
}

#[test]
fn the_clean_up_needs_the_ai_hosts_consent_and_key() {
    let mut cfg = ai("openai", "", false);
    assert_eq!(
        prepare_clean_up(&cfg, "hello", |_| Some("k".to_string())),
        Err(PolishError::ConsentRequired {
            host: "api.openai.com".to_string()
        })
    );
    cfg.consented_hosts.push("api.openai.com".to_string());
    assert_eq!(
        prepare_clean_up(&cfg, "hello", |_| None),
        Err(PolishError::ApiKeyRequired {
            host: "api.openai.com".to_string()
        })
    );
    assert!(matches!(
        prepare_clean_up(&cfg, "hello", |_| Some("k".to_string())),
        Ok(CleanUpRequest::Hosted(_))
    ));
}

#[test]
fn the_clean_up_debug_form_leaves_out_the_key_and_url_credentials() {
    let mut cfg = ai(
        "custom",
        "https://user:pass-zz@api.example.com/v1?token=tok-zz",
        false,
    );
    cfg.consented_hosts.push("api.example.com".to_string());
    let request = prepare_clean_up(&cfg, "private words", |_| Some("sk-zz-key".to_string()))
        .expect("a consented endpoint with a key prepares");
    let printed = format!("{request:?}");
    for (i, secret) in ["sk-zz-key", "pass-zz", "user", "tok-zz", "private words"]
        .into_iter()
        .enumerate()
    {
        assert!(
            !printed.contains(secret),
            "secret #{i} appears in the debug form"
        );
    }
    assert!(
        printed.contains("api.example.com"),
        "the debug form does not show the host"
    );
}

#[test]
fn the_clean_up_on_the_on_device_model_carries_the_instruction() {
    let Ok(CleanUpRequest::OnDevice(request)) =
        prepare_clean_up(&ai("apple", "", false), "hello world", |_| None)
    else {
        panic!("expected an on-device request");
    };
    assert!(request.instructions.contains(CLEAN_UP_INSTRUCTION));
    assert!(request.prompt.contains("hello world"));
}

const CLEANED_STREAM: &str = "data: {\"choices\":[{\"delta\":{\"content\":\"Hello,\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\" world.\"}}]}\n\ndata: [DONE]\n\n";

fn run_hosted_clean_up(base: &str) -> Result<String, String> {
    let Ok(CleanUpRequest::Hosted(request)) =
        prepare_clean_up(&ai("custom", base, false), "hello world", |_| None)
    else {
        panic!("expected a hosted request");
    };
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
        .block_on(async {
            let client = build_client().expect("client");
            run_clean_up(&client, &request, &CancellationToken::new()).await
        })
}

#[test]
fn a_hosted_clean_up_returns_the_whole_reply() {
    let (base, seen) = mock(vec![Reply::Respond("HTTP/1.1 200 OK", CLEANED_STREAM)]);
    assert_eq!(run_hosted_clean_up(&base), Ok("Hello, world.".to_string()));
    let requests = seen.lock().expect("seen");
    assert!(requests[0].contains("Fix punctuation, capitalization"));
    assert!(requests[0].contains("hello world"));
}

#[test]
fn a_failed_clean_up_reports_why() {
    let (base, _) = mock(vec![Reply::Respond(
        "HTTP/1.1 500 Internal Server Error",
        "{}",
    )]);
    assert!(run_hosted_clean_up(&base).is_err());
    let (base, _) = mock(vec![Reply::Respond(
        "HTTP/1.1 200 OK",
        "data: {\"choices\":[{\"delta\":{\"content\":\"  \"}}]}\n\ndata: [DONE]\n\n",
    )]);
    assert_eq!(
        run_hosted_clean_up(&base),
        Err("The clean-up returned no text.".to_string())
    );
}
