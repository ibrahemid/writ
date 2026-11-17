use std::collections::HashSet;

use writ_core::ai::providers;
use writ_core::config::{AiConfig, DictationConfig, DictationProvider};
use writ_core::dictation::error::DictationErrorCode;
use writ_core::dictation::on_device::language_name;
use writ_core::dictation::platform::{Platform, MAC_MIC_SETTINGS_URI, WINDOWS_MIC_SETTINGS_URI};
use writ_core::dictation::{
    clean_transcript, encode_wav_16k_mono, error_from_bridge, frame_level, is_digital_silence,
    key_account, language, on_device_locale, presets, prompt_tail, DictationAction, DictationError,
    DictationEvent, DictationFailure, LANGUAGES, PRESETS,
};

fn strings(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|s| s.to_string()).collect()
}

/// The locales DictationTranscriber reports on macOS 26.5.
fn on_device_supported() -> Vec<String> {
    strings(&[
        "ar-SA", "ca-ES", "cs-CZ", "da-DK", "de-AT", "de-CH", "de-DE", "el-GR", "en-AU", "en-CA",
        "en-GB", "en-IE", "en-IN", "en-NZ", "en-SG", "en-US", "en-ZA", "es-CL", "es-ES", "es-MX",
        "es-US", "fi-FI", "fr-BE", "fr-CA", "fr-CH", "fr-FR", "he-IL", "hi-IN", "hr-HR", "hu-HU",
        "id-ID", "it-CH", "it-IT", "ja-JP", "ko-KR", "ms-MY", "nb-NO", "nl-BE", "nl-NL", "pl-PL",
        "pt-BR", "pt-PT", "ro-RO", "ru-RU", "sk-SK", "sv-SE", "th-TH", "tr-TR", "uk-UA", "vi-VN",
        "yue-CN", "zh-CN", "zh-HK", "zh-TW",
    ])
}

#[test]
fn a_wav_is_a_44_byte_header_and_little_endian_samples() {
    let wav = encode_wav_16k_mono(&[1, -2, i16::MAX]);
    assert_eq!(wav.len(), 44 + 2 * 3);
    assert_eq!(&wav[0..4], b"RIFF");
    assert_eq!(u32::from_le_bytes(wav[4..8].try_into().unwrap()), 36 + 6);
    assert_eq!(&wav[8..16], b"WAVEfmt ");
    assert_eq!(u16::from_le_bytes([wav[20], wav[21]]), 1);
    assert_eq!(u16::from_le_bytes([wav[22], wav[23]]), 1);
    assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 16_000);
    assert_eq!(u32::from_le_bytes(wav[28..32].try_into().unwrap()), 32_000);
    assert_eq!(u16::from_le_bytes([wav[34], wav[35]]), 16);
    assert_eq!(&wav[36..40], b"data");
    assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()), 6);
    assert_eq!(&wav[44..], &[1, 0, 0xfe, 0xff, 0xff, 0x7f]);
    assert_eq!(encode_wav_16k_mono(&[]).len(), 44);
}

#[test]
fn the_level_rises_with_loudness() {
    let at = |amp: f32| -> f32 {
        let frame: Vec<i16> = (0..480)
            .map(|i| ((i as f32 * 0.2).sin() * amp * f32::from(i16::MAX)) as i16)
            .collect();
        frame_level(&frame)
    };
    assert_eq!(frame_level(&[0; 480]), 0.0);
    assert_eq!(frame_level(&[]), 0.0);
    let levels = [at(0.001), at(0.01), at(0.1), at(0.5), at(1.0)];
    for pair in levels.windows(2) {
        assert!(pair[0] < pair[1], "{levels:?}");
    }
    assert!(levels[4] > 0.9 && levels[4] <= 1.0, "{levels:?}");
}

