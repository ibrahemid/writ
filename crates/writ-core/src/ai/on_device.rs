//! Apple's on-device model as a provider: when its row is offered, what the
//! system reports about it, and what each way it can fail says.
//!
//! The model is reached through the Foundation Models framework, which has no
//! network address and no C interface: the app shell holds a Swift bridge and
//! reports the bridge's answers here as numbers. Everything that decides what
//! those numbers mean lives in this module, so it is tested without the
//! framework, on any platform.

use serde::Serialize;

/// The provider id of the on-device row, in `config.toml` and the table.
pub const ON_DEVICE_PROVIDER: &str = "apple";

/// The first macOS release that ships the Foundation Models framework.
pub const ON_DEVICE_MIN_MACOS: u32 = 26;

/// Whether `provider` is the on-device row.
pub fn is_on_device(provider: &str) -> bool {
    provider == ON_DEVICE_PROVIDER
}

/// What the system says about the on-device model right now.
///
/// The bridge answers a number ([`OnDeviceAvailability::from_code`]): `0`
/// available, `1` not usable from this build or this OS, `2` the Mac cannot
/// run it, `3` Apple Intelligence is off, `4` the model is still downloading.
/// Any other number is a reason a later system added, read as unusable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OnDeviceAvailability {
    /// The model can answer now.
    Available,
    /// The framework is absent: an OS before macOS 26, another platform, or a
    /// build made with an SDK that predates it.
    Unsupported,
    /// The Mac cannot run the model.
    DeviceNotEligible,
    /// Apple Intelligence is turned off in System Settings.
    AppleIntelligenceOff,
    /// The model is still being downloaded.
    ModelNotReady,
}

impl OnDeviceAvailability {
    /// The availability a bridge code stands for.
    pub fn from_code(code: i32) -> Self {
        match code {
            0 => Self::Available,
            2 => Self::DeviceNotEligible,
            3 => Self::AppleIntelligenceOff,
            4 => Self::ModelNotReady,
            _ => Self::Unsupported,
        }
    }

    /// The word the connection status carries for it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Unsupported => "unsupported",
            Self::DeviceNotEligible => "device_not_eligible",
            Self::AppleIntelligenceOff => "apple_intelligence_off",
            Self::ModelNotReady => "model_not_ready",
        }
    }

    /// Why a request cannot run now, or `None` when it can.
    pub fn failure(self) -> Option<OnDeviceFailure> {
        match self {
            Self::Available => None,
            Self::Unsupported => Some(OnDeviceFailure::Unsupported),
            Self::DeviceNotEligible => Some(OnDeviceFailure::DeviceNotEligible),
            Self::AppleIntelligenceOff => Some(OnDeviceFailure::AppleIntelligenceOff),
            Self::ModelNotReady => Some(OnDeviceFailure::ModelNotReady),
        }
    }
}

/// Whether the on-device row is in the provider table this machine shows.
///
/// The row needs macOS 26 or later (`os_major` is the running system's major
/// version, `0` where there is none to read) and a Mac that can run the model.
/// Apple Intelligence being off, or the model still downloading, keeps the
/// row: both are a setting or a wait away, and the connection line says which.
pub fn offers_on_device(os_major: u32, availability: OnDeviceAvailability) -> bool {
    os_major >= ON_DEVICE_MIN_MACOS
        && !matches!(
            availability,
            OnDeviceAvailability::Unsupported | OnDeviceAvailability::DeviceNotEligible
        )
}

/// Why an on-device rewrite did not finish. Every variant is the sentence a
/// person reads; the bridge's own error text never reaches one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum OnDeviceFailure {
    /// The framework is absent here.
    #[error("Apple's on-device model needs macOS 26 or later.")]
    Unsupported,
    /// The Mac cannot run the model.
    #[error("This Mac cannot run Apple's on-device model.")]
    DeviceNotEligible,
    /// Apple Intelligence is turned off.
    #[error("Turn on Apple Intelligence in System Settings to use the on-device model.")]
    AppleIntelligenceOff,
    /// The model, or an asset it needs, is not on the Mac yet.
    #[error("Apple's on-device model is still downloading. Try again later.")]
    ModelNotReady,
    /// The instructions and the text exceed the model's context window.
    #[error("The text is too long for the on-device model. Select a shorter passage.")]
    TooLong,
    /// The model's guardrails, or the model itself, declined the text.
    #[error("Apple's on-device model declined this text.")]
    Declined,
    /// The text is in a language the model does not support.
    #[error("The on-device model does not support this language.")]
    Language,
    /// The system is limiting requests, or another one is running.
    #[error("The on-device model is busy. Try again in a moment.")]
    Busy,
    /// Anything else the framework reported.
    #[error("The on-device model could not finish the rewrite.")]
    Failed,
}

impl OnDeviceFailure {
    /// The failure a bridge code stands for. Codes 1 to 4 are the
    /// availability codes; 10 to 16 are the framework's generation errors in
    /// the order the bridge numbers them.
    pub fn from_code(code: i32) -> Self {
        match code {
            1 => Self::Unsupported,
            2 => Self::DeviceNotEligible,
            3 => Self::AppleIntelligenceOff,
            4 | 11 => Self::ModelNotReady,
            10 => Self::TooLong,
            12 | 16 => Self::Declined,
            13 => Self::Language,
            14 | 15 => Self::Busy,
            _ => Self::Failed,
        }
    }

