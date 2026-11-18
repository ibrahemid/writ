//! IPC surface for Dictation: what this machine supports, the endpoint and key
//! rows of its settings page, and a session's start, stop and cancel.
//!
//! The policy is `jotra_core::dictation`, the requests
//! `jotra_ai::transcription`, the session [`crate::dictation`]. These commands
//! read the config, check it, open the microphone and hand the rest to the
//! session driver, which reports over the start's channel.

use std::sync::Arc;

use jotra_ai::connection::{build_client_for, needs_key_lookup};
use jotra_ai::transcription::{
    self, prepare_transcription, record_audio_consent, DictationEndpointState,
};
use jotra_audio::{AudioFrame, CaptureError, Recorder};
use jotra_core::config::{AiConfig, JotraConfig};
use jotra_core::dictation::key::own_account;
use jotra_core::dictation::language::{locale_language, on_device_languages};
use jotra_core::dictation::on_device::BRIDGE_LANGUAGE_UNSUPPORTED;
use jotra_core::dictation::platform::mic_settings_uri;
use jotra_core::dictation::{
    error_from_bridge, key_account, on_device_locale, DictationConfig, DictationEngine,
    DictationError, DictationEvent, DictationFailure, Segmenter, SegmenterParams, SessionLimits,
    LANGUAGES,
};
use jotra_core::polish;
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};
use tokio::sync::mpsc;

use super::ai::{key_for, key_state_for, AiKeyState, AiState};
use super::config::mutate_config;
use super::offload::{run_blocking, ERR_COMMAND_STOPPED};
use crate::dictation::bridge::{self, MicStatus};
use crate::dictation::session::{
    self, AiCleanUp, ApiSession, ApiTranscriber, CleanUp, Emitter, OnDeviceSession,
};
use crate::dictation::window::SightWatch;
use crate::dictation::{DictationState, ReleaseOnDrop};
use crate::events::{emit_event, JotraFrontendEvent};
use crate::poison::recover_poison;
use crate::state::AppState;

/// What this machine offers dictation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DictationCapabilities {
    /// Apple's on-device speech analyzer answers here with at least one locale.
    pub on_device: bool,
    /// The listed language codes the on-device engine has a locale for.
    pub on_device_languages: Vec<String>,
    /// The language subtag of the person's first preferred language; empty
    /// where there is no bridge.
    pub system_language: String,
    /// The system has a microphone settings page to open.
    pub mic_settings: bool,
    /// The engine a session would run on now.
    pub effective_engine: DictationEngine,
}

/// One language the settings list offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DictationLanguageDto {
    /// ISO-639-1 code.
    pub code: String,
    /// English name.
    pub name: String,
}

/// The session a start opened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DictationSession {
    /// The id stop and cancel name.
    pub session_id: u32,
    /// The engine it runs on.
    pub engine: DictationEngine,
    /// The on-device locale; `None` on the API engine.
    pub locale: Option<String>,
}

/// The answer to a consent request for an endpoint on this machine.
const LOCAL_ENDPOINT: &str = "This endpoint is on your machine; nothing is sent.";
/// The answer to opening microphone settings where the system has none.
const NO_MIC_SETTINGS: &str = "This system has no microphone settings page.";

fn read_config(app: &AppHandle, location: &'static str) -> JotraConfig {
    let state = app.state::<AppState>();
    let guard = recover_poison(state.config.lock(), location);
    guard.clone()
}

/// The locales the bridge answered, or none.
fn supported(locales: &Result<Vec<String>, i32>) -> &[String] {
    locales.as_deref().unwrap_or(&[])
}

/// The capabilities for what the bridge answered.
fn capabilities_from(
    d: &DictationConfig,
    locales: &Result<Vec<String>, i32>,
    system_locale: &str,
    mic_settings: bool,
) -> DictationCapabilities {
    let supported = supported(locales);
    DictationCapabilities {
        on_device: !supported.is_empty(),
        on_device_languages: on_device_languages(supported),
        system_language: locale_language(system_locale),
        mic_settings,
        effective_engine: d.effective_engine(!supported.is_empty()),
    }
}

/// Refuses a start while the app is off.
fn check_enabled(d: &DictationConfig) -> Result<(), DictationError> {
    if d.enabled {
        Ok(())
    } else {
        Err(DictationError::Disabled)
    }
}