#[test]
fn digital_silence_is_every_sample_zero() {
    assert!(is_digital_silence(&[0; 320]));
    assert!(is_digital_silence(&[]));
    assert!(!is_digital_silence(&[0, 0, 1, 0]));
}

#[test]
fn the_language_list_is_unique_sorted_and_has_arabic_and_english() {
    assert_eq!(LANGUAGES.len(), 57);
    let codes: HashSet<&str> = LANGUAGES.iter().map(|l| l.code).collect();
    assert_eq!(codes.len(), LANGUAGES.len());
    for pair in LANGUAGES.windows(2) {
        assert!(
            pair[0].name < pair[1].name,
            "{} before {}",
            pair[0].name,
            pair[1].name
        );
    }
    assert_eq!(language("ar").unwrap().name, "Arabic");
    assert_eq!(language("en").unwrap().name, "English");
    assert!(language("xx").is_none());
}

#[test]
fn a_language_picks_its_on_device_locale() {
    let supported = on_device_supported();
    assert_eq!(
        on_device_locale("ar", "ar_SA", &supported).as_deref(),
        Some("ar-SA")
    );
    assert_eq!(
        on_device_locale("en", "ar_SA", &supported).as_deref(),
        Some("en-AU")
    );
    assert_eq!(
        on_device_locale("en", "en_GB@calendar=gregorian", &supported).as_deref(),
        Some("en-GB")
    );
    assert_eq!(
        on_device_locale("no", "nb_NO", &supported).as_deref(),
        Some("nb-NO")
    );
    assert_eq!(on_device_locale("cy", "en_GB", &supported), None);
}

#[test]
fn automatic_uses_the_system_language_on_device() {
    let supported = on_device_supported();
    assert_eq!(
        on_device_locale("", "en_GB", &supported).as_deref(),
        Some("en-GB")
    );
    assert_eq!(
        on_device_locale("", "de-CH", &supported).as_deref(),
        Some("de-CH")
    );
    assert_eq!(on_device_locale("", "cy_GB", &supported), None);
    assert_eq!(on_device_locale("", "", &supported), None);
    assert_eq!(on_device_locale("en", "en_US", &[]), None);
}

#[test]
fn the_on_device_languages_are_the_listed_ones_it_has() {
    let langs = writ_core::dictation::language::on_device_languages(&on_device_supported());
    assert!(langs.contains(&"ar".to_string()));
    assert!(langs.contains(&"no".to_string()));
    assert!(langs.contains(&"zh".to_string()));
    assert!(!langs.contains(&"cy".to_string()));
    assert_eq!(
        writ_core::dictation::language::locale_language("zh-Hant_TW"),
        "zh"
    );
}

#[test]
fn the_prompt_tail_cuts_arabic_on_a_character() {
    let text = "مرحبا بالعالم";
    assert_eq!(prompt_tail(text, 5), "لعالم");
    assert_eq!(prompt_tail(text, 100), text);
    assert_eq!(prompt_tail(text, 0), "");
    assert_eq!(prompt_tail("abc", 2), "bc");
}

#[test]
fn a_transcript_is_trimmed_and_punctuation_alone_is_nothing() {
    assert_eq!(
        clean_transcript("  hello there \n").as_deref(),
        Some("hello there")
    );
    assert_eq!(clean_transcript(" مرحبا ").as_deref(), Some("مرحبا"));
    assert_eq!(clean_transcript("١٢").as_deref(), Some("١٢"));
    assert_eq!(clean_transcript(""), None);
    assert_eq!(clean_transcript(" ... "), None);
    assert_eq!(clean_transcript("،"), None);
}

