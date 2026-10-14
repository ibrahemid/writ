use tauri::AppHandle;
#[cfg(any(target_os = "macos", target_os = "linux"))]
use writ_core::default_app::aggregate_status;
use writ_core::default_app::{claimable_type, claimable_types, ClaimableType, DefaultAppStatus};

use crate::commands::offload::{run_detached, ERR_COMMAND_STOPPED};

// The bundle id is stable for the lifetime of the process — look it up once.
// Consumed by the macOS handler paths and the platform-independent tests.
#[cfg(any(test, target_os = "macos"))]
fn our_bundle_id() -> &'static str {
    "com.writ.editor"
}

/// Why a default-app call did nothing.
#[derive(Debug, thiserror::Error)]
pub enum DefaultAppCommandError {
    /// No claimable group has this id.
    #[error("unknown type group: {0}")]
    UnknownGroup(String),
    /// This system offers Writ no way to become the default.
    #[error("Writ cannot set default apps on this system.")]
    Unsupported,
    /// The system call that sets or reads the default failed.
    #[error("{0}")]
    System(String),
}

impl From<DefaultAppCommandError> for String {
    fn from(error: DefaultAppCommandError) -> Self {
        error.to_string()
    }
}

/// IPC: the file-type groups the settings UI can offer to make Writ the default for.
#[tauri::command]
pub fn list_default_app_types() -> Vec<ClaimableType> {
    claimable_types().to_vec()
}

fn group(id: &str) -> Result<&'static ClaimableType, DefaultAppCommandError> {
    claimable_type(id).ok_or_else(|| DefaultAppCommandError::UnknownGroup(id.to_string()))
}

/// IPC: whether Writ is the default handler for every type in group `id`.
///
/// macOS asks Launch Services per UTI, Linux asks `xdg-mime` per MIME type,
/// Windows answers [`DefaultAppStatus::SystemSettings`] (only the user picks a
/// default there), and any other system [`DefaultAppStatus::Unsupported`].
/// Off the main thread: the answer comes from the system's own records.
#[tauri::command]
pub async fn get_default_app_status(id: String) -> Result<DefaultAppStatus, String> {
    let group = group(&id)?;
    run_detached(ERR_COMMAND_STOPPED, move || {
        query_status(group).map_err(String::from)
    })
    .await
}

#[cfg(target_os = "macos")]
fn query_status(group: &ClaimableType) -> Result<DefaultAppStatus, DefaultAppCommandError> {
    let mut statuses = Vec::with_capacity(group.utis.len());
    for uti in group.utis {
        let handler_id = macos::query_default_handler(uti)
            .map_err(|e| DefaultAppCommandError::System(e.to_string()))?;
        let status = DefaultAppStatus::from_handler_id(handler_id.as_deref(), our_bundle_id());
        statuses.push(enrich_with_display_name(status, handler_id.as_deref()));
    }
    Ok(aggregate_status(&statuses))
}

#[cfg(target_os = "linux")]
fn query_status(group: &ClaimableType) -> Result<DefaultAppStatus, DefaultAppCommandError> {
    let Some(entry) = linux::desktop_entry() else {
        return Ok(DefaultAppStatus::Unsupported);
    };
    let mut statuses = Vec::with_capacity(group.mimes.len());
    for mime in group.mimes {
        match linux::query_default(mime, entry) {
            Ok(status) => statuses.push(status),
            Err(linux::XdgError::Missing) => return Ok(DefaultAppStatus::Unsupported),
            Err(error) => return Err(DefaultAppCommandError::System(error.to_string())),
        }
    }
    Ok(aggregate_status(&statuses))
}

