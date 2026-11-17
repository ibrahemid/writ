use cpal::{Error, ErrorKind};
use writ_audio::{is_fatal_stream_error, CaptureError};
use writ_core::dictation::DictationError;

#[test]
fn opening_failures_map_to_capture_errors() {
    assert_eq!(
        CaptureError::from(Error::new(ErrorKind::PermissionDenied)),
        CaptureError::PermissionDenied
    );
    assert_eq!(
        CaptureError::from(Error::new(ErrorKind::DeviceNotAvailable)),
        CaptureError::NoInputDevice
    );
    assert_eq!(
        CaptureError::from(Error::with_message(
            ErrorKind::UnsupportedConfig,
            "24 channels"
        )),
        CaptureError::Unsupported("24 channels".to_string())
    );
    assert_eq!(
        CaptureError::from(Error::with_message(ErrorKind::BackendError, "boom")),
        CaptureError::Backend("boom".to_string())
    );
}

#[test]
fn windows_access_denied_text_is_a_permission_failure() {
    for text in [
        "Access is denied. (0x80070005)",
        "IAudioClient::Initialize failed: 0x80070005",
        "E_ACCESSDENIED",
    ] {
        assert_eq!(
            CaptureError::from(Error::with_message(ErrorKind::BackendError, text)),
            CaptureError::PermissionDenied,
            "{text}"
        );
        assert_eq!(
            CaptureError::from_stream(&Error::with_message(ErrorKind::Other, text)),
            CaptureError::PermissionDenied,
            "{text}"
        );
    }
}

#[test]
fn a_device_that_goes_away_while_recording_is_lost() {
    for kind in [ErrorKind::DeviceNotAvailable, ErrorKind::StreamInvalidated] {
        assert!(matches!(
            CaptureError::from_stream(&Error::with_message(kind, "unplugged")),
            CaptureError::DeviceLost(detail) if detail == "unplugged"
        ));
    }
    assert!(matches!(
        CaptureError::from_stream(&Error::new(ErrorKind::BackendError)),
        CaptureError::Backend(_)
    ));
}

#[test]
fn route_changes_and_xruns_keep_the_stream_running() {
    assert!(!is_fatal_stream_error(ErrorKind::DeviceChanged));
    assert!(!is_fatal_stream_error(ErrorKind::Xrun));
    assert!(!is_fatal_stream_error(ErrorKind::RealtimeDenied));
    assert!(is_fatal_stream_error(ErrorKind::DeviceNotAvailable));
    assert!(is_fatal_stream_error(ErrorKind::StreamInvalidated));
    assert!(is_fatal_stream_error(ErrorKind::BackendError));
}

#[test]
fn capture_errors_become_dictation_errors() {
    assert_eq!(
        DictationError::from(CaptureError::NoInputDevice),
        DictationError::NoInputDevice
    );
    assert_eq!(
        DictationError::from(CaptureError::PermissionDenied),
        DictationError::MicDenied
    );
    for error in [
        CaptureError::DeviceLost("gone".to_string()),
        CaptureError::Unsupported("gone".to_string()),
        CaptureError::Backend("gone".to_string()),
    ] {
        assert_eq!(
            DictationError::from(error),
            DictationError::CaptureFailed("gone".to_string())
        );
    }
}
