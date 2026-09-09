//! SHA-256 helpers.
//!
//! Writ compares file content by digest in three places: the watcher stamps
//! that recognise Writ's own writes ([`crate::watcher::ignore`]), the write
//! guard that refuses a save over a file changed underneath it, and the notes
//! migration that verifies a written file before it removes anything. All
//! three have to agree on one digest, so it is defined once here.
//!
//! There is a fourth comparison with a different question to answer: whether
//! the document in the editor still says what its file says. Line endings are
//! not part of that answer — the editor holds `\n` whatever the file uses —
//! so it runs on [`comparison_digest_hex`] rather than the raw digest above.
//! The two must not be merged: the raw one is what lets the write guard see a
//! file somebody else rewrote with different line endings.

use std::borrow::Cow;

use sha2::{Digest, Sha256};

/// A SHA-256 digest as raw bytes.
pub type Sha256Digest = [u8; 32];

const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";

/// Computes the SHA-256 digest of `bytes`.
pub fn sha256_bytes(bytes: &[u8]) -> Sha256Digest {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let result = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&result);
    out
}

/// Computes the SHA-256 digest of `bytes` as 64 lowercase hex characters.
///
/// This is the form stored in the database, where a digest has to survive a
/// TEXT column and a JSON report.
pub fn sha256_hex(bytes: &[u8]) -> String {
    digest_hex(sha256_bytes(bytes))
}

/// Renders an already-computed digest in the same 64 lowercase hex characters
/// [`sha256_hex`] produces.
///
/// The write guard holds a digest of bytes it has already dropped, and
/// re-reading a file to name it in an error would read it twice.
pub fn digest_hex(digest: Sha256Digest) -> String {
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push(HEX_DIGITS[(byte >> 4) as usize] as char);
        out.push(HEX_DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

/// The digest 64 lowercase hex characters spell, or `None` for anything else.
///
/// The inverse of [`digest_hex`], for a digest that has been out of the
/// process and come back: a client hands one back to say which text it read
/// before it rewrote a note. Anything that is not exactly 64 hex characters is
/// `None` rather than a guess, because a caller that mistyped the digest of
/// the text it read is asking about a file it has not seen.
pub fn digest_from_hex(hex: &str) -> Option<Sha256Digest> {
    if hex.len() != 64 {
        return None;
    }
    let mut digest = [0u8; 32];
    for (byte, pair) in digest.iter_mut().zip(hex.as_bytes().chunks_exact(2)) {
        let high = nibble(pair[0])?;
        let low = nibble(pair[1])?;
        *byte = (high << 4) | low;
    }
    Some(digest)
}

/// One lowercase hex character as its value.
fn nibble(character: u8) -> Option<u8> {
    match character {
        b'0'..=b'9' => Some(character - b'0'),
        b'a'..=b'f' => Some(character - b'a' + 10),
        _ => None,
    }
}

/// `bytes` with every line break as a single `\n`.
///
/// The one normalisation rule, mirrored by `normaliseLineEndings` in
/// `src/lib/doc-hash.ts`; the fixture in `tests/fixtures/line-ending-digests.json`
/// is what keeps the two honest. Borrowed unchanged when there is no `\r` in
/// the input, which is every file Writ has written.
pub fn normalise_line_endings(bytes: &[u8]) -> Cow<'_, [u8]> {
    if !bytes.contains(&b'\r') {
        return Cow::Borrowed(bytes);
    }
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\r' {
            out.push(b'\n');
            // A lone `\r` is a line break of its own (classic Mac), so only a
            // `\n` that belongs to this break is swallowed.
            index += if bytes.get(index + 1) == Some(&b'\n') {
                2
            } else {
                1
            };
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    Cow::Owned(out)
}

/// The digest that answers "does this document still say what its file says",
/// as 64 lowercase hex characters.
///
/// Both sides of that comparison compute it: Rust over the file's bytes, the
/// editor over the document CodeMirror holds. Line endings are normalised
/// first ([`normalise_line_endings`]) because CodeMirror converts them on
/// load, so a CRLF file would otherwise read as edited from the moment it
/// opened.
pub fn comparison_digest_hex(bytes: &[u8]) -> String {
    sha256_hex(&normalise_line_endings(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_is_the_byte_digest_rendered_in_order() {
        let bytes = sha256_bytes(b"writ");
        let hex = sha256_hex(b"writ");
        for (index, byte) in bytes.iter().enumerate() {
            assert_eq!(&hex[index * 2..index * 2 + 2], format!("{byte:02x}"));
        }
    }

    #[test]
    fn equal_input_gives_an_equal_digest() {
        assert_eq!(sha256_bytes(b"same"), sha256_bytes(b"same"));
    }

    #[test]
    fn a_digest_survives_a_round_trip_through_hex() {
        let digest = sha256_bytes(b"writ");
        assert_eq!(digest_from_hex(&digest_hex(digest)), Some(digest));
    }

    #[test]
    fn anything_that_is_not_sixty_four_hex_characters_is_no_digest() {
        let hex = sha256_hex(b"writ");
        assert_eq!(digest_from_hex(""), None);
        assert_eq!(digest_from_hex(&hex[..63]), None);
        assert_eq!(digest_from_hex(&format!("{hex}0")), None);
        assert_eq!(digest_from_hex(&hex.to_uppercase()), None);
        assert_eq!(digest_from_hex(&"z".repeat(64)), None);
    }
}
