//! Policy for an image rendered inside the editor: what resolves, what is
//! refused, and what the refusal is called.

use std::fs;

use tempfile::TempDir;
use writ_core::preview::{
    inline_image_data_url, resolve_inline_image, size_within_inline_limit, InlineImageRefusal,
    INLINE_IMAGE_MAX_BYTES,
};

const PNG_HEADER: &[u8] = b"\x89PNG\r\n\x1a\n";

struct Fixture {
    _dir: TempDir,
    notes: std::path::PathBuf,
    note_dir: std::path::PathBuf,
}

fn fixture() -> Fixture {
    let dir = TempDir::new().unwrap();
    let notes = dir.path().join("notes");
    let note_dir = notes.join("daily");
    fs::create_dir_all(&note_dir).unwrap();
    Fixture {
        _dir: dir,
        notes,
        note_dir,
    }
}

fn write_png(path: &std::path::Path) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut bytes = PNG_HEADER.to_vec();
    bytes.extend_from_slice(b"body");
    fs::write(path, bytes).unwrap();
}

#[test]
fn resolves_a_reference_beside_the_file() {
    let f = fixture();
    write_png(&f.note_dir.join("shot.png"));
    let resolved = resolve_inline_image(&f.notes, &f.note_dir, "shot.png").unwrap();
    assert_eq!(resolved.file_name().unwrap(), "shot.png");
    // Both sides go through `canonicalize` so the comparison holds on Windows,
    // where the resolver strips the `\\?\` prefix that `canonicalize` adds.
    let resolved = std::fs::canonicalize(&resolved).unwrap();
    let notes = std::fs::canonicalize(&f.notes).unwrap();
    assert!(resolved.starts_with(notes.parent().unwrap()));
}

#[test]
fn resolves_a_reference_under_the_attachments_folder() {
    let f = fixture();
    write_png(&f.notes.join("attachments").join("logo.png"));
    let resolved = resolve_inline_image(&f.notes, &f.note_dir, "logo.png").unwrap();
    assert_eq!(resolved.file_name().unwrap(), "logo.png");
}

#[test]
fn refuses_a_reference_that_climbs_out_of_the_roots() {
    let f = fixture();
    assert_eq!(
        resolve_inline_image(&f.notes, &f.note_dir, "../../../etc/hosts"),
        Err(InlineImageRefusal::OutsideRoot),
    );
}

#[test]
fn refuses_a_reference_with_no_file_behind_it() {
    let f = fixture();
    assert_eq!(
        resolve_inline_image(&f.notes, &f.note_dir, "absent.png"),
        Err(InlineImageRefusal::NotFound),
    );
}

#[test]
fn refuses_an_empty_reference() {
    let f = fixture();
    assert_eq!(
        resolve_inline_image(&f.notes, &f.note_dir, "   "),
        Err(InlineImageRefusal::OutsideRoot),
    );
}

#[test]
fn accepts_a_file_at_the_size_limit() {
    assert_eq!(size_within_inline_limit(INLINE_IMAGE_MAX_BYTES), Ok(()));
}

#[test]
fn refuses_a_file_over_the_size_limit() {
    assert_eq!(
        size_within_inline_limit(INLINE_IMAGE_MAX_BYTES + 1),
        Err(InlineImageRefusal::TooLarge),
    );
}

#[test]
fn encodes_a_png_as_a_data_url() {
    let mut bytes = PNG_HEADER.to_vec();
    bytes.extend_from_slice(b"body");
    let image = inline_image_data_url(&bytes).unwrap();
    assert_eq!(image.mime, "image/png");
    assert_eq!(image.byte_len, bytes.len() as u64);
    assert!(image.data_url.starts_with("data:image/png;base64,"));
    assert_eq!(image.data_url, "data:image/png;base64,iVBORw0KGgpib2R5",);
}

#[test]
fn refuses_bytes_that_are_not_an_image() {
    assert_eq!(
        inline_image_data_url(b"<!doctype html><html></html>"),
        Err(InlineImageRefusal::NotAnImage),
    );
}

#[test]
fn names_every_refusal() {
    assert_eq!(InlineImageRefusal::OutsideRoot.as_str(), "outside_root");
    assert_eq!(InlineImageRefusal::NotFound.as_str(), "not_found");
    assert_eq!(InlineImageRefusal::TooLarge.as_str(), "too_large");
    assert_eq!(InlineImageRefusal::NotAnImage.as_str(), "not_an_image");
}
