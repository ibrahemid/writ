//! The one spelling of a path every surface agrees on.

use std::path::PathBuf;

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
}
