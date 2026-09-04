use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager, State};

use crate::commands::file::{open_generated_document, FileOpenResult};
use crate::state::AppState;

const NOTICES_FILE: &str = "THIRD-PARTY-NOTICES.md";

/// Title of the generated document the licence listing opens as. Also the
/// buffer title, so it is what the tab and the sidebar show.
pub const THIRD_PARTY_NOTICES_TITLE: &str = "Third-party licences";

/// First readable candidate, in the order given.
///
/// The bundled resource is the authoritative copy; a debug build has no
/// resource directory of its own, so the caller appends the repo-root file as a
/// fallback.
fn pick_notices_path(candidates: &[PathBuf]) -> Option<&Path> {
    candidates
        .iter()
        .map(PathBuf::as_path)
        .find(|path| path.is_file())
}

/// The notices file at the repo root, given the crate's manifest directory.
///
/// `src-tauri` is one level below the root, so the root is the manifest
/// directory's parent.
fn repo_root_notices(manifest_dir: &Path) -> Option<PathBuf> {
    manifest_dir.parent().map(|root| root.join(NOTICES_FILE))
}

fn notices_candidates(app: &AppHandle) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Ok(dir) = app.path().resource_dir() {
        candidates.push(dir.join(NOTICES_FILE));
    }
    // Debug builds run from the workspace, where the file is only at the repo
    // root. `CARGO_MANIFEST_DIR` is the build machine's path, so it is never
    // compiled into a release binary.
    #[cfg(debug_assertions)]
    {
        candidates.extend(repo_root_notices(Path::new(env!("CARGO_MANIFEST_DIR"))));
    }
    candidates
}

/// The third-party licence notices bundled with this build.
fn bundled_third_party_notices(app: &AppHandle) -> Result<String, String> {
    let candidates = notices_candidates(app);
    let path = pick_notices_path(&candidates).ok_or_else(|| {
        tracing::warn!(?candidates, "third-party notices not found");
        "Third-party licences are missing from this build.".to_string()
    })?;
    std::fs::read_to_string(path).map_err(|e| {
        tracing::warn!(error = %e, path = %path.display(), "reading third-party notices failed");
        "Could not read the third-party licences.".to_string()
    })
}

/// IPC: opens the bundled third-party licence notices as a read-only buffer.
///
/// The text is read fresh from the bundled file on every call, so an already
/// open tab is refreshed rather than left showing what an earlier build
/// wrote. [`open_generated_document`] is what keeps the listing out of the
/// notes folder and out of the search index (ADR-028 §1).
#[tauri::command]
pub fn open_third_party_notices(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<FileOpenResult, String> {
    let content = bundled_third_party_notices(&app)?;
    open_generated_document(&state, THIRD_PARTY_NOTICES_TITLE, &content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_copy_wins_over_the_repo_root_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let resource = dir.path().join("resource").join(NOTICES_FILE);
        let fallback = dir.path().join("repo").join(NOTICES_FILE);
        std::fs::create_dir_all(resource.parent().unwrap()).unwrap();
        std::fs::create_dir_all(fallback.parent().unwrap()).unwrap();
        std::fs::write(&resource, "bundled").unwrap();
        std::fs::write(&fallback, "repo").unwrap();

        let candidates = [resource.clone(), fallback];
        let picked = pick_notices_path(&candidates).unwrap();
        assert_eq!(picked, resource);
    }

    #[test]
    fn falls_back_to_the_repo_root_when_the_resource_is_absent() {
        let dir = tempfile::tempdir().unwrap();
        let resource = dir.path().join("resource").join(NOTICES_FILE);
        let fallback = dir.path().join("repo").join(NOTICES_FILE);
        std::fs::create_dir_all(fallback.parent().unwrap()).unwrap();
        std::fs::write(&fallback, "repo").unwrap();

        let candidates = [resource, fallback.clone()];
        let picked = pick_notices_path(&candidates).unwrap();
        assert_eq!(picked, fallback);
    }

    #[test]
    fn no_candidate_exists() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join(NOTICES_FILE);
        assert!(pick_notices_path(&[missing]).is_none());
        assert!(pick_notices_path(&[]).is_none());
    }

    #[test]
    fn the_repo_root_fallback_is_the_manifest_parent() {
        assert_eq!(
            repo_root_notices(Path::new("/build/writ/src-tauri")),
            Some(PathBuf::from("/build/writ").join(NOTICES_FILE))
        );
    }

    #[test]
    fn a_manifest_dir_with_no_parent_has_no_fallback() {
        assert_eq!(repo_root_notices(Path::new("/")), None);
        assert_eq!(repo_root_notices(Path::new("")), None);
    }

    #[test]
    fn a_directory_is_not_a_candidate() {
        let dir = tempfile::tempdir().unwrap();
        let as_dir = dir.path().join(NOTICES_FILE);
        std::fs::create_dir(&as_dir).unwrap();
        assert!(pick_notices_path(&[as_dir]).is_none());
    }
}
