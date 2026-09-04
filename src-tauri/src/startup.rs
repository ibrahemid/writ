use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use writ_core::file_ops::arg_paths_from_iter;
use writ_core::startup::{classify_data_dir, DataDirVerdict, Platform};

use crate::security::{canonicalize_for_authorization, resolve_for_containment, AuthorizedPaths};

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

/// The platform table [`writ_core::startup::classify_data_dir`] is given.
///
/// Each host builds only its own constant; the other tables are reached from
/// tests, which pass the variant they mean.
#[cfg(target_os = "macos")]
pub const HOST_PLATFORM: Platform = Platform::Macos;
/// The platform table [`writ_core::startup::classify_data_dir`] is given.
#[cfg(target_os = "windows")]
pub const HOST_PLATFORM: Platform = Platform::Windows;
/// The platform table [`writ_core::startup::classify_data_dir`] is given.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub const HOST_PLATFORM: Platform = Platform::Linux;

/// The `.stfolder` marker file name Syncthing writes at the top of every
/// folder it syncs.
const STFOLDER: &str = ".stfolder";

/// Ancestors of `dir` that hold a [`STFOLDER`] marker, nearest first.
///
/// Syncthing names a synced folder whatever the user named it, so the marker
/// is the only signal there is. The walk is bounded by the ancestor count, so
/// it costs a handful of `exists()` calls on the startup path.
pub fn stfolder_markers(dir: &Path) -> Vec<PathBuf> {
    dir.ancestors()
        .filter(|ancestor| ancestor.join(STFOLDER).exists())
        .map(Path::to_path_buf)
        .collect()
}

/// Asks [`classify_data_dir`] where `data_dir` will land and returns the
/// first answer that stops the launch.
///
/// The policy compares paths as given, so the adapter has to resolve them
/// first. [`resolve_for_containment`] resolves the deepest part of the path
/// that exists and appends the rest, which is what makes the answer honest on
/// a first launch: the folder Writ is about to create does not exist, so
/// `canonicalize` alone says nothing, and a symlinked parent would carry the
/// database into a synced folder unseen. On macOS it also settles the
/// `/var` against `/private/var` spelling `notes_root` arrives in.
///
/// The resolved path is asked about first, so a refusal names the folder the
/// database would land in. The path as given is asked about second, for the
/// paths resolution has no answer for: a relative or non-UTF-8 `WRIT_DATA_DIR`
/// returns `None` there, and so does a symlink whose target does not exist,
/// which `create_dir_all` then turns down in `AppState::initialize` rather
/// than following.
pub fn data_dir_verdict(
    data_dir: &Path,
    home: Option<&Path>,
    notes_root: Option<&Path>,
) -> DataDirVerdict {
    let mut spellings = Vec::with_capacity(2);
    if let Some(planned) = resolve_for_containment(data_dir).map(PathBuf::from) {
        spellings.push(planned);
    }
    if !spellings.iter().any(|known| known == data_dir) {
        spellings.push(data_dir.to_path_buf());
    }

    for spelling in &spellings {
        let verdict = classify_data_dir(
            HOST_PLATFORM,
            spelling,
            home,
            notes_root,
            &stfolder_markers(spelling),
        );
        if verdict != DataDirVerdict::Ok {
            return verdict;
        }
    }
    DataDirVerdict::Ok
}

/// The sync service whose tree `dir` sits in, as the user knows it, or `None`
/// when the folder is not in one.
///
/// The same classifier that refuses a data folder in a synced tree
/// ([`classify_data_dir`]), asked about the notes folder, where the answer is
/// the opposite: notes in a synced folder are the point, and the name is what
/// Settings reports. `notes_root` is not passed, because the overlap between
/// the two folders is the data folder's question and is answered at launch.
pub fn sync_provider_for(dir: &Path, home: Option<&Path>) -> Option<String> {
    let resolved = resolve_for_containment(dir)
        .map(PathBuf::from)
        .unwrap_or_else(|| dir.to_path_buf());

    match classify_data_dir(
        HOST_PLATFORM,
        &resolved,
        home,
        None,
        &stfolder_markers(&resolved),
    ) {
        DataDirVerdict::InsideSyncProvider { provider, .. } => Some(provider.label().to_string()),
        DataDirVerdict::InsideSyncContainer { name, .. } => Some(name),
        DataDirVerdict::Ok | DataDirVerdict::InsideNotesFolder { .. } => None,
    }
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
    fn a_notes_folder_under_a_syncthing_marker_names_syncthing() {
        let dir = tempfile::TempDir::new().unwrap();
        let synced = dir.path().join("Sync");
        std::fs::create_dir_all(synced.join(".stfolder")).unwrap();
        let notes = synced.join("Writ");
        std::fs::create_dir_all(&notes).unwrap();

        assert_eq!(
            sync_provider_for(&notes, None),
            Some("Syncthing".to_string())
        );
    }

    #[test]
    fn a_notes_folder_under_a_provider_folder_names_the_provider() {
        let dir = tempfile::TempDir::new().unwrap();
        let home = crate::security::canonicalize_root(dir.path()).unwrap();
        let notes = home.join("Dropbox").join("Writ");
        std::fs::create_dir_all(&notes).unwrap();

        assert_eq!(
            sync_provider_for(&notes, Some(&home)),
            Some("Dropbox".to_string())
        );
    }

    #[test]
    fn a_notes_folder_outside_a_synced_tree_names_nothing() {
        let dir = tempfile::TempDir::new().unwrap();
        let home = crate::security::canonicalize_root(dir.path()).unwrap();
        let notes = home.join("Writ");
        std::fs::create_dir_all(&notes).unwrap();

        assert_eq!(sync_provider_for(&notes, Some(&home)), None);
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
