//! What can go wrong while capturing, and how the dictation session reads it.

use cpal::{Error as CpalError, ErrorKind};
use writ_core::dictation::DictationError;

/// A capture failure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CaptureError {
    /// The system has no input device.
    #[error("no input device")]
    NoInputDevice,
    /// The system refused the microphone.
    #[error("microphone access denied")]
    PermissionDenied,
    /// The device went away while recording.
    #[error("{0}")]
    DeviceLost(String),
    /// The device offers no format this crate can read.
    #[error("{0}")]
    Unsupported(String),
    /// Any other audio backend failure.
    #[error("{0}")]
    Backend(String),
}

/// Text Windows puts in an access-denied failure (`E_ACCESSDENIED`), which
/// the WASAPI backend can report as an unclassified error.
const ACCESS_DENIED_MARKERS: &[&str] = &["access is denied", "0x80070005", "e_accessdenied"];

fn says_access_denied(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    ACCESS_DENIED_MARKERS.iter().any(|m| text.contains(m))
}

impl From<CpalError> for CaptureError {
    /// A failure while opening the device or starting the stream.
    fn from(error: CpalError) -> Self {
        let text = error.to_string();
        match error.kind() {
            ErrorKind::PermissionDenied => Self::PermissionDenied,
            ErrorKind::DeviceNotAvailable => Self::NoInputDevice,
            ErrorKind::UnsupportedConfig | ErrorKind::UnsupportedOperation => {
                Self::Unsupported(text)
            }
            _ if says_access_denied(&text) => Self::PermissionDenied,
            _ => Self::Backend(text),
        }
    }
}

impl CaptureError {
    /// A failure the running stream reported.
    pub fn from_stream(error: &CpalError) -> Self {
        let text = error.to_string();
        match error.kind() {
            ErrorKind::PermissionDenied => Self::PermissionDenied,
            ErrorKind::DeviceNotAvailable | ErrorKind::StreamInvalidated => Self::DeviceLost(text),
            _ if says_access_denied(&text) => Self::PermissionDenied,
            _ => Self::Backend(text),
        }
    }
}

/// Whether a stream error ends the recording. A route change, an xrun or a
/// refused real-time priority leaves the stream running.
pub fn is_fatal_stream_error(kind: ErrorKind) -> bool {
    !matches!(
        kind,
        ErrorKind::DeviceChanged | ErrorKind::Xrun | ErrorKind::RealtimeDenied
    )
}

impl From<CaptureError> for DictationError {
    fn from(error: CaptureError) -> Self {
        match error {
            CaptureError::NoInputDevice => DictationError::NoInputDevice,
            CaptureError::PermissionDenied => DictationError::MicDenied,
            CaptureError::DeviceLost(detail)
            | CaptureError::Unsupported(detail)
            | CaptureError::Backend(detail) => DictationError::CaptureFailed(detail),
        }
    }
}