/// The account dictation's own key lives under. Refused while dictation reads
/// the AI connection's key, which only the AI pages change, and for a custom
/// server whose base URL names no address to keep the key for.
fn own_key_account(d: &DictationConfig, ai: &AiConfig) -> Result<String, DictationError> {
    if key_account(d, ai).shared_with_ai {
        return Err(DictationError::KeyShared);
    }
    own_account(d).ok_or(DictationError::EndpointNotAllowed)
}

/// The on-device locale a start runs in, or why there is none.
fn on_device_start_locale(
    d: &DictationConfig,
    locales: &Result<Vec<String>, i32>,
    system_locale: &str,
) -> Result<String, DictationError> {
    let supported = match locales {
        Ok(list) if !list.is_empty() => list,
        Ok(_) => return Err(DictationError::UnsupportedOs),
        Err(code) => return Err(error_from_bridge(*code, &d.language)),
    };
    let wanted = match (d.language.trim(), system_locale.trim()) {
        ("", "") => "the system language",
        ("", system) => system,
        (chosen, _) => chosen,
    };
    on_device_locale(&d.language, system_locale, supported)
        .ok_or_else(|| error_from_bridge(BRIDGE_LANGUAGE_UNSUPPORTED, wanted))
}

/// Records audio consent for the host the transcription endpoint reaches,
/// under the config lock and on disk. Answers whether the list changed; a
/// failed write leaves memory and disk as they were. Refuses a local or
/// disallowed endpoint: there is nothing to consent to.
pub fn consent_to_endpoint(state: &AppState) -> Result<bool, String> {
    let mut refusal = None;
    let changed = mutate_config(state, |config| {
        apply_audio_consent(&mut config.dictation).unwrap_or_else(|reason| {
            refusal = Some(reason);
            false
        })
    })?;
    match refusal {
        Some(reason) => Err(reason),
        None => Ok(changed),
    }
}

/// Adds the transcription endpoint's host to `dictation.consented_hosts`.
/// Answers whether the list changed, or why there is nothing to consent to.
fn apply_audio_consent(d: &mut DictationConfig) -> Result<bool, String> {
    match polish::resolve_endpoint(&d.effective_base_url()) {
        Ok(target) if target.is_allowed && target.is_hosted => {
            Ok(record_audio_consent(d, &target.host))
        }
        Ok(target) if target.is_allowed => Err(LOCAL_ENDPOINT.to_string()),
        Ok(_) | Err(_) => Err(DictationError::EndpointNotAllowed.to_string()),
    }
}

/// Tells the frontend the `[dictation]` section changed, so its next settings
/// write does not send back a copy read before this one.
fn announce_dictation_config(app: &AppHandle) {
    if let Err(error) = emit_event(
        app,
        JotraFrontendEvent::ConfigChanged {
            keys: vec!["dictation".to_string()],
        },
    ) {
        tracing::warn!(error = %error, "failed to emit config event");
    }
}

/// IPC: what this machine supports, for the settings page.
#[tauri::command]
pub async fn dictation_capabilities(app: AppHandle) -> Result<DictationCapabilities, String> {
    let config = read_config(&app, "commands::dictation::dictation_capabilities");
    let locales = bridge::supported_locales().await;
    Ok(capabilities_from(
        &config.dictation,
        &locales,
        &bridge::system_locale(),
        mic_settings_uri().is_some(),
    ))
}

/// IPC: the languages the settings list offers.
#[tauri::command]
pub fn dictation_languages() -> Vec<DictationLanguageDto> {
    LANGUAGES
        .iter()
        .map(|l| DictationLanguageDto {
            code: l.code.to_string(),
            name: l.name.to_string(),
        })
        .collect()
}

/// IPC: where the transcription endpoint points and what it still needs.
#[tauri::command]
pub fn dictation_endpoint_state(
    app: AppHandle,
    ai: State<'_, AiState>,
) -> Result<DictationEndpointState, String> {
    let config = read_config(&app, "commands::dictation::dictation_endpoint_state");
    Ok(transcription::endpoint_state(
        &config.dictation,
        &config.ai,
        |account| ai.key_state(account),
    ))
}