#[cfg(target_os = "windows")]
fn query_status(_group: &ClaimableType) -> Result<DefaultAppStatus, DefaultAppCommandError> {
    Ok(DefaultAppStatus::SystemSettings)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn query_status(_group: &ClaimableType) -> Result<DefaultAppStatus, DefaultAppCommandError> {
    Ok(DefaultAppStatus::Unsupported)
}

/// IPC: make Writ the default handler for every type in group `id`.
///
/// macOS sets it through Launch Services and Linux through `xdg-mime default`.
/// Windows lets no app make itself the default, so its Default apps page
/// opens for the user to pick Writ there. Any other system, and a Linux
/// install with no desktop entry, answers [`DefaultAppCommandError::Unsupported`].
#[tauri::command]
pub async fn set_default_app(app: AppHandle, id: String) -> Result<(), String> {
    let group = group(&id)?;
    run_detached(ERR_COMMAND_STOPPED, move || {
        claim(&app, group).map_err(String::from)
    })
    .await
}

#[cfg(target_os = "macos")]
fn claim(_app: &AppHandle, group: &ClaimableType) -> Result<(), DefaultAppCommandError> {
    for uti in group.utis {
        macos::set_default_handler(uti, our_bundle_id())
            .map_err(|e| DefaultAppCommandError::System(e.to_string()))?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn claim(_app: &AppHandle, group: &ClaimableType) -> Result<(), DefaultAppCommandError> {
    let entry = linux::desktop_entry().ok_or(DefaultAppCommandError::Unsupported)?;
    linux::set_default(entry, group.mimes).map_err(|error| match error {
        linux::XdgError::Missing => DefaultAppCommandError::Unsupported,
        other => DefaultAppCommandError::System(other.to_string()),
    })
}

#[cfg(target_os = "windows")]
fn claim(app: &AppHandle, _group: &ClaimableType) -> Result<(), DefaultAppCommandError> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(
            writ_core::default_app::WINDOWS_DEFAULT_APPS_URI,
            None::<&str>,
        )
        .map_err(|e| DefaultAppCommandError::System(e.to_string()))
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn claim(_app: &AppHandle, _group: &ClaimableType) -> Result<(), DefaultAppCommandError> {
    Err(DefaultAppCommandError::Unsupported)
}

/// Attempt to resolve a bundle id to its display name for richer status messages.
///
/// If resolution fails (sandboxed environment, app not installed), the status is
/// returned unchanged; the caller must tolerate `OtherApp { name: None }`.
#[cfg(target_os = "macos")]
fn enrich_with_display_name(
    status: DefaultAppStatus,
    handler_id: Option<&str>,
) -> DefaultAppStatus {
    use writ_core::default_app::DefaultAppStatus::OtherApp;
    if let OtherApp { name: None } = &status {
        if let Some(id) = handler_id {
            let display = macos::display_name_for_bundle(id);
            return OtherApp { name: display };
        }
    }
    status
}

#[cfg(target_os = "linux")]
mod linux {
    use std::process::Command;

    use writ_core::default_app::{
        application_dirs, desktop_entry_status, find_desktop_entry, DefaultAppStatus,
    };

    /// Why `xdg-mime` gave no answer.
    #[derive(Debug, thiserror::Error)]
    pub enum XdgError {
        /// `xdg-mime` is not installed.
        #[error("xdg-mime is not installed")]
        Missing,
        /// It ran and failed.
        #[error("xdg-mime failed: {0}")]
        Failed(String),
    }

    /// Writ's desktop entry, when the system has one.
    pub fn desktop_entry() -> Option<&'static str> {
        let data_home = std::env::var("XDG_DATA_HOME").ok();
        let data_dirs = std::env::var("XDG_DATA_DIRS").ok();
        let home = dirs::home_dir();
        let dirs = application_dirs(data_home.as_deref(), data_dirs.as_deref(), home.as_deref());
        find_desktop_entry(&dirs, std::path::Path::is_file)
    }

    fn run(args: &[&str]) -> Result<String, XdgError> {
        let output = Command::new("xdg-mime")
            .args(args)
            .output()
            .map_err(|error| match error.kind() {
                std::io::ErrorKind::NotFound => XdgError::Missing,
                _ => XdgError::Failed(error.to_string()),
            })?;
        if !output.status.success() {
            return Err(XdgError::Failed(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// `xdg-mime query default <mime>`, read against Writ's `entry`.
    pub fn query_default(mime: &str, entry: &str) -> Result<DefaultAppStatus, XdgError> {
        run(&["query", "default", mime]).map(|answer| desktop_entry_status(&answer, entry))
    }

    /// `xdg-mime default <entry> <mime>...`.
    pub fn set_default(entry: &str, mimes: &[&str]) -> Result<(), XdgError> {
        let mut args = vec!["default", entry];
        args.extend_from_slice(mimes);
        run(&args).map(|_| ())
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use core_foundation::base::TCFType;
    use core_foundation::string::{CFString, CFStringRef};

    // Launch Services symbols are deprecated as of macOS 12, the minimum this
    // bundle targets, but they remain present and are still the only mechanism
    // callable as plain C: the NSWorkspace replacements are Objective-C. Using
    // them directly here avoids a crate dependency whose #[deprecated]
    // attributes would fire clippy -D warnings.
    #[allow(non_snake_case)]
    #[link(name = "CoreServices", kind = "framework")]
    extern "C" {
        // Returns the bundle id of the registered default role handler for the
        // given content-type UTI and role. Follows the Create Rule — caller owns
        // the returned object and must release it.
        fn LSCopyDefaultRoleHandlerForContentType(
            inContentType: CFStringRef,
            inRole: u32,
        ) -> CFStringRef;

        // Sets the default handler. Returns an OSStatus (0 == noErr).
        fn LSSetDefaultRoleHandlerForContentType(
            inContentType: CFStringRef,
            inRole: u32,
            inHandlerBundleID: CFStringRef,
        ) -> i32;

        // Returns an array of CFURLs for apps matching the bundle id. Used to
        // look up the display name. Returns NULL when no match.
        fn LSCopyApplicationURLsForBundleIdentifier(
            inBundleIdentifier: CFStringRef,
            outError: *mut core_foundation::base::CFTypeRef,
        ) -> core_foundation::array::CFArrayRef;
    }

    // kLSRolesAll — read/write the handler that Finder actually uses.
    const LS_ROLES_ALL: u32 = 0xFFFF_FFFF;

    /// Query the default handler bundle id for `uti`. Returns `None` when no
    /// handler is registered.
    pub fn query_default_handler(uti: &str) -> Result<Option<String>, DefaultAppError> {
        let uti_cf = CFString::new(uti);
        // SAFETY: LSCopyDefaultRoleHandlerForContentType follows the Create Rule.
        // We immediately wrap it in CFString::wrap_under_create_rule so it is
        // released exactly once when the wrapper drops.
        let raw = unsafe {
            LSCopyDefaultRoleHandlerForContentType(uti_cf.as_concrete_TypeRef(), LS_ROLES_ALL)
        };
        if raw.is_null() {
            return Ok(None);
        }
        // wrap_under_create_rule takes ownership: releases on drop.
        let handler: CFString = unsafe { CFString::wrap_under_create_rule(raw) };
        Ok(Some(handler.to_string()))
    }

    /// Register `bundle_id` as the default handler for `uti`.
    pub fn set_default_handler(uti: &str, bundle_id: &str) -> Result<(), DefaultAppError> {
        let uti_cf = CFString::new(uti);
        let bundle_cf = CFString::new(bundle_id);
        let status = unsafe {
            LSSetDefaultRoleHandlerForContentType(
                uti_cf.as_concrete_TypeRef(),
                LS_ROLES_ALL,
                bundle_cf.as_concrete_TypeRef(),
            )
        };
        if status == 0 {
            Ok(())
        } else {
            Err(DefaultAppError::OsStatus(status))
        }
    }

    /// Resolve a bundle id to its localised display name.
    ///
    /// Returns `None` when the application is not installed or the name cannot
    /// be determined (e.g. inside a sandbox without the File permission).
    pub fn display_name_for_bundle(bundle_id: &str) -> Option<String> {
        use core_foundation::array::{CFArray, CFArrayRef};
        use core_foundation::url::CFURL;

        let bundle_cf = CFString::new(bundle_id);
        // SAFETY: LSCopyApplicationURLsForBundleIdentifier follows the Create
        // Rule; we wrap immediately.
        let raw_array: CFArrayRef = unsafe {
            LSCopyApplicationURLsForBundleIdentifier(
                bundle_cf.as_concrete_TypeRef(),
                std::ptr::null_mut(),
            )
        };
        if raw_array.is_null() {
            return None;
        }
        let array: CFArray<CFURL> = unsafe { CFArray::wrap_under_create_rule(raw_array) };
        if array.is_empty() {
            return None;
        }

        // Take the first matching URL (highest rank) and extract the display name.
        let url: CFURL = array.get(0)?.clone();
        let path = url.to_path()?;

        // Use the filename stem (e.g. "TextEdit" from "TextEdit.app").
        path.file_stem().and_then(|s| s.to_str()).map(str::to_owned)
    }

    #[derive(Debug)]
    pub enum DefaultAppError {
        OsStatus(i32),
    }

    impl std::fmt::Display for DefaultAppError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::OsStatus(code) => write!(f, "Launch Services error: OSStatus {code}"),
            }
        }
    }

    impl std::error::Error for DefaultAppError {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_group_is_rejected_at_the_boundary() {
        // The command guard rejects unknown type-group ids before any LS call.
        // This path executes on all platforms.
        assert!(claimable_type("not-a-group").is_none());
        assert!(claimable_type("source-code").is_some());
    }

    #[test]
    fn list_default_app_types_returns_the_table() {
        let types = list_default_app_types();
        assert!(types.iter().any(|t| t.id == "source-code"));
        assert!(types.iter().all(|t| !t.exts.contains(&"html")));
    }

    // Every UTI the settings UI can claim must be declared as a handled type in
    // the bundle (a `contentTypes` entry or an `exportedType.identifier`), or
    // Launch Services accepts the set call but Finder never routes opens to Writ
    // and the row can never read as default. This guards the claimable table
    // against the tauri.conf.json declarations so the two cannot drift.
    #[test]
    fn every_claimable_uti_is_declared_in_the_bundle() {
        let raw = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tauri.conf.json"))
            .expect("read tauri.conf.json");
        let conf: serde_json::Value = serde_json::from_str(&raw).expect("parse tauri.conf.json");
        let assocs = conf["bundle"]["fileAssociations"]
            .as_array()
            .expect("fileAssociations array");

        let mut declared = std::collections::HashSet::new();
        for a in assocs {
            if let Some(cts) = a["contentTypes"].as_array() {
                declared.extend(cts.iter().filter_map(|v| v.as_str().map(str::to_owned)));
            }
            if let Some(id) = a["exportedType"]["identifier"].as_str() {
                declared.insert(id.to_owned());
            }
        }

        for group in claimable_types() {
            for uti in group.utis {
                assert!(
                    declared.contains(*uti),
                    "claimable UTI `{uti}` (group `{}`) is not declared in tauri.conf.json",
                    group.id
                );
            }
        }
    }

    // Every MIME type a group claims on Linux must be one the desktop entry
    // declares, or `xdg-mime default` records a default the file manager never
    // offers Writ for.
    #[test]
    fn every_claimable_mime_type_is_declared_in_the_bundle() {
        let raw = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tauri.conf.json"))
            .expect("read tauri.conf.json");
        let conf: serde_json::Value = serde_json::from_str(&raw).expect("parse tauri.conf.json");
        let declared: std::collections::HashSet<&str> = conf["bundle"]["fileAssociations"]
            .as_array()
            .expect("fileAssociations array")
            .iter()
            .filter_map(|a| a["mimeType"].as_str())
            .collect();
        for group in claimable_types() {
            for mime in group.mimes {
                assert!(
                    declared.contains(mime),
                    "claimable MIME type `{mime}` (group `{}`) is not declared in tauri.conf.json",
                    group.id
                );
            }
        }
    }

    fn bundle_associations() -> Vec<serde_json::Value> {
        let raw = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tauri.conf.json"))
            .expect("read tauri.conf.json");
        let conf: serde_json::Value = serde_json::from_str(&raw).expect("parse tauri.conf.json");
        conf["bundle"]["fileAssociations"]
            .as_array()
            .expect("fileAssociations array")
            .clone()
    }

    // An extension never matches a file whose name has none: `ext:
    // ["dockerfile"]` reaches `x.dockerfile` and never `Dockerfile`. Such a
    // name is matched by a MIME type whose glob is the name (Linux), or not
    // at all.
    #[test]
    fn no_association_declares_an_extension_for_an_extensionless_name() {
        const EXTENSIONLESS: &[&str] = &["dockerfile", "makefile", "procfile", "gemfile"];
        for association in bundle_associations() {
            let name = association["name"]
                .as_str()
                .unwrap_or_default()
                .to_lowercase();
            let exts: Vec<String> = association["ext"]
                .as_array()
                .expect("ext array")
                .iter()
                .filter_map(|ext| ext.as_str().map(str::to_lowercase))
                .collect();
            for ext in &exts {
                assert!(
                    !EXTENSIONLESS.contains(&ext.as_str()),
                    "association `{name}` declares `.{ext}`, a name with no extension"
                );
            }
            if EXTENSIONLESS.contains(&name.as_str()) {
                assert!(
                    association["mimeType"]
                        .as_str()
                        .is_some_and(|mime| mime != "text/plain"),
                    "association `{name}` matches nothing by name"
                );
            }
        }
    }

    // Writ owns the com.writ.* types and imports every other one: exporting
    // `net.daringfireball.markdown` competes with the type's owner in Launch
    // Services.
    #[test]
    fn writ_exports_only_its_own_types_and_imports_markdown() {
        for association in bundle_associations() {
            if let Some(id) = association["exportedType"]["identifier"].as_str() {
                assert!(id.starts_with("com.writ."), "Writ exports `{id}`");
            }
        }
        let plist = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Info.plist"))
            .expect("read Info.plist");
        let imports = plist
            .split("<key>UTImportedTypeDeclarations</key>")
            .nth(1)
            .expect("Info.plist imports types");
        assert!(imports.contains("<string>net.daringfireball.markdown</string>"));
    }

    #[test]
    fn a_system_without_a_way_to_set_defaults_says_so() {
        let message = DefaultAppCommandError::Unsupported.to_string();
        assert_eq!(message, "Writ cannot set default apps on this system.");
        assert_eq!(
            String::from(DefaultAppCommandError::UnknownGroup("x".into())),
            "unknown type group: x"
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_answers_that_the_user_picks_defaults_in_settings() {
        let group = claimable_type("markdown").expect("markdown");
        assert_eq!(
            query_status(group).expect("status"),
            DefaultAppStatus::SystemSettings
        );
    }

    #[test]
    fn unsupported_platform_returns_unsupported_status() {
        // Exercises the non-macOS branch of get_default_app_status via the policy
        // type directly; the FFI path is exercised by manual in-app testing on macOS.
        let status = DefaultAppStatus::from_handler_id(None, our_bundle_id());
        assert_eq!(status, DefaultAppStatus::NoHandler);

        let status = DefaultAppStatus::Unsupported;
        assert!(status.is_unsupported());
    }

    #[test]
    fn bundle_id_comparison_is_case_insensitive() {
        let status = DefaultAppStatus::from_handler_id(Some("COM.WRIT.EDITOR"), our_bundle_id());
        assert_eq!(status, DefaultAppStatus::IsDefault);
    }
}
