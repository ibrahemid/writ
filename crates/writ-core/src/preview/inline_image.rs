//! Policy for an image the editor renders under the line that authored it.
//!
//! The editor draws the image from the preview scheme's note-asset route
//! (ADR-035), the same route and the same scope the preview pane's images use,
//! so no bytes cross IPC. What is decided here is whether a URL is handed out
//! at all, and every refusal is decided before the file's bytes are read: a
//! placeholder a sync provider has not downloaded is named from its stat,
//! because reading it would download it, and a file over the cap is refused on
//! its length.
//!
//! Resolution is [`resolve_asset_reference`]'s, not a second one: the same
//! candidates, the same containment check, the same refusal of a symbolic
//! link.

use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::protocol::{
    resolve_asset_reference, sniff_image_mime, AssetReference, ASSET_MAX_BYTES, ASSET_PREFIX,
};
use super::types::AssetScope;

/// Why an image is not rendered inline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Error)]
#[serde(rename_all = "snake_case")]
pub enum InlineImageRefusal {
    /// The reference resolves outside the notes folder and the file's own
    /// folder, or names a symbolic link.
    #[error("the reference resolves outside both roots")]
    OutsideRoot,
    /// Nothing is on disk at the resolved path.
    #[error("no file at the resolved path")]
    NotFound,
    /// A sync provider holds the file's bytes and this machine does not.
    #[error("the file's bytes are not on this machine")]
    NotDownloaded,
    /// The file is over [`ASSET_MAX_BYTES`].
    #[error("the file is over the image size limit")]
    TooLarge,
    /// The leading bytes are not one of the image formats Writ serves.
    #[error("the leading bytes are not an image")]
    NotAnImage,
}

impl InlineImageRefusal {
    /// Stable identifier for the refusal, shared with the frontend.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::OutsideRoot => "outside_root",
            Self::NotFound => "not_found",
            Self::NotDownloaded => "not_downloaded",
            Self::TooLarge => "too_large",
            Self::NotAnImage => "not_an_image",
        }
    }
}

/// Resolve an authored reference to a file inside the roots.
///
/// `note_dir` is the folder holding the file the reference was written in.
/// A reference that resolves inside a root but has no file behind it is
/// [`InlineImageRefusal::NotFound`], which the caller names rather than
/// drawing a broken image. Only metadata is read.
pub fn resolve_inline_image(
    notes_root: &Path,
    note_dir: &Path,
    reference: &str,
) -> Result<AssetReference, InlineImageRefusal> {
    let resolved = resolve_asset_reference(notes_root, note_dir, reference)
        .map_err(|_| InlineImageRefusal::OutsideRoot)?;
    if !resolved.path.is_file() {
        return Err(InlineImageRefusal::NotFound);
    }
    Ok(resolved)
}

/// Whether a file may be drawn, decided from its stat alone.
///
/// `dataless` is what a metadata-only probe said about the file. A placeholder
/// is refused before its length is looked at: the length a provider reports
/// for one is the size of the download, not of anything on this machine.
pub fn check_inline_image_stat(byte_len: u64, dataless: bool) -> Result<(), InlineImageRefusal> {
    if dataless {
        return Err(InlineImageRefusal::NotDownloaded);
    }
    if byte_len > ASSET_MAX_BYTES {
        return Err(InlineImageRefusal::TooLarge);
    }
    Ok(())
}

/// The MIME type the leading bytes name, or a refusal when they name none.
///
/// The extension is what a file's author controls; the leading bytes are not,
/// which is the rule the note-asset route serves under. `head` needs no more
/// than [`SNIFF_BYTES`](super::protocol::SNIFF_BYTES).
pub fn check_inline_image_head(head: &[u8]) -> Result<&'static str, InlineImageRefusal> {
    sniff_image_mime(head).ok_or(InlineImageRefusal::NotAnImage)
}

/// The path, under the document scope, that serves `found` for `scope`:
/// `_note-asset/<buffer id>/<token>/<root>/<relative path>`.
///
/// `None` when the buffer id or the token is not one `[A-Za-z0-9_-]` segment.
/// Both are host-minted, so such a value is refused rather than escaped: a `/`
/// or `%` in either would move where the route splits the URL (ADR-035).
pub fn inline_image_route(scope: &AssetScope, found: &AssetReference) -> Option<String> {
    if !is_url_segment(&scope.buffer_id) || !is_url_segment(&scope.token) {
        return None;
    }
    Some(format!(
        "{ASSET_PREFIX}/{}/{}/{}/{}",
        scope.buffer_id,
        scope.token,
        found.root.as_str(),
        found.url_path
    ))
}

fn is_url_segment(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}