/// IPC: allows audio to go to the host the transcription endpoint reaches.
/// The host is resolved here, so consent is stored under the exact string
/// the session's check reads.
#[tauri::command]
pub async fn dictation_consent_host(app: AppHandle) -> Result<DictationEndpointState, String> {
    let task_app = app.clone();
    run_blocking(app, ERR_COMMAND_STOPPED, move |state| {
        if consent_to_endpoint(state)? {
            announce_dictation_config(&task_app);
        }
        let config = read_config(&task_app, "commands::dictation::dictation_consent_host");
        Ok(transcription::endpoint_state(
            &config.dictation,
            &config.ai,
            |account| key_state_for(&task_app, account),
        ))
    })
    .await
}

/// IPC: stores dictation's own key. Refused while it reads the AI
/// connection's key.
#[tauri::command]
pub fn dictation_set_key(
    app: AppHandle,
    ai: State<'_, AiState>,
    key: String,
) -> Result<AiKeyState, String> {
    let config = read_config(&app, "commands::dictation::dictation_set_key");
    let account = own_key_account(&config.dictation, &config.ai).map_err(|e| e.to_string())?;
    ai.set_key(&account, &key)
}

/// IPC: removes dictation's own key. Refused while it reads the AI
/// connection's key.
#[tauri::command]
pub fn dictation_clear_key(app: AppHandle, ai: State<'_, AiState>) -> Result<AiKeyState, String> {
    let config = read_config(&app, "commands::dictation::dictation_clear_key");
    let account = own_key_account(&config.dictation, &config.ai).map_err(|e| e.to_string())?;
    Ok(ai.clear_key(&account))
}

/// IPC: starts a session. Every check runs before the microphone opens; the
/// session then reports over `on_event` until one terminal event.
#[tauri::command]
pub async fn dictation_start(
    app: AppHandle,
    on_event: Channel<DictationEvent>,
) -> Result<DictationSession, DictationFailure> {
    start_session(app, on_event).await.map_err(|error| {
        tracing::info!(code = ?error.code(), "dictation did not start");
        error.failure()
    })
}

/// IPC: ends session `session_id`; what was heard is still inserted. An id
/// that is not live has already ended.
#[tauri::command]
pub fn dictation_stop(state: State<'_, DictationState>, session_id: u32) -> Result<(), String> {
    state.stop(session_id);
    Ok(())
}

/// IPC: cancels session `session_id`; what is not inserted yet is discarded.
#[tauri::command]
pub fn dictation_cancel(state: State<'_, DictationState>, session_id: u32) -> Result<(), String> {
    state.cancel(session_id);
    Ok(())
}

/// IPC: opens the system's microphone permission page.
#[tauri::command]
pub async fn dictation_open_mic_settings(app: AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let uri = mic_settings_uri().ok_or_else(|| NO_MIC_SETTINGS.to_string())?;
    app.opener()
        .open_url(uri, None::<&str>)
        .map_err(|e| e.to_string())
}

/// Asks for the microphone where the system has not asked yet. Off macOS the
/// bridge reports it allowed and the capture's own error speaks for it.
async fn ensure_mic() -> Result<(), DictationError> {
    match bridge::mic_status() {
        MicStatus::Authorized => Ok(()),
        MicStatus::NotDetermined if bridge::request_mic().await => Ok(()),
        MicStatus::NotDetermined | MicStatus::Denied | MicStatus::Restricted => {
            Err(DictationError::MicDenied)
        }
    }
}

/// Opens the default input device off the async workers.
async fn open_microphone() -> Result<(Recorder, mpsc::Receiver<AudioFrame>), DictationError> {
    let started = tauri::async_runtime::spawn_blocking(Recorder::start)
        .await
        .map_err(|error| {
            tracing::error!(error = %error, "the capture thread did not answer");
            DictationError::from(CaptureError::Backend(
                "the capture did not start".to_string(),
            ))
        })?;
    started.map_err(DictationError::from)
}

/// The clean-up pass the session runs, when it is on. The connection's key is
/// read now, so no segment waits on the key store.
fn clean_up_for(app: &AppHandle, d: &DictationConfig, ai: &AiConfig) -> Option<Arc<dyn CleanUp>> {
    if !d.clean_up {
        return None;
    }
    let key = if needs_key_lookup(ai) {
        key_for(app, &ai.provider)
    } else {
        None
    };
    Some(Arc::new(AiCleanUp::new(ai.clone(), key)))
}

