//! SHA-256 helpers.
//!
//! Writ compares file content by digest in three places: the watcher stamps
//! that recognise Writ's own writes ([`crate::watcher::ignore`]), the write
//! guard that refuses a save over a file changed underneath it, and the notes
//! migration that verifies a written file before it removes anything. All
//! three have to agree on one digest, so it is defined once here.

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
}
