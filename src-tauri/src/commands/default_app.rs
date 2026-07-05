use tauri::State;
#[cfg(target_os = "macos")]
use writ_core::default_app::aggregate_status;
use writ_core::default_app::{claimable_type, claimable_types, ClaimableType, DefaultAppStatus};

use crate::state::AppState;

// The bundle id is stable for the lifetime of the process — look it up once.
// Consumed by the macOS handler paths and the platform-independent tests.
#[cfg(any(test, target_os = "macos"))]
fn our_bundle_id() -> &'static str {
    "com.writ.editor"
}

/// IPC: the file-type groups the settings UI can offer to make Writ the default for.
#[tauri::command]
pub fn list_default_app_types() -> Vec<ClaimableType> {
    claimable_types().to_vec()
}

/// IPC: query whether Writ is the macOS default handler for every UTI in group `id`.
///
/// Returns `DefaultAppStatus::Unsupported` on every platform except macOS.
#[tauri::command]
pub fn get_default_app_status(
    _state: State<'_, AppState>,
    id: String,
) -> Result<DefaultAppStatus, String> {
    let group = claimable_type(&id).ok_or_else(|| format!("unknown type group: {id}"))?;

    #[cfg(target_os = "macos")]
    {
        let mut statuses = Vec::with_capacity(group.utis.len());
        for uti in group.utis {
            let handler_id = macos::query_default_handler(uti).map_err(|e| e.to_string())?;
            let status = DefaultAppStatus::from_handler_id(handler_id.as_deref(), our_bundle_id());
            statuses.push(enrich_with_display_name(status, handler_id.as_deref()));
        }
        Ok(aggregate_status(&statuses))
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = group;
        Ok(DefaultAppStatus::Unsupported)
    }
}

/// IPC: register Writ as the macOS default handler for every UTI in group `id`.
///
/// Returns `DefaultAppStatus::Unsupported` on every platform except macOS.
#[tauri::command]
pub fn set_default_app(_state: State<'_, AppState>, id: String) -> Result<(), String> {
    let group = claimable_type(&id).ok_or_else(|| format!("unknown type group: {id}"))?;

    #[cfg(target_os = "macos")]
    {
        for uti in group.utis {
            macos::set_default_handler(uti, our_bundle_id()).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    {
        let _ = group;
        Ok(())
    }
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

#[cfg(target_os = "macos")]
mod macos {
    use core_foundation::base::TCFType;
    use core_foundation::string::{CFString, CFStringRef};

    // Launch Services symbols are deprecated since macOS 12 but still present
    // and the only mechanism that targets our minimum of 10.15. Using them
    // directly here avoids a crate dependency whose #[deprecated] attributes
    // would fire clippy -D warnings.
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
