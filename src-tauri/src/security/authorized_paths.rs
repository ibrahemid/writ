use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use writ_storage::paths::strip_verbatim_prefix;

use crate::poison::recover_poison;

#[derive(Default, Debug)]
struct Inner {
    pending_open: HashSet<String>,
    blessed_sources: HashSet<String>,
    rename_undo: Option<(String, RenameUndo)>,
    trash_undo: Option<(String, TrashUndo)>,
}

/// What `Undo rename` may touch: the note's file as the rename left it, the
/// name it had before, and the files the rename rewrote.
///
/// Held here and handed to the webview only as a token, so an undo writes the
/// files Rust itself rewrote and no path the webview names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenameUndo {
    /// Where the renamed note is now.
    pub renamed: PathBuf,
    /// The file name it had before the rename.
    pub previous_name: String,
    /// The files the rename rewrote, in the order it wrote them.
    pub updated: Vec<String>,
}

/// What the Undo after Move to Trash puts back: the note's path and the bytes
/// its file held when it went.
///
/// Held here and handed to the webview only as a token, like [`RenameUndo`],
/// so the undo writes the file Rust trashed and no path the webview names.
#[derive(Clone, PartialEq, Eq)]
pub struct TrashUndo {
    /// Where the note was.
    pub path: PathBuf,
    /// What its file held.
    pub bytes: Vec<u8>,
}

impl std::fmt::Debug for TrashUndo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TrashUndo")
            .field("bytes", &self.bytes.len())
            .finish_non_exhaustive()
    }
}

