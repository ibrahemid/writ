use writ_core::hash::{sha256_bytes, sha256_hex};

/// The two published SHA-256 vectors, so a swap of the digest crate cannot
/// silently change what Writ compares files by.
const EMPTY_HEX: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const ABC_HEX: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

#[test]
fn sha256_hex_matches_the_published_vectors() {
    assert_eq!(sha256_hex(b""), EMPTY_HEX);
    assert_eq!(sha256_hex(b"abc"), ABC_HEX);
}

#[test]
fn sha256_hex_is_lowercase_and_sixty_four_characters() {
    let hex = sha256_hex(b"writ");
    assert_eq!(hex.len(), 64);
    assert!(hex
        .chars()
        .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
}

#[test]
fn sha256_bytes_agrees_with_the_hex_form() {
    let digest = sha256_bytes(b"abc");
    let rendered: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    assert_eq!(rendered, ABC_HEX);
    assert_eq!(digest.len(), 32);
}

#[test]
fn different_input_gives_a_different_digest() {
    assert_ne!(sha256_bytes(b"one"), sha256_bytes(b"two"));
}
