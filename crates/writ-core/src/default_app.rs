/// Platform file-type default-app policy — pure, no I/O, no platform dependencies.
///
/// The claimable-type table and comparison logic live here so they can be
/// unit-tested on every platform without any macOS SDK linkage.
use serde::{Deserialize, Serialize};

/// A group of file types Writ can offer to become the default opener for.
///
/// HTML is deliberately absent: Writ declares it can open `.html` (so it appears
/// in "Open With" and handles explicit opens) but must never register itself as
/// the default handler for `public.html`, which is what put it in the OS web-link
/// path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ClaimableType {
    /// Stable id used across the IPC boundary and the settings UI.
    pub id: &'static str,
    /// Human label for the settings row.
    pub label: &'static str,
    /// Representative extensions, for display (e.g. "Default for .rs, .go, …").
    pub exts: &'static [&'static str],
    /// Concrete UTIs whose default handler this group sets.
    ///
    /// LaunchServices default handlers are per-concrete-UTI and do NOT cascade
    /// through conformance, so a group enumerates every UTI it owns rather than
    /// relying on an ancestor like `public.source-code`. The exported `com.writ.*`
    /// identifiers cover dev extensions macOS has no system UTI for.
    pub utis: &'static [&'static str],
}

/// The set of type groups the in-app "Make default" control exposes.
///
/// Mirrors the `contentTypes` / `exportedType` declarations in `tauri.conf.json`:
/// a UTI here must be declared there, or Launch Services accepts the set call but
/// Finder never routes opens to Writ.
pub fn claimable_types() -> &'static [ClaimableType] {
    const TYPES: &[ClaimableType] = &[
        ClaimableType {
            id: "plain-text",
            label: "Plain text & logs",
            exts: &["txt", "text", "log"],
            utis: &["public.plain-text", "com.apple.log"],
        },
        ClaimableType {
            id: "markdown",
            label: "Markdown",
            exts: &["md", "markdown"],
            utis: &["net.daringfireball.markdown"],
        },
        ClaimableType {
            id: "config-data",
            label: "Config & data",
            exts: &[
                "json", "yaml", "yml", "toml", "cfg", "ini", "conf", "env", "csv", "xml",
            ],
            utis: &[
                "public.json",
                "public.yaml",
                "public.toml",
                "com.microsoft.ini",
                "com.writ.config-text",
                "public.comma-separated-values-text",
                "public.xml",
            ],
        },
        ClaimableType {
            id: "source-code",
            label: "Source code",
            exts: &[
                "rs", "go", "jsx", "ts", "tsx", "py", "sh", "js", "c", "cpp", "h", "swift",
            ],
            // Concrete UTIs only. `public.source-code` is intentionally absent:
            // default handlers don't cascade through it, so claiming it would
            // never route a real file yet would block this group from ever
            // reading as "default" (every listed UTI must be ours to qualify).
            utis: &[
                "com.writ.rust-source",
                "com.writ.go-source",
                "com.writ.jsx-source",
                "com.writ.typescript-source",
                "public.python-script",
                "public.shell-script",
                "com.netscape.javascript-source",
                "public.c-source",
                "public.c-plus-plus-source",
                "public.c-header",
                "public.swift-source",
            ],
        },
    ];
    TYPES
}

/// Look up a claimable type group by its stable id.
pub fn claimable_type(id: &str) -> Option<&'static ClaimableType> {
    claimable_types().iter().find(|t| t.id == id)
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