/// Whether a start needs the on-device locales: only an engine left to the
/// machine is chosen by them.
fn needs_locales(d: &DictationConfig) -> bool {
    d.engine != Some(DictationEngine::Api)
}

async fn start_session(
    app: AppHandle,
    on_event: Channel<DictationEvent>,
) -> Result<DictationSession, DictationError> {
    let config = read_config(&app, "commands::dictation::dictation_start");
    let (d, ai) = (config.dictation, config.ai);
    check_enabled(&d)?;

    let dictation = app.state::<DictationState>();
    let reservation = dictation.reserve()?;
    let session_id = reservation.id();
    let controls = reservation.controls();

    let locales = if needs_locales(&d) {
        bridge::supported_locales().await
    } else {
        Ok(Vec::new())
    };
    let engine = d.effective_engine(!supported(&locales).is_empty());
    let silence_check = cfg!(target_os = "macos");
    let out = Emitter::logging_failures(session_id, move |event| on_event.send(event));
    let task_app = app.clone();
    let release_slot = move || task_app.state::<DictationState>().release(session_id);
    let watch_app = app.clone();

    let locale = match engine {
        DictationEngine::Api => {
            let prepared =
                prepare_transcription(&d, &ai, !supported(&locales).is_empty(), |account| {
                    key_for(&app, account)
                })?;
            let client = build_client_for(prepared.is_private_network()).map_err(|error| {
                tracing::error!(error = %error, "the transcription client could not be built");
                DictationError::Network {
                    host: prepared.host.clone(),
                }
            })?;
            let clean_up = clean_up_for(&app, &d, &ai);
            ensure_mic().await?;
            let (recorder, frames) = open_microphone().await?;
            let session = ApiSession {
                session_id,
                frames,
                capture: Box::new(recorder),
                transcriber: Arc::new(ApiTranscriber::new(client, prepared)),
                clean_up,
                segmenter: Segmenter::new(SegmenterParams::default()),
                silence_check,
                limits: SessionLimits::default(),
            };
            tauri::async_runtime::spawn(async move {
                let _slot = ReleaseOnDrop::new(release_slot);
                let _watch = SightWatch::start(&watch_app, session_id);
                session::run_api(session, controls, out).await;
            });
            None
        }
        DictationEngine::OnDevice => {
            let locale = on_device_start_locale(&d, &locales, &bridge::system_locale())?;
            let clean_up = clean_up_for(&app, &d, &ai);
            ensure_mic().await?;
            let (recorder, frames) = open_microphone().await?;
            let (recognizer, events) = bridge::start(&locale);
            let session = OnDeviceSession {
                session_id,
                frames,
                capture: Box::new(recorder),
                recognizer: Box::new(recognizer),
                events,
                clean_up,
                locale: locale.clone(),
                silence_check,
                limits: SessionLimits::default(),
            };
            tauri::async_runtime::spawn(async move {
                let _slot = ReleaseOnDrop::new(release_slot);
                let _watch = SightWatch::start(&watch_app, session_id);
                session::run_on_device(session, controls, out).await;
            });
            Some(locale)
        }
    };
    reservation.keep();
    tracing::info!(session = session_id, engine = ?engine, "dictation started");
    Ok(DictationSession {
        session_id,
        engine,
        locale,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use jotra_core::dictation::{DictationErrorCode, DictationProvider};

    fn enabled() -> DictationConfig {
        DictationConfig {
            enabled: true,
            ..DictationConfig::default()
        }
    }

    fn ai(provider: &str) -> AiConfig {
        let mut config = AiConfig {
            provider: provider.to_string(),
            ..AiConfig::default()
        };
        config.chat.enabled = true;
        config
    }

    fn locales(ids: &[&str]) -> Result<Vec<String>, i32> {
        Ok(ids.iter().map(|id| id.to_string()).collect())
    }

    #[test]
    fn only_a_start_that_may_run_on_device_asks_for_its_locales() {
        let engine = |engine| DictationConfig {
            engine,
            ..enabled()
        };
        assert!(!needs_locales(&engine(Some(DictationEngine::Api))));
        assert!(needs_locales(&engine(Some(DictationEngine::OnDevice))));
        assert!(needs_locales(&engine(None)));
    }

    #[test]
    fn a_start_is_refused_while_dictation_is_off() {
        assert_eq!(
            check_enabled(&DictationConfig::default()),
            Err(DictationError::Disabled)
        );
        assert_eq!(check_enabled(&enabled()), Ok(()));
        assert_eq!(
            DictationError::Disabled.failure().code,
            DictationErrorCode::Disabled
        );
    }

    #[test]
    fn the_key_is_changed_here_only_when_it_is_dictations_own() {
        let d = DictationConfig {
            provider: DictationProvider::OpenAi,
            ..enabled()
        };
        assert_eq!(
            own_key_account(&d, &ai("openai")),
            Err(DictationError::KeyShared)
        );
        assert_eq!(
            own_key_account(&d, &ai("groq")).as_deref(),
            Ok("dictation.openai")
        );
        let mut hidden = ai("openai");
        hidden.chat.enabled = false;
        assert_eq!(
            own_key_account(&d, &hidden).as_deref(),
            Ok("dictation.openai")
        );
    }

    #[test]
    fn a_custom_key_is_kept_for_its_address_only() {
        let mut d = DictationConfig {
            provider: DictationProvider::Custom,
            base_url: "https://speech.example.com/v1".to_string(),
            ..enabled()
        };
        let first = own_key_account(&d, &ai("openai")).expect("own account");
        assert_eq!(first, "dictation.custom.https://speech.example.com");

        d.base_url = "https://other.example.com/v1".to_string();
        let second = own_key_account(&d, &ai("openai")).expect("own account");
        assert_ne!(first, second, "a new server does not get the old key");

        d.base_url = "not a url".to_string();
        assert_eq!(
            own_key_account(&d, &ai("openai")),
            Err(DictationError::EndpointNotAllowed)
        );
    }

    #[test]
    fn a_shared_key_is_neither_set_nor_cleared_here() {
        let store = AiState::with_store(Box::<jotra_ai::keys::MemoryKeyStore>::default());
        store.set_key("openai", "ai-key").expect("stores");
        let d = DictationConfig {
            provider: DictationProvider::OpenAi,
            ..enabled()
        };
        let shared = own_key_account(&d, &ai("openai"));
        assert!(shared.is_err());
        assert_eq!(store.key("openai").as_deref(), Some("ai-key"));

        let own = own_key_account(&d, &ai("anthropic")).expect("own account");
        store.set_key(&own, "dictation-key").expect("stores");
        assert_eq!(store.key("openai").as_deref(), Some("ai-key"));
        assert_eq!(
            store.key("dictation.openai").as_deref(),
            Some("dictation-key")
        );
    }

    #[test]
    fn consent_records_the_audio_host_once_and_only_for_a_hosted_endpoint() {
        let mut d = DictationConfig {
            provider: DictationProvider::Groq,
            ..enabled()
        };
        assert_eq!(apply_audio_consent(&mut d), Ok(true));
        assert_eq!(apply_audio_consent(&mut d), Ok(false));
        assert_eq!(d.consented_hosts, ["api.groq.com"]);

        let mut local = DictationConfig {
            provider: DictationProvider::Custom,
            base_url: "http://localhost:8000/v1".to_string(),
            ..enabled()
        };
        assert_eq!(
            apply_audio_consent(&mut local),
            Err(LOCAL_ENDPOINT.to_string())
        );
        assert!(local.consented_hosts.is_empty());

        let mut plain = DictationConfig {
            provider: DictationProvider::Custom,
            base_url: "http://speech.example.com/v1".to_string(),
            ..enabled()
        };
        assert_eq!(
            apply_audio_consent(&mut plain),
            Err(DictationError::EndpointNotAllowed.to_string())
        );
        assert!(plain.consented_hosts.is_empty());
    }

    #[test]
    fn capabilities_follow_what_the_bridge_answered() {
        let answered = locales(&["en-US", "en-GB", "ar-SA"]);
        let caps = capabilities_from(&enabled(), &answered, "ar_SA", true);
        assert_eq!(
            caps,
            DictationCapabilities {
                on_device: true,
                on_device_languages: vec!["ar".to_string(), "en".to_string()],
                system_language: "ar".to_string(),
                mic_settings: true,
                effective_engine: DictationEngine::OnDevice,
            }
        );

        let unsupported = capabilities_from(&enabled(), &Err(1), "", false);
        assert!(!unsupported.on_device);
        assert!(unsupported.on_device_languages.is_empty());
        assert_eq!(unsupported.system_language, "");
        assert_eq!(unsupported.effective_engine, DictationEngine::Api);

        let empty = capabilities_from(&enabled(), &locales(&[]), "en_US", true);
        assert!(!empty.on_device);
        assert_eq!(empty.effective_engine, DictationEngine::Api);

        let picked = DictationConfig {
            engine: Some(DictationEngine::Api),
            ..enabled()
        };
        assert_eq!(
            capabilities_from(&picked, &answered, "en_US", true).effective_engine,
            DictationEngine::Api
        );
    }

    #[test]
    fn capabilities_serialize_as_the_settings_page_reads_them() {
        let caps = capabilities_from(&enabled(), &locales(&["en-US"]), "en_US", false);
        let value = serde_json::to_value(caps).expect("serializes");
        assert_eq!(value["effective_engine"], "on_device");
        assert_eq!(value["on_device_languages"][0], "en");
        assert_eq!(value["mic_settings"], false);
    }

    #[test]
    fn the_on_device_locale_follows_the_language_then_the_system() {
        let answered = locales(&["en-US", "en-GB", "ar-SA"]);
        assert_eq!(
            on_device_start_locale(&enabled(), &answered, "en_GB").as_deref(),
            Ok("en-GB")
        );
        let arabic = DictationConfig {
            language: "ar".to_string(),
            ..enabled()
        };
        assert_eq!(
            on_device_start_locale(&arabic, &answered, "en_GB").as_deref(),
            Ok("ar-SA")
        );
    }

    #[test]
    fn an_on_device_start_names_what_is_missing() {
        let answered = locales(&["en-US"]);
        let german = DictationConfig {
            language: "de".to_string(),
            ..enabled()
        };
        assert_eq!(
            on_device_start_locale(&german, &answered, "en_US"),
            Err(DictationError::LanguageUnsupported {
                language: "German".to_string()
            })
        );
        assert_eq!(
            on_device_start_locale(&enabled(), &answered, "fr_FR"),
            Err(DictationError::LanguageUnsupported {
                language: "French".to_string()
            })
        );
        assert_eq!(
            on_device_start_locale(&enabled(), &Err(1), "en_US"),
            Err(DictationError::UnsupportedOs)
        );
        assert_eq!(
            on_device_start_locale(&enabled(), &locales(&[]), "en_US"),
            Err(DictationError::UnsupportedOs)
        );
        assert!(matches!(
            on_device_start_locale(&enabled(), &answered, ""),
            Err(DictationError::LanguageUnsupported { .. })
        ));
    }

    #[test]
    fn the_language_list_comes_from_the_core_table() {
        let languages = dictation_languages();
        assert_eq!(languages.len(), LANGUAGES.len());
        assert!(languages
            .iter()
            .any(|l| l.code == "ar" && l.name == "Arabic"));
    }

    #[test]
    fn mic_settings_open_only_where_the_system_has_a_page() {
        assert_eq!(mic_settings_uri().is_some(), !cfg!(target_os = "linux"));
    }

    #[test]
    fn every_command_here_is_in_the_invoke_handler() {
        const LIB_RS: &str = include_str!("../lib.rs");
        for command in [
            "commands::dictation::dictation_capabilities",
            "commands::dictation::dictation_languages",
            "commands::dictation::dictation_endpoint_state",
            "commands::dictation::dictation_consent_host",
            "commands::dictation::dictation_set_key",
            "commands::dictation::dictation_clear_key",
            "commands::dictation::dictation_start",
            "commands::dictation::dictation_stop",
            "commands::dictation::dictation_cancel",
            "commands::dictation::dictation_open_mic_settings",
        ] {
            assert!(LIB_RS.contains(command), "{command} is not registered");
        }
        assert!(
            LIB_RS.contains("dictation::DictationState::default()"),
            "the session slot is not managed"
        );
    }
}
