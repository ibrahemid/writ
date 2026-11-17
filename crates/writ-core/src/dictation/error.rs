//! Everything that can end a dictation session, and what the person is told.

use std::fmt;

use serde::Serialize;

use super::platform::Platform;
use crate::polish::PolishError;

/// Why a dictation session could not start or stopped early.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DictationError {
    /// The Dictation switch is off.
    Disabled,
    /// A session is already running.
    Busy,
    /// The system refused the app the microphone.
    MicDenied,
    /// The microphone delivered only zeros, which is what a refused
    /// microphone sends on systems that do not report the refusal.
    Silence,
    /// The system has no input device.
    NoInputDevice,
    /// The input stream failed after it started.
    CaptureFailed(String),
    /// The base URL failed the outbound guard or did not parse.
    EndpointNotAllowed,
    /// No transcription model is configured.
    ModelRequired,
    /// The person has not allowed audio to go to `host`.
    ConsentRequired {
        /// The endpoint host.
        host: String,
    },
    /// `host` needs a key and none is stored.
    KeyRequired {
        /// The endpoint host.
        host: String,
    },
    /// The key is the AI connection's and only its own page changes it.
    KeyShared,
    /// `host` answered 401 or 403.
    Unauthorized {
        /// The endpoint host.
        host: String,
    },
    /// `host` answered 404 for the model.
    ModelNotFound {
        /// The endpoint host.
        host: String,
        /// The configured model.
        model: String,
    },
    /// `host` answered 429 after the retry.
    RateLimited {
        /// The endpoint host.
        host: String,
    },
    /// `host` answered 5xx after the retry, another unexpected status, or a
    /// 2xx whose body is not a transcript.
    Server {
        /// The endpoint host.
        host: String,
        /// The HTTP status.
        status: u16,
    },
    /// `host` answered 400 or 422.
    Refused {
        /// The endpoint host.
        host: String,
        /// The server's reason, already cut to one short line.
        detail: String,
    },
    /// `host` could not be reached, or the request timed out.
    Network {
        /// The endpoint host.
        host: String,
    },
    /// On-device dictation needs macOS 26 or later.
    UnsupportedOs,
    /// The on-device engine has no model for `language`.
    LanguageUnsupported {
        /// The language's English name, or its code when unlisted.
        language: String,
    },
    /// The on-device model for `language` failed to download.
    AssetsFailed {
        /// The language's English name, or its locale.
        language: String,
    },
    /// The on-device engine stopped with a bridge code.
    EngineFailed {
        /// The bridge's error code.
        code: i32,
    },
    /// The whole session produced no text.
    NoSpeech,
}

/// The stable code the frontend switches on, one per [`DictationError`]
/// variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DictationErrorCode {
    /// [`DictationError::Disabled`].
    Disabled,
    /// [`DictationError::Busy`].
    Busy,
    /// [`DictationError::MicDenied`].
    MicDenied,
    /// [`DictationError::Silence`].
    Silence,
    /// [`DictationError::NoInputDevice`].
    NoInputDevice,
    /// [`DictationError::CaptureFailed`].
    CaptureFailed,
    /// [`DictationError::EndpointNotAllowed`].
    EndpointNotAllowed,
    /// [`DictationError::ModelRequired`].
    ModelRequired,
    /// [`DictationError::ConsentRequired`].
    ConsentRequired,
    /// [`DictationError::KeyRequired`].
    KeyRequired,
    /// [`DictationError::KeyShared`].
    KeyShared,
    /// [`DictationError::Unauthorized`].
    Unauthorized,
    /// [`DictationError::ModelNotFound`].
    ModelNotFound,
    /// [`DictationError::RateLimited`].
    RateLimited,
    /// [`DictationError::Server`].
    Server,
    /// [`DictationError::Refused`].
    Refused,
    /// [`DictationError::Network`].
    Network,
    /// [`DictationError::UnsupportedOs`].
    UnsupportedOs,
    /// [`DictationError::LanguageUnsupported`].
    LanguageUnsupported,
    /// [`DictationError::AssetsFailed`].
    AssetsFailed,
    /// [`DictationError::EngineFailed`].
    EngineFailed,
    /// [`DictationError::NoSpeech`].
    NoSpeech,
}

/// The button a failure toast carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DictationAction {
    /// Open the system's microphone permission page.
    OpenMicSettings,
    /// Open Settings > Apps > Dictation.
    OpenDictationSettings,
}

/// A failure as the frontend receives it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DictationFailure {
    /// What went wrong.
    pub code: DictationErrorCode,
    /// The sentence the toast shows.
    pub message: String,
    /// The toast's button, when one helps.
    pub action: Option<DictationAction>,
}

impl DictationError {
    /// The variant's code.
    pub fn code(&self) -> DictationErrorCode {
        match self {
            Self::Disabled => DictationErrorCode::Disabled,
            Self::Busy => DictationErrorCode::Busy,
            Self::MicDenied => DictationErrorCode::MicDenied,
            Self::Silence => DictationErrorCode::Silence,
            Self::NoInputDevice => DictationErrorCode::NoInputDevice,
            Self::CaptureFailed(_) => DictationErrorCode::CaptureFailed,
            Self::EndpointNotAllowed => DictationErrorCode::EndpointNotAllowed,
            Self::ModelRequired => DictationErrorCode::ModelRequired,
            Self::ConsentRequired { .. } => DictationErrorCode::ConsentRequired,
            Self::KeyRequired { .. } => DictationErrorCode::KeyRequired,
            Self::KeyShared => DictationErrorCode::KeyShared,
            Self::Unauthorized { .. } => DictationErrorCode::Unauthorized,
            Self::ModelNotFound { .. } => DictationErrorCode::ModelNotFound,
            Self::RateLimited { .. } => DictationErrorCode::RateLimited,
            Self::Server { .. } => DictationErrorCode::Server,
            Self::Refused { .. } => DictationErrorCode::Refused,
            Self::Network { .. } => DictationErrorCode::Network,
            Self::UnsupportedOs => DictationErrorCode::UnsupportedOs,
            Self::LanguageUnsupported { .. } => DictationErrorCode::LanguageUnsupported,
            Self::AssetsFailed { .. } => DictationErrorCode::AssetsFailed,
            Self::EngineFailed { .. } => DictationErrorCode::EngineFailed,
            Self::NoSpeech => DictationErrorCode::NoSpeech,
        }
    }

