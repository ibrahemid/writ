//! The one list of text extensions, and the questions asked of it.

use std::path::Path;

use writ_core::notes::extensions::{
    has_text_extension, is_markdown_extension, is_text_extension, MARKDOWN_EXTENSIONS,
    TEXT_EXTENSIONS,
};

#[test]
fn the_markdown_pair_is_the_head_of_the_text_list() {
    assert_eq!(TEXT_EXTENSIONS, ["md", "markdown", "txt", "text"]);
    assert_eq!(MARKDOWN_EXTENSIONS, ["md", "markdown"]);
}

#[test]
fn every_text_extension_is_recognised_in_any_case() {
    for extension in ["md", "MD", "Markdown", "txt", "TXT", "text", "Text"] {
        assert!(is_text_extension(extension), "{extension}");
    }
    for extension in ["", "rtf", "mdx", "txt.bak", "png", ".md"] {
        assert!(!is_text_extension(extension), "{extension}");
    }
}

#[test]
fn only_the_markdown_pair_is_markdown() {
    for extension in ["md", "MD", "markdown", "MarkDown"] {
        assert!(is_markdown_extension(extension), "{extension}");
    }
    for extension in ["txt", "text", "mdx", ""] {
        assert!(!is_markdown_extension(extension), "{extension}");
    }
}

#[test]
fn a_path_is_judged_by_its_last_extension() {
    assert!(has_text_extension(Path::new("/notes/Launch.md")));
    assert!(has_text_extension(Path::new("/notes/Log.TEXT")));
    assert!(has_text_extension(Path::new("notes.txt")));
    assert!(has_text_extension(Path::new("archive.tar.md")));
    assert!(!has_text_extension(Path::new("/notes/Launch.md.png")));
    assert!(!has_text_extension(Path::new("/notes/README")));
    assert!(!has_text_extension(Path::new("/notes/.md")));
}
