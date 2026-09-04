use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::poison::recover_poison;

#[derive(Default, Debug)]
struct Inner {
    pending_open: HashSet<String>,
    blessed_sources: HashSet<String>,
}

#[derive(Default, Debug)]
pub struct AuthorizedPaths {
    inner: Mutex<Inner>,
}

impl AuthorizedPaths {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_for_open(&self, canonical: String) {
        let mut guard = recover_poison(
            self.inner.lock(),
            "security::authorized_paths::record_for_open",
        );
        guard.pending_open.insert(canonical);
    }

    pub fn consume_for_open(&self, canonical: &str) -> bool {
        let mut guard = recover_poison(
            self.inner.lock(),
            "security::authorized_paths::consume_for_open",
        );
        guard.pending_open.remove(canonical)
    }

    /// Whether `canonical` has an open authorization waiting, without
    /// spending it. Used by the download gate: the token belongs to the open
    /// that follows the download, not to the download.
    pub fn is_pending_open(&self, canonical: &str) -> bool {
        let guard = recover_poison(
            self.inner.lock(),
            "security::authorized_paths::is_pending_open",
        );
        guard.pending_open.contains(canonical)
    }

    pub fn record_blessed_source(&self, canonical: String) {
        let mut guard = recover_poison(
            self.inner.lock(),
            "security::authorized_paths::record_blessed_source",
        );
        guard.blessed_sources.insert(canonical);
    }

    pub fn is_blessed_source(&self, canonical: &str) -> bool {
        let guard = recover_poison(
            self.inner.lock(),
            "security::authorized_paths::is_blessed_source",
        );
        guard.blessed_sources.contains(canonical)
    }

    pub fn pending_open_len(&self) -> usize {
        let guard = recover_poison(
            self.inner.lock(),
            "security::authorized_paths::pending_open_len",
        );
        guard.pending_open.len()
    }
}

/// `true` on the platforms whose default filesystem is case-preserving but
/// case-insensitive.
#[cfg(any(target_os = "macos", target_os = "windows"))]
const CASE_INSENSITIVE_FILESYSTEM: bool = true;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
const CASE_INSENSITIVE_FILESYSTEM: bool = false;

/// Compares two already-canonical path strings for authorization.
///
/// Case-insensitive on macOS and Windows, where APFS and NTFS are
/// case-preserving but case-insensitive by default, and byte-exact on Linux.
/// Without this a case-only rename of a file that is open — `notes.md` to
/// `Notes.md`, which APFS performs in place — makes the canonical round trip
/// disagree with the stored path and turns every later save into an
/// unauthorized write.
///
/// Unicode normalisation needs no handling here. Both sides come out of
/// [`canonicalize_for_authorization`], which returns the filesystem's own
/// normalisation of the name, so an NFC and an NFD spelling of the same file
/// arrive already agreeing.
pub fn paths_equal_for_authorization(a: &str, b: &str) -> bool {
    if CASE_INSENSITIVE_FILESYSTEM {
        a.to_lowercase() == b.to_lowercase()
    } else {
        a == b
    }
}

pub fn canonicalize_for_authorization(path: &Path) -> std::io::Result<String> {
    let canonical: PathBuf = std::fs::canonicalize(path)?;
    let stripped = strip_unc_prefix(canonical);
    stripped
        .into_os_string()
        .into_string()
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "non-utf8 path"))
}

/// Canonicalize a containment root (workspace or inbox folder) to the same
/// form `canonicalize_for_authorization` produces for candidate paths.
///
/// Both sides of the `starts_with` containment check must agree: on Windows,
/// `std::fs::canonicalize` prefixes `\\?\`, and a root stored with the prefix
/// never matches a candidate stripped of it.
pub fn canonicalize_root(path: &Path) -> std::io::Result<PathBuf> {
    Ok(strip_unc_prefix(std::fs::canonicalize(path)?))
}

/// Resolves `path` against the filesystem as far as it exists, then appends
/// the components that do not exist yet.
///
/// Two callers need the answer for a path that does not exist yet: the write
/// gate's containment check (a note being minted) and the data-folder guard,
/// which has to know where `WRIT_DATA_DIR` will land before anything creates
/// it. The watcher's ignore keys are built from this too
/// ([`crate::watcher::handler::ignore_key_path`]), so a file being created is
/// stamped under the path the watcher will deliver for it.
///
/// Walking up to the deepest existing ancestor is what makes the answer honest
/// for a file Writ is about to create: every symlink and every `..` above the
/// new name is resolved by `canonicalize`, and only names the filesystem has
/// never seen are appended literally.
///
/// Returns `None` for a relative path, for a path whose unresolved tail is
/// `..` or empty (`Path::file_name` yields nothing for either, so such a tail
/// can never be appended), and for any resolution error other than a missing
/// file.
pub fn resolve_for_containment(path: &Path) -> Option<String> {
    if !path.is_absolute() {
        return None;
    }

    let mut unresolved: Vec<std::ffi::OsString> = Vec::new();
    let mut cursor = path.to_path_buf();
    loop {
        match canonicalize_for_authorization(&cursor) {
            Ok(base) => {
                let mut resolved = PathBuf::from(base);
                for name in unresolved.iter().rev() {
                    resolved.push(name);
                }
                return resolved.into_os_string().into_string().ok();
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                unresolved.push(cursor.file_name()?.to_os_string());
                cursor = cursor.parent()?.to_path_buf();
            }
            Err(_) => return None,
        }
    }
}

