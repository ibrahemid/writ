//! Where each system keeps its microphone permission.

/// System Settings > Privacy & Security > Microphone on macOS.
pub const MAC_MIC_SETTINGS_URI: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone";

/// Settings > Privacy & security > Microphone on Windows.
pub const WINDOWS_MIC_SETTINGS_URI: &str = "ms-settings:privacy-microphone";

/// The system family whose microphone permission a message points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// macOS.
    Mac,
    /// Windows.
    Windows,
    /// Linux and the other Unix systems, which gate the microphone through
    /// the sound server rather than a privacy setting.
    Linux,
}

impl Platform {
    /// The system this build runs on.
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Mac
        } else if cfg!(target_os = "windows") {
            Self::Windows
        } else {
            Self::Linux
        }
    }

    /// The microphone settings page, where the system has one.
    pub fn mic_settings_uri(self) -> Option<&'static str> {
        match self {
            Self::Mac => Some(MAC_MIC_SETTINGS_URI),
            Self::Windows => Some(WINDOWS_MIC_SETTINGS_URI),
            Self::Linux => None,
        }
    }
}

/// The microphone settings page for the running system, where it has one.
pub fn mic_settings_uri() -> Option<&'static str> {
    Platform::current().mic_settings_uri()
}
