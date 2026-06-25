/// Platform file-type default-app policy — pure, no I/O, no platform dependencies.
///
/// The UTI constants and comparison logic live here so they can be unit-tested on
/// every platform without any macOS SDK linkage.
use serde::{Deserialize, Serialize};

/// Well-known file extensions Writ exposes "Make default" for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KnownExt {
    /// Markdown (`.md`, `.markdown`).
    Md,
    /// HTML (`.html`, `.htm`).
    Html,
}

/// Maps a known extension to its canonical UTI.
///
/// Both `md` and `markdown` surface as `net.daringfireball.markdown`.
/// `html`/`htm` resolve to `public.html`.
pub fn ext_to_uti(ext: &str) -> Option<(&'static str, KnownExt)> {
    match ext.to_ascii_lowercase().trim_start_matches('.') {
        "md" | "markdown" => Some(("net.daringfireball.markdown", KnownExt::Md)),
        "html" | "htm" => Some(("public.html", KnownExt::Html)),
        _ => None,
    }
}

/// What to do with a URL delivered by the OS open event (macOS `RunEvent::Opened`).
///
/// Writ registers as an *editor* for local source files, so the OS hands it
/// `file://` URLs to open. But when Writ is the chosen handler for a content type
/// such as `public.html`, the OS can also route non-file links here. Those must be
/// handed to the real default app for their scheme (browser for `http(s)`, mail
/// client for `mailto`, …) instead of being dropped — otherwise the user sees Writ
/// take focus with nothing opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenDisposition {
    /// A `file://` URL: resolve to a path and open it in the editor.
    OpenFile,
    /// A non-file URL (http, https, mailto, …): hand off to the OS default app.
    HandoffToDefaultApp,
    /// Empty scheme or Writ's own internal scheme: never open, never forward.
    Ignore,
}

/// Classifies an OS-delivered open URL by its scheme. See [`OpenDisposition`].
///
/// Comparison is case-insensitive. Writ's internal `writ-preview` scheme is never
/// handed off; an empty scheme is ignored defensively.
pub fn classify_open_scheme(scheme: &str) -> OpenDisposition {
    match scheme.to_ascii_lowercase().as_str() {
        "file" => OpenDisposition::OpenFile,
        "" | "writ-preview" => OpenDisposition::Ignore,
        _ => OpenDisposition::HandoffToDefaultApp,
    }
}

/// Whether Writ is the registered default handler for a given UTI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum DefaultAppStatus {
    /// Writ is the default handler.
    IsDefault,
    /// Another application is the default handler.
    OtherApp {
        /// Display name of the current default, if resolvable.
        name: Option<String>,
    },
    /// No handler is registered for this UTI.
    NoHandler,
    /// The current platform does not support this query.
    Unsupported,
}

impl DefaultAppStatus {
    /// Interpret a raw bundle-id string returned by the OS.
    ///
    /// The comparison is case-insensitive; macOS Launch Services normalises
    /// bundle ids to lowercase internally.
    pub fn from_handler_id(handler_id: Option<&str>, our_bundle_id: &str) -> Self {
        match handler_id {
            None => Self::NoHandler,
            Some(id) if id.eq_ignore_ascii_case(our_bundle_id) => Self::IsDefault,
            Some(_) => Self::OtherApp { name: None },
        }
    }

    /// Returns `true` when the result indicates the platform cannot answer.
    pub fn is_unsupported(&self) -> bool {
        matches!(self, Self::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_open_scheme_opens_file_urls() {
        assert_eq!(classify_open_scheme("file"), OpenDisposition::OpenFile);
        assert_eq!(classify_open_scheme("FILE"), OpenDisposition::OpenFile);
    }

    #[test]
    fn classify_open_scheme_hands_non_file_urls_to_default_app() {
        for scheme in ["http", "https", "HTTPS", "mailto", "tel", "ftp"] {
            assert_eq!(
                classify_open_scheme(scheme),
                OpenDisposition::HandoffToDefaultApp,
                "scheme={scheme}"
            );
        }
    }

    #[test]
    fn classify_open_scheme_ignores_empty_and_internal_schemes() {
        assert_eq!(classify_open_scheme(""), OpenDisposition::Ignore);
        assert_eq!(
            classify_open_scheme("writ-preview"),
            OpenDisposition::Ignore
        );
        assert_eq!(
            classify_open_scheme("WRIT-PREVIEW"),
            OpenDisposition::Ignore
        );
    }

    #[test]
    fn ext_to_uti_maps_md() {
        let (uti, kind) = ext_to_uti("md").unwrap();
        assert_eq!(uti, "net.daringfireball.markdown");
        assert_eq!(kind, KnownExt::Md);
    }

    #[test]
    fn ext_to_uti_maps_markdown_alias() {
        let (uti, kind) = ext_to_uti("markdown").unwrap();
        assert_eq!(uti, "net.daringfireball.markdown");
        assert_eq!(kind, KnownExt::Md);
    }

    #[test]
    fn ext_to_uti_maps_html() {
        let (uti, kind) = ext_to_uti("html").unwrap();
        assert_eq!(uti, "public.html");
        assert_eq!(kind, KnownExt::Html);
    }

    #[test]
    fn ext_to_uti_maps_htm_alias() {
        let (uti, kind) = ext_to_uti("htm").unwrap();
        assert_eq!(uti, "public.html");
        assert_eq!(kind, KnownExt::Html);
    }

    #[test]
    fn ext_to_uti_strips_leading_dot() {
        assert!(ext_to_uti(".md").is_some());
        assert!(ext_to_uti(".html").is_some());
    }

    #[test]
    fn ext_to_uti_is_case_insensitive() {
        assert!(ext_to_uti("MD").is_some());
        assert!(ext_to_uti("HTML").is_some());
    }

    #[test]
    fn ext_to_uti_returns_none_for_unknown() {
        assert!(ext_to_uti("txt").is_none());
        assert!(ext_to_uti("").is_none());
        assert!(ext_to_uti("rs").is_none());
    }

    #[test]
    fn from_handler_id_is_default_when_bundle_matches() {
        let status = DefaultAppStatus::from_handler_id(Some("com.writ.editor"), "com.writ.editor");
        assert_eq!(status, DefaultAppStatus::IsDefault);
    }

    #[test]
    fn from_handler_id_is_case_insensitive() {
        let status = DefaultAppStatus::from_handler_id(Some("COM.WRIT.EDITOR"), "com.writ.editor");
        assert_eq!(status, DefaultAppStatus::IsDefault);
    }

    #[test]
    fn from_handler_id_other_app_when_different_bundle() {
        let status =
            DefaultAppStatus::from_handler_id(Some("com.apple.TextEdit"), "com.writ.editor");
        assert!(matches!(status, DefaultAppStatus::OtherApp { .. }));
    }

    #[test]
    fn from_handler_id_no_handler_when_none() {
        let status = DefaultAppStatus::from_handler_id(None, "com.writ.editor");
        assert_eq!(status, DefaultAppStatus::NoHandler);
    }

    #[test]
    fn unsupported_variant_reports_correctly() {
        assert!(DefaultAppStatus::Unsupported.is_unsupported());
        assert!(!DefaultAppStatus::IsDefault.is_unsupported());
        assert!(!DefaultAppStatus::NoHandler.is_unsupported());
        assert!(!DefaultAppStatus::OtherApp { name: None }.is_unsupported());
    }
}