    /// The sentence shown for this error on `platform`.
    pub fn message_on(&self, platform: Platform) -> String {
        match self {
            Self::Disabled => "Dictation is turned off.".to_string(),
            Self::Busy => "Dictation is already running.".to_string(),
            Self::MicDenied => match platform {
                Platform::Mac => "Writ can't use the microphone. Allow it in System Settings > Privacy & Security > Microphone.".to_string(),
                Platform::Windows => "Writ can't use the microphone. Turn on microphone access for desktop apps in Settings > Privacy & security > Microphone.".to_string(),
                Platform::Linux => "Writ can't use the microphone. Check the input device in your sound settings.".to_string(),
            },
            Self::Silence => match platform {
                Platform::Mac => "The microphone sent only silence. Check that Writ is allowed in System Settings > Privacy & Security > Microphone.".to_string(),
                Platform::Windows => "The microphone sent only silence. Check that microphone access for desktop apps is on in Settings > Privacy & security > Microphone.".to_string(),
                Platform::Linux => "The microphone sent only silence. Check the input device in your sound settings.".to_string(),
            },
            Self::NoInputDevice => "No microphone found.".to_string(),
            Self::CaptureFailed(detail) => format!("The microphone stopped: {}.", detail.trim_end_matches('.')),
            Self::EndpointNotAllowed => PolishError::EndpointNotAllowed.to_string(),
            Self::ModelRequired => "Enter a model in Settings > Apps > Dictation.".to_string(),
            Self::ConsentRequired { host } => {
                format!("Allow sending audio to {host} in Settings > Apps > Dictation.")
            }
            Self::KeyRequired { host } => {
                format!("Add an API key for {host} in Settings > Apps > Dictation.")
            }
            Self::KeyShared => "This key belongs to the AI connection. Change it on the Chat or Rewrite page.".to_string(),
            Self::Unauthorized { host } => format!("{host} rejected the API key."),
            Self::ModelNotFound { host, model } => format!("{host} has no model named {model}."),
            Self::RateLimited { host } => {
                format!("{host} is limiting requests. Try again in a minute.")
            }
            Self::Server { host, status } if (200..=299).contains(status) => {
                format!("{host} sent a reply Writ could not read.")
            }
            Self::Server { host, status } => format!("{host} returned an error ({status})."),
            Self::Refused { host, detail } => format!("{host} rejected the request: {detail}"),
            Self::Network { host } => format!("Could not reach {host}."),
            Self::UnsupportedOs => "On-device dictation needs macOS 26 or later.".to_string(),
            Self::LanguageUnsupported { language } => format!(
                "On-device dictation doesn't support {language}. Choose another language or the API engine."
            ),
            Self::AssetsFailed { language } => {
                format!("Could not download the {language} speech model.")
            }
            Self::EngineFailed { code } => format!("On-device dictation stopped ({code})."),
            Self::NoSpeech => "No speech heard.".to_string(),
        }
    }

    /// The toast button for this error on `platform`.
    pub fn action_on(&self, platform: Platform) -> Option<DictationAction> {
        match self {
            Self::MicDenied | Self::Silence => platform
                .mic_settings_uri()
                .map(|_| DictationAction::OpenMicSettings),
            Self::KeyRequired { .. }
            | Self::ConsentRequired { .. }
            | Self::Unauthorized { .. }
            | Self::ModelNotFound { .. }
            | Self::EndpointNotAllowed
            | Self::ModelRequired
            | Self::UnsupportedOs
            | Self::LanguageUnsupported { .. } => Some(DictationAction::OpenDictationSettings),
            _ => None,
        }
    }

    /// The failure as the frontend receives it on `platform`.
    pub fn failure_on(&self, platform: Platform) -> DictationFailure {
        DictationFailure {
            code: self.code(),
            message: self.message_on(platform),
            action: self.action_on(platform),
        }
    }

    /// The failure as the frontend receives it on this system.
    pub fn failure(&self) -> DictationFailure {
        self.failure_on(Platform::current())
    }

    /// Whether one retry may succeed: the network, rate limits and 5xx.
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Network { .. } | Self::RateLimited { .. } => true,
            Self::Server { status, .. } => (500..=599).contains(status),
            _ => false,
        }
    }

    /// The endpoint host the error names, when it names one.
    pub fn host(&self) -> Option<&str> {
        match self {
            Self::ConsentRequired { host }
            | Self::KeyRequired { host }
            | Self::Unauthorized { host }
            | Self::ModelNotFound { host, .. }
            | Self::RateLimited { host }
            | Self::Server { host, .. }
            | Self::Refused { host, .. }
            | Self::Network { host } => Some(host),
            _ => None,
        }
    }
}

impl fmt::Display for DictationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message_on(Platform::current()))
    }
}

impl std::error::Error for DictationError {}

impl From<DictationError> for DictationFailure {
    fn from(error: DictationError) -> Self {
        error.failure()
    }
}