/// A token nobody can guess, or `None` when the system has no randomness.
fn mint_token() -> Option<String> {
    let mut bytes = [0u8; 16];
    match getrandom::fill(&mut bytes) {
        Ok(()) => Some(bytes.iter().map(|b| format!("{b:02x}")).collect()),
        Err(error) => {
            tracing::warn!(%error, "no undo token could be minted");
            None
        }
    }
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

    /// Hands a waiting open authorization back without opening anything.
    ///
    /// The one-shot token a not-downloaded answer recorded is spent by the
    /// open that follows the bytes landing. A download that ends any other way
    /// never performs that open, so the token goes back here rather than
    /// sitting in the set for the rest of the session.
    pub fn discard_pending_open(&self, canonical: &str) -> bool {
        let mut guard = recover_poison(
            self.inner.lock(),
            "security::authorized_paths::discard_pending_open",
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

    /// Blesses a stored source path and, when it resolves, its canonical form.
    ///
    /// The stored path is what a save presents; resolving can fail outright for
    /// a file deleted while its tab was away, and refusing there would leave
    /// that tab unable to recreate the file it came from.
    pub fn bless_source_path(&self, stored: &str) {
        if let Ok(canonical) = canonicalize_for_authorization(Path::new(stored)) {
            self.record_blessed_source(canonical);
        }
        self.record_blessed_source(stored.to_string());
    }

    /// Takes a source's blessing away, in both forms it may have been recorded
    /// under: the stored path and its resolved one.
    pub fn revoke_blessed_source(&self, stored: &str) {
        let resolved = canonicalize_for_authorization(Path::new(stored)).ok();
        let mut guard = recover_poison(
            self.inner.lock(),
            "security::authorized_paths::revoke_blessed_source",
        );
        guard.blessed_sources.remove(stored);
        if let Some(resolved) = resolved {
            guard.blessed_sources.remove(&resolved);
        }
    }

    /// Holds the one rename `Undo rename` can put back, replacing any earlier
    /// one, and answers the token the webview presents to undo it. `None` when
    /// the system has no randomness to mint one with, which leaves the rename
    /// without an undo rather than with a guessable one.
    pub fn hold_rename_undo(&self, undo: RenameUndo) -> Option<String> {
        let mut bytes = [0u8; 16];
        let token = match getrandom::fill(&mut bytes) {
            Ok(()) => bytes.iter().map(|b| format!("{b:02x}")).collect::<String>(),
            Err(error) => {
                tracing::warn!(%error, "no undo token could be minted for a rename");
                self.clear_rename_undo();
                return None;
            }
        };
        let mut guard = recover_poison(
            self.inner.lock(),
            "security::authorized_paths::hold_rename_undo",
        );
        guard.rename_undo = Some((token.clone(), undo));
        Some(token)
    }

    /// Spends `token`: the rename it stands for, once. A token that is not the
    /// one held answers `None` and leaves the held rename where it is.
    pub fn take_rename_undo(&self, token: &str) -> Option<RenameUndo> {
        let mut guard = recover_poison(
            self.inner.lock(),
            "security::authorized_paths::take_rename_undo",
        );
        match guard.rename_undo.as_ref() {
            Some((held, _)) if held == token => guard.rename_undo.take().map(|(_, undo)| undo),
            _ => None,
        }
    }

    /// Drops the held rename, so no token undoes it.
    pub fn clear_rename_undo(&self) {
        let mut guard = recover_poison(
            self.inner.lock(),
            "security::authorized_paths::clear_rename_undo",
        );
        guard.rename_undo = None;
    }

    /// Holds the one Move to Trash the Undo can put back, replacing any
    /// earlier one, and answers the token the webview presents to undo it.
    pub fn hold_trash_undo(&self, undo: TrashUndo) -> Option<String> {
        let token = mint_token();
        let mut guard = recover_poison(
            self.inner.lock(),
            "security::authorized_paths::hold_trash_undo",
        );
        guard.trash_undo = token.clone().map(|token| (token, undo));
        token
    }

    /// Spends `token`: the trashed note it stands for, once.
    pub fn take_trash_undo(&self, token: &str) -> Option<TrashUndo> {
        let mut guard = recover_poison(
            self.inner.lock(),
            "security::authorized_paths::take_trash_undo",
        );
        match guard.trash_undo.as_ref() {
            Some((held, _)) if held == token => guard.trash_undo.take().map(|(_, undo)| undo),
            _ => None,
        }
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
    let stripped = strip_verbatim_prefix(canonical);
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
/// never matches a candidate stripped of it. The stores strip it the same way
/// ([`strip_verbatim_prefix`]), so an authorized path and an indexed one are
/// one spelling.
pub fn canonicalize_root(path: &Path) -> std::io::Result<PathBuf> {
    Ok(strip_verbatim_prefix(std::fs::canonicalize(path)?))
}

/// [`writ_core::notes::containment::resolve_for_containment`] in the spelling
/// every other path in this crate carries.
///
/// The walk-up rule lives in `writ-core` because the MCP tool surface applies
/// it to a path a client chose (ADR-031 rule 3.7) and that crate imports no
/// tauri. What is added here is the Windows verbatim prefix and the `String`
/// the authorized-path stores are keyed by, so an authorized path and an
/// indexed one stay one spelling.
pub fn resolve_for_containment(path: &Path) -> Option<String> {
    let resolved = writ_core::notes::containment::resolve_for_containment(path)?;
    strip_verbatim_prefix(resolved)
        .into_os_string()
        .into_string()
        .ok()
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
    fn discarding_a_token_takes_only_that_path_out_of_the_set() {
        let dir = TempDir::new().unwrap();
        let canonical = make_file(&dir, "given-back.txt");
        let other = make_file(&dir, "kept.txt");
        let auth = AuthorizedPaths::new();
        auth.record_for_open(canonical.clone());
        auth.record_for_open(other.clone());

        assert!(auth.discard_pending_open(&canonical));
        assert!(!auth.is_pending_open(&canonical));
        assert!(auth.is_pending_open(&other));
        assert_eq!(auth.pending_open_len(), 1);

        // A path with no token to give back says so rather than failing.
        assert!(!auth.discard_pending_open(&canonical));
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
    fn revoking_a_source_takes_both_recorded_forms() {
        let dir = TempDir::new().expect("tempdir");
        let canonical = make_file(&dir, "revoke.txt");
        let stored = dir.path().join("revoke.txt").to_string_lossy().into_owned();
        let auth = AuthorizedPaths::new();
        auth.record_blessed_source(stored.clone());
        auth.record_blessed_source(canonical.clone());

        auth.revoke_blessed_source(&stored);

        assert!(!auth.is_blessed_source(&stored));
        assert!(!auth.is_blessed_source(&canonical));
    }

    fn undo(name: &str) -> RenameUndo {
        RenameUndo {
            renamed: PathBuf::from(format!("/notes/{name}.md")),
            previous_name: "Old.md".to_string(),
            updated: vec!["/notes/First.md".to_string()],
        }
    }

    #[test]
    fn a_rename_undo_is_spent_once_and_only_with_its_token() {
        let auth = AuthorizedPaths::new();
        let token = auth.hold_rename_undo(undo("New")).expect("a token");

        assert_eq!(auth.take_rename_undo("not-the-token"), None);
        assert_eq!(auth.take_rename_undo(&token), Some(undo("New")));
        assert_eq!(auth.take_rename_undo(&token), None);
    }

    #[test]
    fn a_later_rename_replaces_the_undo_held_for_an_earlier_one() {
        let auth = AuthorizedPaths::new();
        let first = auth.hold_rename_undo(undo("First")).expect("a token");
        let second = auth.hold_rename_undo(undo("Second")).expect("a token");

        assert_ne!(first, second);
        assert_eq!(auth.take_rename_undo(&first), None);
        assert_eq!(auth.take_rename_undo(&second), Some(undo("Second")));
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