#[test]
fn presets_read_the_provider_table() {
    assert_eq!(PRESETS.len(), 2);
    for (provider, model) in [
        (DictationProvider::OpenAi, "gpt-transcribe"),
        (DictationProvider::Groq, "whisper-large-v3-turbo"),
    ] {
        let preset = presets::preset(provider).unwrap();
        assert_eq!(preset.model, model);
        let row = providers::provider(provider.id()).unwrap();
        assert_eq!(presets::base_url(provider), Some(row.base_url));
        assert_eq!(presets::key_page(provider), row.key_page_url);
        assert!(presets::key_page(provider).is_some());
    }
    assert_eq!(presets::preset(DictationProvider::Custom), None);
    assert_eq!(presets::base_url(DictationProvider::Custom), None);
    assert_eq!(presets::key_page(DictationProvider::Custom), None);
    assert_eq!(
        presets::preset(DictationProvider::OpenAi).unwrap().label,
        "OpenAI"
    );
    assert_eq!(
        presets::preset(DictationProvider::Groq).unwrap().label,
        "Groq"
    );
}

fn ai(provider: &str, base_url: &str) -> AiConfig {
    AiConfig {
        provider: provider.to_string(),
        base_url: base_url.to_string(),
        ..AiConfig::default()
    }
}

fn dictation(provider: DictationProvider, base_url: &str) -> DictationConfig {
    DictationConfig {
        provider,
        base_url: base_url.to_string(),
        ..DictationConfig::default()
    }
}

#[test]
fn the_same_provider_and_host_share_the_ai_key() {
    let account = key_account(&dictation(DictationProvider::OpenAi, ""), &ai("openai", ""));
    assert_eq!(account.account, "openai");
    assert!(account.shared_with_ai);

    let lan = "http://192.168.1.20:8000/v1";
    let account = key_account(
        &dictation(DictationProvider::Custom, lan),
        &ai("custom", lan),
    );
    assert_eq!(account.account, "custom");
    assert!(account.shared_with_ai);
}

#[test]
fn another_provider_or_host_has_its_own_key() {
    let own = |d: DictationConfig, a: AiConfig, expected: &str| {
        let account = key_account(&d, &a);
        assert_eq!(account.account, expected);
        assert!(!account.shared_with_ai);
    };
    own(
        dictation(DictationProvider::Groq, ""),
        ai("openai", ""),
        "dictation.groq",
    );
    own(
        dictation(DictationProvider::OpenAi, ""),
        AiConfig::default(),
        "dictation.openai",
    );
    own(
        dictation(DictationProvider::Custom, "http://192.168.1.20:8000/v1"),
        ai("custom", "http://192.168.1.30:8000/v1"),
        "dictation.custom",
    );
    own(
        dictation(DictationProvider::Custom, ""),
        ai("custom", ""),
        "dictation.custom",
    );
}

fn every_error() -> Vec<DictationError> {
    let host = || "api.example.com".to_string();
    vec![
        DictationError::Disabled,
        DictationError::Busy,
        DictationError::MicDenied,
        DictationError::Silence,
        DictationError::NoInputDevice,
        DictationError::CaptureFailed("device unplugged".to_string()),
        DictationError::EndpointNotAllowed,
        DictationError::ModelRequired,
        DictationError::ConsentRequired { host: host() },
        DictationError::KeyRequired { host: host() },
        DictationError::KeyShared,
        DictationError::Unauthorized { host: host() },
        DictationError::ModelNotFound {
            host: host(),
            model: "whisper-9".to_string(),
        },
        DictationError::RateLimited { host: host() },
        DictationError::Server {
            host: host(),
            status: 502,
        },
        DictationError::Refused {
            host: host(),
            detail: "file too short".to_string(),
        },
        DictationError::Network { host: host() },
        DictationError::UnsupportedOs,
        DictationError::LanguageUnsupported {
            language: "Welsh".to_string(),
        },
        DictationError::AssetsFailed {
            language: "Arabic".to_string(),
        },
        DictationError::EngineFailed { code: 9 },
        DictationError::NoSpeech,
    ]
}