    /// A word for the log line, which carries no text of the request.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unsupported => "unsupported",
            Self::DeviceNotEligible => "device_not_eligible",
            Self::AppleIntelligenceOff => "apple_intelligence_off",
            Self::ModelNotReady => "model_not_ready",
            Self::TooLong => "too_long",
            Self::Declined => "declined",
            Self::Language => "language",
            Self::Busy => "busy",
            Self::Failed => "failed",
        }
    }
}

/// The part of `snapshot` that `sent` does not already hold.
///
/// The framework streams each reply as the whole text so far. The rewrite
/// surface takes pieces to append, so each snapshot is cut to what it adds.
/// `None` when the snapshot does not begin with what was already sent: a
/// piece already shown cannot be taken back.
pub fn snapshot_delta<'a>(sent: &str, snapshot: &'a str) -> Option<&'a str> {
    snapshot.strip_prefix(sent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_row_is_offered_from_macos_26_on_a_mac_that_can_run_the_model() {
        let available = OnDeviceAvailability::Available;
        assert!(!offers_on_device(15, available));
        assert!(!offers_on_device(0, available));
        assert!(offers_on_device(26, available));
        assert!(offers_on_device(27, available));
        // A setting or a wait away: the row stays and the connection line says
        // what is missing.
        assert!(offers_on_device(
            26,
            OnDeviceAvailability::AppleIntelligenceOff
        ));
        assert!(offers_on_device(26, OnDeviceAvailability::ModelNotReady));
        assert!(!offers_on_device(
            26,
            OnDeviceAvailability::DeviceNotEligible
        ));
        assert!(!offers_on_device(26, OnDeviceAvailability::Unsupported));
    }

    #[test]
    fn bridge_codes_read_as_their_availability() {
        assert_eq!(
            OnDeviceAvailability::from_code(0),
            OnDeviceAvailability::Available
        );
        assert_eq!(
            OnDeviceAvailability::from_code(1),
            OnDeviceAvailability::Unsupported
        );
        assert_eq!(
            OnDeviceAvailability::from_code(2),
            OnDeviceAvailability::DeviceNotEligible
        );
        assert_eq!(
            OnDeviceAvailability::from_code(3),
            OnDeviceAvailability::AppleIntelligenceOff
        );
        assert_eq!(
            OnDeviceAvailability::from_code(4),
            OnDeviceAvailability::ModelNotReady
        );
        // A reason a later system adds is not taken for a working model.
        assert_eq!(
            OnDeviceAvailability::from_code(5),
            OnDeviceAvailability::Unsupported
        );
        assert_eq!(
            OnDeviceAvailability::from_code(-1),
            OnDeviceAvailability::Unsupported
        );
    }

    #[test]
    fn availability_serialises_as_its_status_word() {
        for availability in [
            OnDeviceAvailability::Available,
            OnDeviceAvailability::Unsupported,
            OnDeviceAvailability::DeviceNotEligible,
            OnDeviceAvailability::AppleIntelligenceOff,
            OnDeviceAvailability::ModelNotReady,
        ] {
            assert_eq!(
                serde_json::to_string(&availability).unwrap(),
                format!("\"{}\"", availability.as_str())
            );
        }
        assert_eq!(OnDeviceAvailability::Available.failure(), None);
        assert_eq!(
            OnDeviceAvailability::AppleIntelligenceOff.failure(),
            Some(OnDeviceFailure::AppleIntelligenceOff)
        );
    }

    #[test]
    fn every_generation_error_reads_as_a_sentence() {
        let cases = [
            (10, OnDeviceFailure::TooLong),
            (11, OnDeviceFailure::ModelNotReady),
            (12, OnDeviceFailure::Declined),
            (13, OnDeviceFailure::Language),
            (14, OnDeviceFailure::Busy),
            (15, OnDeviceFailure::Busy),
            (16, OnDeviceFailure::Declined),
            (17, OnDeviceFailure::Failed),
            (20, OnDeviceFailure::Failed),
            (3, OnDeviceFailure::AppleIntelligenceOff),
        ];
        for (code, failure) in cases {
            assert_eq!(OnDeviceFailure::from_code(code), failure, "{code}");
            let sentence = failure.to_string();
            assert!(sentence.ends_with('.'), "{sentence}");
        }
        assert!(OnDeviceFailure::TooLong.to_string().contains("too long"));
    }

    #[test]
    fn a_snapshot_is_cut_to_what_it_adds() {
        assert_eq!(snapshot_delta("", "Hel"), Some("Hel"));
        assert_eq!(snapshot_delta("Hel", "Hello"), Some("lo"));
        assert_eq!(snapshot_delta("Hello", "Hello"), Some(""));
        // Multibyte text cuts at a character boundary.
        assert_eq!(snapshot_delta("مرح", "مرحبا"), Some("با"));
        assert_eq!(snapshot_delta("naïve ", "naïve café"), Some("café"));
        assert_eq!(snapshot_delta("🙂", "🙂👍"), Some("👍"));
        // A snapshot that rewrote what was already shown has nothing to add.
        assert_eq!(snapshot_delta("Hello", "Help"), None);
    }

    #[test]
    fn only_the_apple_row_is_on_device() {
        assert!(is_on_device("apple"));
        assert!(!is_on_device("ollama"));
        assert!(!is_on_device(""));
    }
}