#[cfg(windows)]
fn strip_unc_prefix(path: PathBuf) -> PathBuf {
    const UNC: &str = r"\\?\";
    match path.to_str() {
        Some(s) if s.starts_with(UNC) => PathBuf::from(&s[UNC.len()..]),
        _ => path,
    }
}

#[cfg(not(windows))]
fn strip_unc_prefix(path: PathBuf) -> PathBuf {
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_file(dir: &TempDir, name: &str) -> String {
        let p = dir.path().join(name);
        std::fs::write(&p, "x").unwrap();
        canonicalize_for_authorization(&p).unwrap()
    }

    #[test]
    fn record_then_consume_is_single_use() {
        let dir = TempDir::new().unwrap();
        let canonical = make_file(&dir, "single.txt");
        let auth = AuthorizedPaths::new();

        auth.record_for_open(canonical.clone());
        assert!(auth.consume_for_open(&canonical));
        assert!(!auth.consume_for_open(&canonical));
    }

    #[test]
    fn pending_open_reads_the_token_without_spending_it() {
        let dir = TempDir::new().unwrap();
        let canonical = make_file(&dir, "pending.txt");
        let auth = AuthorizedPaths::new();

        assert!(!auth.is_pending_open(&canonical));
        auth.record_for_open(canonical.clone());
        assert!(auth.is_pending_open(&canonical));
        assert!(auth.is_pending_open(&canonical));
        assert!(auth.consume_for_open(&canonical));
        assert!(!auth.is_pending_open(&canonical));
    }

    #[test]
    fn consume_unrecorded_path_is_false() {
        let auth = AuthorizedPaths::new();
        assert!(!auth.consume_for_open("/tmp/never-recorded-xyz"));
    }

    #[test]
    fn record_is_idempotent_but_still_single_use() {
        let dir = TempDir::new().unwrap();
        let canonical = make_file(&dir, "idempotent.txt");
        let auth = AuthorizedPaths::new();

        auth.record_for_open(canonical.clone());
        auth.record_for_open(canonical.clone());
        assert_eq!(auth.pending_open_len(), 1);
        assert!(auth.consume_for_open(&canonical));
        assert!(!auth.consume_for_open(&canonical));
    }

    #[test]
    fn blessed_source_is_session_lived_not_consumed() {
        let dir = TempDir::new().unwrap();
        let canonical = make_file(&dir, "bless.txt");
        let auth = AuthorizedPaths::new();

        auth.record_blessed_source(canonical.clone());
        assert!(auth.is_blessed_source(&canonical));
        assert!(auth.is_blessed_source(&canonical));
        assert!(!auth.is_blessed_source("/other/unblessed"));
    }

    #[test]
    fn canonicalize_resolves_relative_to_absolute() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("rel.txt");
        std::fs::write(&p, "y").unwrap();

        let canonical = canonicalize_for_authorization(&p).unwrap();
        assert!(std::path::Path::new(&canonical).is_absolute());
    }

    #[test]
    fn root_and_candidate_canonical_forms_agree_for_containment() {
        // A root canonicalized via canonicalize_root must be a starts_with
        // prefix of any contained candidate canonicalized via
        // canonicalize_for_authorization (on Windows both sides strip \\?\).
        let dir = TempDir::new().unwrap();
        let sub = dir.path().join("inbox");
        std::fs::create_dir(&sub).unwrap();
        let file = sub.join("note.md");
        std::fs::write(&file, "x").unwrap();

        let root = canonicalize_root(&sub).unwrap();
        let candidate = canonicalize_for_authorization(&file).unwrap();
        assert!(
            std::path::Path::new(&candidate).starts_with(&root),
            "candidate {candidate} must sit under root {}",
            root.display()
        );
    }

    #[test]
    fn paths_equal_for_authorization_matches_an_identical_path() {
        assert!(paths_equal_for_authorization(
            "/home/u/Writ/note.md",
            "/home/u/Writ/note.md"
        ));
        assert!(!paths_equal_for_authorization(
            "/home/u/Writ/note.md",
            "/home/u/Writ/other.md"
        ));
    }

    #[test]
    fn paths_equal_for_authorization_follows_the_platform_on_case() {
        let differs_only_by_case =
            paths_equal_for_authorization("/home/u/Writ/Note.md", "/home/u/Writ/note.md");
        assert_eq!(differs_only_by_case, CASE_INSENSITIVE_FILESYSTEM);
    }

    #[test]
    fn canonicalize_fails_on_missing_file() {
        let result = canonicalize_for_authorization(std::path::Path::new(
            "/this/path/should/not/exist/xyz123",
        ));
        assert!(result.is_err());
    }

    #[test]
    fn canonicalize_resolves_symlinks_to_target() {
        let dir = TempDir::new().unwrap();
        let target = dir.path().join("real.txt");
        std::fs::write(&target, "z").unwrap();
        let link = dir.path().join("link.txt");

        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &link).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&target, &link).unwrap();

        let via_link = canonicalize_for_authorization(&link).unwrap();
        let via_target = canonicalize_for_authorization(&target).unwrap();
        assert_eq!(via_link, via_target);
    }

    #[test]
    fn record_and_consume_treat_canonical_paths_as_keys() {
        let dir = TempDir::new().unwrap();
        let canonical = make_file(&dir, "key.txt");
        let auth = AuthorizedPaths::new();

        auth.record_for_open(canonical.clone());

        let upper = canonical.to_uppercase();
        if upper != canonical {
            assert!(!auth.consume_for_open(&upper));
        }
        assert!(auth.consume_for_open(&canonical));
    }
}