#[test]
fn every_error_has_its_own_code_matching_the_frontend_union() {
    let codes: Vec<String> = every_error()
        .iter()
        .map(|e| {
            serde_json::to_value(e.code())
                .unwrap()
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();
    assert_eq!(
        codes,
        [
            "disabled",
            "busy",
            "mic_denied",
            "silence",
            "no_input_device",
            "capture_failed",
            "endpoint_not_allowed",
            "model_required",
            "consent_required",
            "key_required",
            "key_shared",
            "unauthorized",
            "model_not_found",
            "rate_limited",
            "server",
            "refused",
            "network",
            "unsupported_os",
            "language_unsupported",
            "assets_failed",
            "engine_failed",
            "no_speech",
        ]
    );
}

#[test]
fn errors_that_carry_a_host_name_it() {
    for error in every_error() {
        let message = error.message_on(Platform::Mac);
        assert!(!message.is_empty());
        if let Some(host) = error.host() {
            assert_eq!(host, "api.example.com");
            assert!(message.contains(host), "{message}");
        }
    }
    assert_eq!(
        DictationError::ModelNotFound {
            host: "api.groq.com".to_string(),
            model: "whisper-9".to_string()
        }
        .message_on(Platform::Mac),
        "api.groq.com has no model named whisper-9."
    );
    assert_eq!(
        DictationError::CaptureFailed("device unplugged".to_string()).message_on(Platform::Mac),
        "The microphone stopped: device unplugged."
    );
}

#[test]
fn the_microphone_messages_name_each_systems_setting() {
    let denied = DictationError::MicDenied;
    assert!(denied
        .message_on(Platform::Mac)
        .contains("System Settings > Privacy & Security > Microphone"));
    assert!(denied
        .message_on(Platform::Windows)
        .contains("Settings > Privacy & security > Microphone"));
    assert!(denied
        .message_on(Platform::Linux)
        .contains("sound settings"));
    for platform in [Platform::Mac, Platform::Windows] {
        assert_eq!(
            denied.action_on(platform),
            Some(DictationAction::OpenMicSettings)
        );
        assert_eq!(
            DictationError::Silence.action_on(platform),
            Some(DictationAction::OpenMicSettings)
        );
    }
    assert_eq!(denied.action_on(Platform::Linux), None);
    assert_eq!(DictationError::Silence.action_on(Platform::Linux), None);
    assert_eq!(Platform::Mac.mic_settings_uri(), Some(MAC_MIC_SETTINGS_URI));
    assert_eq!(
        Platform::Windows.mic_settings_uri(),
        Some(WINDOWS_MIC_SETTINGS_URI)
    );
    assert_eq!(Platform::Linux.mic_settings_uri(), None);
}

#[test]
fn settings_errors_offer_the_dictation_page() {
    let settings: HashSet<DictationErrorCode> = [
        DictationErrorCode::KeyRequired,
        DictationErrorCode::ConsentRequired,
        DictationErrorCode::Unauthorized,
        DictationErrorCode::ModelNotFound,
        DictationErrorCode::EndpointNotAllowed,
        DictationErrorCode::ModelRequired,
        DictationErrorCode::UnsupportedOs,
        DictationErrorCode::LanguageUnsupported,
    ]
    .into();
    for error in every_error() {
        let action = error.action_on(Platform::Mac);
        if settings.contains(&error.code()) {
            assert_eq!(
                action,
                Some(DictationAction::OpenDictationSettings),
                "{error:?}"
            );
        } else if !matches!(error, DictationError::MicDenied | DictationError::Silence) {
            assert_eq!(action, None, "{error:?}");
        }
    }
}

#[test]
fn only_the_network_rate_limits_and_5xx_are_retried() {
    let host = || "h".to_string();
    assert!(DictationError::Network { host: host() }.is_transient());
    assert!(DictationError::RateLimited { host: host() }.is_transient());
    assert!(DictationError::Server {
        host: host(),
        status: 503
    }
    .is_transient());
    assert!(!DictationError::Server {
        host: host(),
        status: 418
    }
    .is_transient());
    assert!(!DictationError::Unauthorized { host: host() }.is_transient());
    assert!(!DictationError::NoSpeech.is_transient());
}

#[test]
fn a_failure_serializes_as_the_frontend_reads_it() {
    let failure: DictationFailure = DictationError::KeyRequired {
        host: "api.openai.com".to_string(),
    }
    .failure_on(Platform::Mac);
    assert_eq!(
        serde_json::to_value(&failure).unwrap(),
        serde_json::json!({
            "code": "key_required",
            "message": "Add an API key for api.openai.com in Settings > Apps > Dictation.",
            "action": "open_dictation_settings",
        })
    );
    let none = DictationError::NoSpeech.failure_on(Platform::Linux);
    assert_eq!(
        serde_json::to_value(&none).unwrap()["action"],
        serde_json::Value::Null
    );
}

#[test]
fn events_serialize_with_a_kind_tag() {
    let cases = [
        (
            DictationEvent::Preparing { progress: None },
            serde_json::json!({"kind": "preparing", "progress": null}),
        ),
        (
            DictationEvent::Preparing {
                progress: Some(0.5),
            },
            serde_json::json!({"kind": "preparing", "progress": 0.5}),
        ),
        (
            DictationEvent::Recording,
            serde_json::json!({"kind": "recording"}),
        ),
        (
            DictationEvent::Level { level: 0.25 },
            serde_json::json!({"kind": "level", "level": 0.25}),
        ),
        (
            DictationEvent::Partial {
                text: "hel".to_string(),
            },
            serde_json::json!({"kind": "partial", "text": "hel"}),
        ),
        (
            DictationEvent::Transcribing { pending: 2 },
            serde_json::json!({"kind": "transcribing", "pending": 2}),
        ),
        (
            DictationEvent::Segment {
                seq: 3,
                text: "hello".to_string(),
            },
            serde_json::json!({"kind": "segment", "seq": 3, "text": "hello"}),
        ),
        (
            DictationEvent::clean_up_skipped("the AI connection is off"),
            serde_json::json!({"kind": "notice", "message": "Inserted without clean-up: the AI connection is off"}),
        ),
        (DictationEvent::Done, serde_json::json!({"kind": "done"})),
        (
            DictationEvent::Cancelled,
            serde_json::json!({"kind": "cancelled"}),
        ),
        (
            DictationEvent::Error {
                failure: DictationError::NoSpeech.failure_on(Platform::Mac),
            },
            serde_json::json!({"kind": "error", "failure": {"code": "no_speech", "message": "No speech heard.", "action": null}}),
        ),
    ];
    for (event, expected) in cases {
        assert_eq!(serde_json::to_value(&event).unwrap(), expected);
    }
    assert!(DictationEvent::Done.is_terminal());
    assert!(DictationEvent::Cancelled.is_terminal());
    assert!(!DictationEvent::Recording.is_terminal());
}

#[test]
fn bridge_codes_map_to_errors() {
    assert_eq!(error_from_bridge(1, "ar"), DictationError::UnsupportedOs);
    assert_eq!(
        error_from_bridge(2, "cy"),
        DictationError::LanguageUnsupported {
            language: "Welsh".to_string()
        }
    );
    assert_eq!(
        error_from_bridge(3, "ar-SA"),
        DictationError::AssetsFailed {
            language: "Arabic".to_string()
        }
    );
    assert_eq!(error_from_bridge(5, "en"), DictationError::MicDenied);
    assert_eq!(
        error_from_bridge(4, "en"),
        DictationError::EngineFailed { code: 4 }
    );
    assert_eq!(
        error_from_bridge(-7, "en"),
        DictationError::EngineFailed { code: -7 }
    );
    assert_eq!(language_name("nb-NO"), "Norwegian");
    assert_eq!(language_name("yue-CN"), "yue-CN");
}
