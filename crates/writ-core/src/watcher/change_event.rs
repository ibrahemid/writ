use serde::{Deserialize, Serialize};

/// Nature of a change observed on a buffer's backing file from outside
/// Writ.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ExternalChange {
    /// The file's contents were modified.
    Modified,
    /// The file is gone and nothing carrying its identity was found, so the
    /// tab keeps its text and stops writing to the path (spec W4).
    Removed,
    /// The same file is at another path now. The tab follows it there; the
    /// text is untouched, because a move changes no bytes.
    Moved,
}

/// Whether a modification reported for an open tab's file is news for that tab.
///
/// A watcher reports what the filesystem told it, and the filesystem tells it
/// about writes that happened before the tab existed: FSEvents coalesces and
/// delivers on its own schedule, so the write that seeded a file can arrive
/// after Writ has opened and read it. Reporting that as an external change
/// tells the user their file was edited underneath them by showing them the
/// bytes they are already looking at.
///
/// `last_read` is the digest of what Writ last read from or wrote to the file,
/// and `on_disk` is the digest of what is there now. Equal digests are no news
/// — a file holding what the tab loaded from it has not changed, whoever wrote
/// it and whenever the report arrived.
///
/// `came_back` is the exception: a file that had been marked gone and is at its
/// path again is news even holding the same bytes, because the tab is refusing
/// to save until it hears so.
///
/// Either digest missing means there is nothing to compare, and an unreadable
/// file or a tab with nothing on record gets the report rather than silence.
pub fn modification_is_news(
    last_read: Option<crate::hash::Sha256Digest>,
    on_disk: Option<crate::hash::Sha256Digest>,
    came_back: bool,
) -> bool {
    if came_back {
        return true;
    }
    match (last_read, on_disk) {
        (Some(last_read), Some(on_disk)) => last_read != on_disk,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::sha256_bytes;

    #[test]
    fn a_file_holding_what_the_tab_loaded_is_not_a_change() {
        assert!(!modification_is_news(
            Some(sha256_bytes(b"as another program left it")),
            Some(sha256_bytes(b"as another program left it")),
            false
        ));
    }

    #[test]
    fn different_bytes_are_a_change() {
        assert!(modification_is_news(
            Some(sha256_bytes(b"first")),
            Some(sha256_bytes(b"second")),
            false
        ));
    }

    #[test]
    fn a_file_that_came_back_is_news_even_unchanged() {
        assert!(modification_is_news(
            Some(sha256_bytes(b"body")),
            Some(sha256_bytes(b"body")),
            true
        ));
    }

    #[test]
    fn nothing_to_compare_against_is_reported_rather_than_swallowed() {
        assert!(modification_is_news(
            None,
            Some(sha256_bytes(b"body")),
            false
        ));
        assert!(modification_is_news(
            Some(sha256_bytes(b"body")),
            None,
            false
        ));
    }
}
