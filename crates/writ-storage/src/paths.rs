//! The one spelling of a path every surface agrees on.

use std::path::{Path, PathBuf};

/// Drops the `\\?\` prefix Windows canonicalisation adds.
///
/// `std::fs::canonicalize` answers in the verbatim form: `\\?\C:\notes\a.md`
/// for a drive path, `\\?\UNC\server\share\a.md` for a network share. Neither
/// is the spelling anything else in the app carries, and two surfaces keyed by
/// the two forms never match: the index would hold one and the file tree the
/// other, so a note carrying a tag would not be the note the tree draws.
///
/// The drive form reads as `C:\notes\a.md` and the share form as
/// `\\server\share\a.md` — in both cases the spelling that opens the file.
/// Anything else, including a name that merely begins with backslashes on a
/// filesystem that allows them, comes back untouched.
pub fn strip_verbatim_prefix(path: PathBuf) -> PathBuf {
    let Some(text) = path.to_str() else {
        return path;
    };
    let Some(rest) = text.strip_prefix(r"\\?\") else {
        return path;
    };
    if let Some(share) = rest.strip_prefix(r"UNC\") {
        return PathBuf::from(format!(r"\\{share}"));
    }
    if starts_with_drive(rest) {
        return PathBuf::from(rest);
    }
    path
}

/// `path` relative to `root`, with forward slashes, or `None` when it is not
/// under the root.
///
/// Both sides drop the `\\?\` Windows canonicalisation adds before they are
/// compared ([`strip_verbatim_prefix`]). The two spellings arrive from
/// different places and rarely agree on their own: one side has been through
/// `canonicalize`, which keeps the verbatim form, and the other through an
/// index key or a folder the app stored as the person typed it, which does
/// not. Comparing the two as they come answers `None` on Windows for a file
/// plainly in the folder.
pub fn relative_slug(root: &Path, path: &Path) -> Option<String> {
    let root = strip_verbatim_prefix(root.to_path_buf());
    let path = strip_verbatim_prefix(path.to_path_buf());
    let relative = path.strip_prefix(&root).ok()?;
    Some(
        relative
            .components()
            .map(|component| component.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/"),
    )
}

/// The file name at the end of a path argument.
///
/// What a log line or a message takes when a note cannot be spelled relative
/// to the notes folder. A name says which note without saying where the folder
/// is.
pub fn file_name_only(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

/// `true` for `C:\…`, the only other shape canonicalisation answers with.
fn starts_with_drive(text: &str) -> bool {
    let mut chars = text.chars();
    matches!(
        (chars.next(), chars.next()),
        (Some(letter), Some(':')) if letter.is_ascii_alphabetic()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_verbatim_drive_path_reads_as_the_drive_path() {
        assert_eq!(
            strip_verbatim_prefix(PathBuf::from(r"\\?\C:\Users\ibra\notes\Launch.md")),
            PathBuf::from(r"C:\Users\ibra\notes\Launch.md")
        );
    }

    #[test]
    fn a_verbatim_share_reads_as_the_share() {
        assert_eq!(
            strip_verbatim_prefix(PathBuf::from(r"\\?\UNC\server\share\notes\Launch.md")),
            PathBuf::from(r"\\server\share\notes\Launch.md")
        );
    }

    #[test]
    fn a_path_without_the_prefix_is_left_alone() {
        for path in [r"C:\notes\Launch.md", "/Users/ibra/notes/Launch.md"] {
            assert_eq!(
                strip_verbatim_prefix(PathBuf::from(path)),
                PathBuf::from(path)
            );
        }
    }

    #[test]
    fn a_name_beginning_with_the_prefix_but_naming_no_drive_is_left_alone() {
        let odd = PathBuf::from(r"\\?\notes");
        assert_eq!(strip_verbatim_prefix(odd.clone()), odd);
    }

    #[test]
    fn stripping_a_stripped_path_changes_nothing() {
        let once = strip_verbatim_prefix(PathBuf::from(r"\\?\C:\notes\Launch.md"));
        assert_eq!(strip_verbatim_prefix(once.clone()), once);
    }

    #[test]
    fn a_file_under_the_root_is_spelled_with_forward_slashes() {
        assert_eq!(
            relative_slug(Path::new("/notes"), Path::new("/notes/Projects/Launch.md")),
            Some("Projects/Launch.md".to_string())
        );
    }

    #[test]
    fn a_root_and_a_file_that_differ_only_in_punctuation_still_meet() {
        assert_eq!(
            relative_slug(Path::new("/notes/"), Path::new("/notes/./Launch.md")),
            Some("Launch.md".to_string())
        );
    }

    #[test]
    fn a_file_the_root_does_not_hold_has_no_slug() {
        assert_eq!(
            relative_slug(Path::new("/notes"), Path::new("/elsewhere/Launch.md")),
            None
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_verbatim_root_and_a_plain_file_are_the_same_folder() {
        // What Windows hands the two sides: one path came back from
        // `canonicalize` with the prefix, the other was stripped of it.
        assert_eq!(
            relative_slug(
                Path::new(r"\\?\C:\notes"),
                Path::new(r"C:\notes\Projects\Launch.md")
            ),
            Some("Projects/Launch.md".to_string())
        );
        assert_eq!(
            relative_slug(
                Path::new(r"C:\notes"),
                Path::new(r"\\?\C:\notes\Projects\Launch.md")
            ),
            Some("Projects/Launch.md".to_string())
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_verbatim_share_and_a_plain_file_are_the_same_folder() {
        assert_eq!(
            relative_slug(
                Path::new(r"\\?\UNC\server\share\notes"),
                Path::new(r"\\server\share\notes\Launch.md")
            ),
            Some("Launch.md".to_string())
        );
    }

    #[cfg(windows)]
    #[test]
    fn a_root_and_a_file_that_separate_their_names_differently_still_meet() {
        assert_eq!(
            relative_slug(
                Path::new(r"C:\notes"),
                Path::new("C:/notes/Projects/Launch.md")
            ),
            Some("Projects/Launch.md".to_string())
        );
    }

    #[test]
    fn a_name_is_what_is_left_of_a_path_that_names_no_folder() {
        assert_eq!(file_name_only("/notes/Ideas/Launch.md"), "Launch.md");
        assert_eq!(file_name_only("Launch.md"), "Launch.md");
        assert_eq!(file_name_only(".."), "..");
    }

    #[cfg(windows)]
    #[test]
    fn a_windows_path_is_named_by_its_last_segment() {
        assert_eq!(file_name_only(r"\\?\C:\notes\Ideas\Launch.md"), "Launch.md");
    }
}
