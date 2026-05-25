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

pub fn canonicalize_for_authorization(path: &Path) -> std::io::Result<String> {
    let canonical: PathBuf = std::fs::canonicalize(path)?;
    let stripped = strip_unc_prefix(canonical);
    stripped
        .into_os_string()
        .into_string()
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "non-utf8 path"))
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
