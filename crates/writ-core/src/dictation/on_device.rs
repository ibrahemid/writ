//! The on-device engine's error codes.

use super::error::DictationError;
use super::language;

/// The bridge reports macOS below 26 or a build without the speech module.
pub const BRIDGE_UNSUPPORTED: i32 = 1;
/// The bridge has no locale for the language.
pub const BRIDGE_LANGUAGE_UNSUPPORTED: i32 = 2;
/// The speech model failed to download.
pub const BRIDGE_ASSETS_FAILED: i32 = 3;
/// The system refused the microphone.
pub const BRIDGE_MIC_DENIED: i32 = 5;

/// The error for a bridge failure `code` while dictating in `language` (an
/// ISO-639-1 code or a locale).
pub fn error_from_bridge(code: i32, language: &str) -> DictationError {
    match code {
        BRIDGE_UNSUPPORTED => DictationError::UnsupportedOs,
        BRIDGE_LANGUAGE_UNSUPPORTED => DictationError::LanguageUnsupported {
            language: language_name(language),
        },
        BRIDGE_ASSETS_FAILED => DictationError::AssetsFailed {
            language: language_name(language),
        },
        BRIDGE_MIC_DENIED => DictationError::MicDenied,
        other => DictationError::EngineFailed { code: other },
    }
}

/// The English name for a code or locale, or the input when unlisted.
pub fn language_name(code_or_locale: &str) -> String {
    let code = language::locale_language(code_or_locale);
    let code = if code == "nb" { "no".to_string() } else { code };
    language::language(&code)
        .map(|l| l.name.to_string())
        .unwrap_or_else(|| code_or_locale.to_string())
}