/// Combine the per-UTI statuses of a claimable group into one row status.
///
/// A group is [`DefaultAppStatus::IsDefault`] only when Writ owns every UTI in it.
/// If the platform cannot answer for any UTI the whole group is
/// [`DefaultAppStatus::Unsupported`]; otherwise it surfaces the first UTI Writ does
/// not own so the row shows what would be displaced.
pub fn aggregate_status(statuses: &[DefaultAppStatus]) -> DefaultAppStatus {
    if statuses.is_empty() || statuses.iter().any(DefaultAppStatus::is_unsupported) {
        return DefaultAppStatus::Unsupported;
    }
    if statuses
        .iter()
        .all(|s| matches!(s, DefaultAppStatus::IsDefault))
    {
        return DefaultAppStatus::IsDefault;
    }
    statuses
        .iter()
        .find(|s| !matches!(s, DefaultAppStatus::IsDefault))
        .cloned()
        .unwrap_or(DefaultAppStatus::NoHandler)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claimable_types_are_non_empty_with_unique_ids() {
        let types = claimable_types();
        assert!(!types.is_empty());
        let mut ids: Vec<&str> = types.iter().map(|t| t.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), types.len(), "claimable type ids must be unique");
        for t in types {
            assert!(!t.exts.is_empty(), "{} has no exts", t.id);
            assert!(!t.utis.is_empty(), "{} has no utis", t.id);
        }
    }

    #[test]
    fn html_is_never_claimable() {
        for t in claimable_types() {
            assert!(!t.exts.contains(&"html"), "{} claims html", t.id);
            assert!(!t.exts.contains(&"htm"), "{} claims htm", t.id);
            assert!(
                !t.utis.contains(&"public.html"),
                "{} claims public.html",
                t.id
            );
        }
    }

    #[test]
    fn no_group_claims_non_routing_ancestors() {
        // public.text is the broad parent of plain-text/rtf/source; claiming it
        // would make Writ the fallback for nearly all text content. public.source-code
        // never routes a concrete file (defaults don't cascade) and, as a member of
        // an all-must-match group, would only keep the row from ever reading default.
        for t in claimable_types() {
            assert!(
                !t.utis.contains(&"public.text"),
                "{} claims public.text",
                t.id
            );
            assert!(
                !t.utis.contains(&"public.source-code"),
                "{} claims public.source-code",
                t.id
            );
        }
    }

    #[test]
    fn dev_extensions_without_system_utis_are_owned_via_exported_utis() {
        let src = claimable_type("source-code").unwrap();
        for ext in ["rs", "go", "jsx"] {
            assert!(src.exts.contains(&ext), "source-code missing {ext}");
        }
        for uti in [
            "com.writ.rust-source",
            "com.writ.go-source",
            "com.writ.jsx-source",
            "com.writ.typescript-source",
        ] {
            assert!(src.utis.contains(&uti), "source-code missing {uti}");
        }
        let cfg = claimable_type("config-data").unwrap();
        assert!(cfg.exts.contains(&"env") && cfg.exts.contains(&"conf"));
        assert!(cfg.utis.contains(&"com.writ.config-text"));
    }

    #[test]
    fn claimable_type_lookup() {
        assert_eq!(claimable_type("markdown").unwrap().id, "markdown");
        assert!(claimable_type("nope").is_none());
    }

    #[test]
    fn aggregate_all_default_is_default() {
        let s = [DefaultAppStatus::IsDefault, DefaultAppStatus::IsDefault];
        assert_eq!(aggregate_status(&s), DefaultAppStatus::IsDefault);
    }

    #[test]
    fn aggregate_partial_is_not_default() {
        let s = [
            DefaultAppStatus::IsDefault,
            DefaultAppStatus::OtherApp { name: None },
        ];
        assert!(matches!(
            aggregate_status(&s),
            DefaultAppStatus::OtherApp { .. }
        ));

        let s = [DefaultAppStatus::IsDefault, DefaultAppStatus::NoHandler];
        assert_eq!(aggregate_status(&s), DefaultAppStatus::NoHandler);
    }

    #[test]
    fn aggregate_any_unsupported_is_unsupported() {
        let s = [DefaultAppStatus::IsDefault, DefaultAppStatus::Unsupported];
        assert_eq!(aggregate_status(&s), DefaultAppStatus::Unsupported);
        assert_eq!(aggregate_status(&[]), DefaultAppStatus::Unsupported);
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
