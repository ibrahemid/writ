//! The file extensions Writ reads as text from the name alone.
//!
//! One list, so the index, the folder listing a connected program sees, the
//! name a person types and the language a file opens in all agree on what a
//! text file is. Every other surface asks this module rather than keeping a
//! copy that drifts.

use std::path::Path;

/// Every extension Writ treats as a text file without opening it: the two
/// Markdown spellings first, then the two plain-text ones.
pub const TEXT_EXTENSIONS: &[&str] = &["md", "markdown", "txt", "text"];

/// The two of [`TEXT_EXTENSIONS`] that name Markdown.
///
/// A link may spell one of these after a note's name and still mean the note,
/// and a file carrying one opens in the Markdown language.
pub const MARKDOWN_EXTENSIONS: &[&str] = TEXT_EXTENSIONS.split_at(2).0;

/// Whether `extension`, without its dot, is one of [`TEXT_EXTENSIONS`].
/// Compared without regard to ASCII case.
pub fn is_text_extension(extension: &str) -> bool {
    TEXT_EXTENSIONS
        .iter()
        .any(|known| known.eq_ignore_ascii_case(extension))
}

/// Whether `extension`, without its dot, is one of [`MARKDOWN_EXTENSIONS`].
/// Compared without regard to ASCII case.
pub fn is_markdown_extension(extension: &str) -> bool {
    MARKDOWN_EXTENSIONS
        .iter()
        .any(|known| known.eq_ignore_ascii_case(extension))
}

/// Whether the file name at the end of `path` carries one of
/// [`TEXT_EXTENSIONS`].
pub fn has_text_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(is_text_extension)
}
