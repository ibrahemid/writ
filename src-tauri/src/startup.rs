use std::ffi::OsString;
use std::path::Path;
use std::sync::Mutex;

use writ_core::file_ops::arg_paths_from_iter;

use crate::security::{canonicalize_for_authorization, AuthorizedPaths};

pub fn push_arg_paths_into_pending<I>(
    pending: &Mutex<Vec<String>>,
    authorized: &AuthorizedPaths,
    args: I,
) -> usize
where
    I: IntoIterator<Item = OsString>,
{
    let paths = arg_paths_from_iter(args);
    if paths.is_empty() {
        return 0;
    }

    let strings: Vec<String> = paths
        .into_iter()
        .filter_map(|p| p.to_str().map(String::from))
        .collect();

    if strings.is_empty() {
        return 0;
    }

    let mut count = 0usize;
    let mut to_push: Vec<String> = Vec::with_capacity(strings.len());
    for raw in &strings {
        if let Ok(canonical) = canonicalize_for_authorization(Path::new(raw)) {
            authorized.record_for_open(canonical.clone());
            to_push.push(canonical);
            count += 1;
        }
    }
    if count == 0 {
        return 0;
    }

    let mut guard = match pending.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    guard.extend(to_push);
    count
}

pub fn authorize_and_canonicalize(
    authorized: &AuthorizedPaths,
    raw_paths: &[String],
) -> Vec<String> {
    let mut out = Vec::with_capacity(raw_paths.len());
    for raw in raw_paths {
        if let Ok(canonical) = canonicalize_for_authorization(Path::new(raw)) {
            authorized.record_for_open(canonical.clone());
            out.push(canonical);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(s: &str) -> OsString {
        OsString::from(s)
    }

    #[test]
    fn extends_pending_with_real_files() {
        let dir = tempfile::TempDir::new().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.md");
        std::fs::write(&a, "alpha").unwrap();
        std::fs::write(&b, "beta").unwrap();

        let pending = Mutex::new(Vec::<String>::new());
        let authorized = AuthorizedPaths::new();
        let argv = vec![
            os("/usr/local/bin/writ"),
            OsString::from(&a),
            OsString::from(&b),
        ];

        let count = push_arg_paths_into_pending(&pending, &authorized, argv.into_iter().skip(1));
        assert_eq!(count, 2);

        let stored = pending.lock().unwrap();
        assert_eq!(stored.len(), 2);
        assert!(stored.iter().any(|p| p.ends_with("a.txt")));
        assert!(stored.iter().any(|p| p.ends_with("b.md")));
    }

    #[test]
    fn ignores_bogus_paths_and_flags() {
        let dir = tempfile::TempDir::new().unwrap();
        let real = dir.path().join("real.txt");
        std::fs::write(&real, "x").unwrap();

        let pending = Mutex::new(Vec::<String>::new());
        let authorized = AuthorizedPaths::new();
        let argv = vec![
            os("/usr/local/bin/writ"),
            OsString::from(&real),
            os("/tmp/writ-bogus-does-not-exist.txt"),
            os("--foo"),
            os("-v"),
        ];

        let count = push_arg_paths_into_pending(&pending, &authorized, argv.into_iter().skip(1));
        assert_eq!(count, 1);

        let stored = pending.lock().unwrap();
        assert_eq!(stored.len(), 1);
        assert!(stored[0].ends_with("real.txt"));
    }

    #[test]
    fn empty_args_leaves_pending_untouched() {
        let pending = Mutex::new(vec!["preexisting".to_string()]);
        let authorized = AuthorizedPaths::new();
        let count =
            push_arg_paths_into_pending(&pending, &authorized, std::iter::empty::<OsString>());
        assert_eq!(count, 0);

        let stored = pending.lock().unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0], "preexisting");
    }

    #[test]
    fn appends_to_existing_pending() {
        let dir = tempfile::TempDir::new().unwrap();
        let f = dir.path().join("f.txt");
        std::fs::write(&f, "x").unwrap();

        let pending = Mutex::new(vec!["already-there.txt".to_string()]);
        let authorized = AuthorizedPaths::new();
        let count = push_arg_paths_into_pending(&pending, &authorized, vec![OsString::from(&f)]);
        assert_eq!(count, 1);

        let stored = pending.lock().unwrap();
        assert_eq!(stored.len(), 2);
        assert_eq!(stored[0], "already-there.txt");
        assert!(stored[1].ends_with("f.txt"));
    }

    #[test]
    fn realistic_cold_launch_argv() {
        let dir = tempfile::TempDir::new().unwrap();
        let a = dir.path().join("file_a.rs");
        let b = dir.path().join("file_b.md");
        std::fs::write(&a, "fn main() {}").unwrap();
        std::fs::write(&b, "# hi").unwrap();

        let pending = Mutex::new(Vec::<String>::new());
        let authorized = AuthorizedPaths::new();

        let argv: Vec<OsString> = vec![
            os("C:\\Program Files\\Writ\\writ.exe"),
            OsString::from(&a),
            OsString::from(&b),
            os("/tmp/writ-not-real-1234.txt"),
            os("--foo"),
        ];

        let count = push_arg_paths_into_pending(&pending, &authorized, argv.into_iter().skip(1));
        assert_eq!(count, 2);

        let stored = pending.lock().unwrap();
        assert_eq!(stored.len(), 2);
        assert!(stored.iter().any(|p| p.ends_with("file_a.rs")));
        assert!(stored.iter().any(|p| p.ends_with("file_b.md")));
    }

    #[test]
    fn push_arg_paths_records_authorization_for_each_pushed_path() {
        let dir = tempfile::TempDir::new().unwrap();
        let a = dir.path().join("auth_a.txt");
        std::fs::write(&a, "x").unwrap();

        let pending = Mutex::new(Vec::<String>::new());
        let authorized = AuthorizedPaths::new();

        let count = push_arg_paths_into_pending(&pending, &authorized, vec![OsString::from(&a)]);
        assert_eq!(count, 1);

        let stored = pending.lock().unwrap();
        let canonical = stored[0].clone();
        drop(stored);

        assert!(authorized.consume_for_open(&canonical));
        assert!(!authorized.consume_for_open(&canonical));
    }
}
