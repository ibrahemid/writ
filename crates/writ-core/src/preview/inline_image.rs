//! Policy for an image the editor renders under the line that authored it.
//!
//! The preview serves its images over `writ-preview://`, which the main
//! window's CSP does not allow. An editor widget therefore carries the bytes
//! inline, as a `data:` URL, and the size refusal is decided on the file's
//! metadata before anything is read: opening a 40 MB file in order to refuse
//! it is the cost this module exists to avoid.
//!
//! Resolution is [`resolve_asset_reference`]'s, not a second one: the same
//! candidates, the same containment check, the same refusal of a symbolic
//! link.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::protocol::{resolve_asset_reference, sniff_image_mime};

/// Largest file the editor carries inline.
///
/// A `data:` URL is about four thirds of the file it holds, and every
/// occurrence of the reference shares one cached copy, so the cap is on the
/// file rather than on the document.
pub const INLINE_IMAGE_MAX_BYTES: u64 = 2 * 1024 * 1024;

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
    /// The file is over [`INLINE_IMAGE_MAX_BYTES`].
    #[error("the file is over the inline size limit")]
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
            Self::TooLarge => "too_large",
            Self::NotAnImage => "not_an_image",
        }
    }
}

/// One image, encoded for an `img` element in the editor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InlineImage {
    /// `data:<mime>;base64,…`, ready to assign to `src`.
    pub data_url: String,
    /// MIME type decided by the leading bytes.
    pub mime: String,
    /// Length of the file the URL holds.
    pub byte_len: u64,
}

/// Resolve an authored reference to a readable file inside the roots.
///
/// `note_dir` is the folder holding the file the reference was written in.
/// A reference that resolves inside a root but has no file behind it is
/// [`InlineImageRefusal::NotFound`], which the caller names rather than
/// drawing a broken image.
pub fn resolve_inline_image(
    notes_root: &Path,
    note_dir: &Path,
    reference: &str,
) -> Result<PathBuf, InlineImageRefusal> {
    let resolved = resolve_asset_reference(notes_root, note_dir, reference)
        .map_err(|_| InlineImageRefusal::OutsideRoot)?;
    if !resolved.path.is_file() {
        return Err(InlineImageRefusal::NotFound);
    }
    Ok(resolved.path)
}

/// Whether a file of `byte_len` bytes may be carried inline.
///
/// Answered from the metadata, before the read.
pub fn size_within_inline_limit(byte_len: u64) -> Result<(), InlineImageRefusal> {
    if byte_len > INLINE_IMAGE_MAX_BYTES {
        return Err(InlineImageRefusal::TooLarge);
    }
    Ok(())
}

/// Encode read bytes as a `data:` URL, refusing anything that does not sniff
/// as an image.
///
/// The extension is what a file's author controls; the leading bytes are not,
/// which is the same rule the protocol handler serves under.
pub fn inline_image_data_url(bytes: &[u8]) -> Result<InlineImage, InlineImageRefusal> {
    let mime = sniff_image_mime(bytes).ok_or(InlineImageRefusal::NotAnImage)?;
    Ok(InlineImage {
        data_url: format!("data:{};base64,{}", mime, base64_encode(bytes)),
        mime: mime.to_string(),
        byte_len: bytes.len() as u64,
    })
}

const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding (RFC 4648 §4).
fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(BASE64_ALPHABET[(triple >> 18) as usize & 0x3F] as char);
        out.push(BASE64_ALPHABET[(triple >> 12) as usize & 0x3F] as char);
        out.push(if chunk.len() > 1 {
            BASE64_ALPHABET[(triple >> 6) as usize & 0x3F] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            BASE64_ALPHABET[triple as usize & 0x3F] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_every_padding_case() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn encodes_bytes_outside_the_ascii_range() {
        assert_eq!(base64_encode(&[0xFF, 0xD8, 0xFF]), "/9j/");
        assert_eq!(base64_encode(&[0x00, 0x00, 0x00]), "AAAA");
    }
}
