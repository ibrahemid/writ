//! Policy for an image rendered inside the editor: what resolves, what is
//! refused, what the refusal is called, and the route a drawn image is served
//! from.

use std::fs;

use tempfile::TempDir;
use writ_core::preview::protocol::{split_asset_request, AssetRoot, ASSET_MAX_BYTES};
use writ_core::preview::{
    check_inline_image_head, check_inline_image_stat, inline_image_route, resolve_inline_image,
    AssetScope, InlineImageRefusal,
};

const PNG_HEADER: &[u8] = b"\x89PNG\r\n\x1a\n";
const MB: u64 = 1024 * 1024;

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

fn scope(f: &Fixture, buffer_id: &str, token: &str) -> AssetScope {
    AssetScope {
        notes_root: f.notes.clone(),
        note_dir: f.note_dir.clone(),
        buffer_id: buffer_id.to_string(),
        token: token.to_string(),
    }
}

#[test]
fn resolves_a_reference_beside_the_file() {
    let f = fixture();
    write_png(&f.note_dir.join("shot.png"));
    let resolved = resolve_inline_image(&f.notes, &f.note_dir, "shot.png").unwrap();
    assert_eq!(resolved.path.file_name().unwrap(), "shot.png");
    assert_eq!(resolved.root, AssetRoot::Notes);
    assert_eq!(resolved.url_path, "daily/shot.png");
    // Both sides go through `canonicalize` so the comparison holds on Windows,
    // where the resolver strips the `\\?\` prefix that `canonicalize` adds.
    let path = std::fs::canonicalize(&resolved.path).unwrap();
    let notes = std::fs::canonicalize(&f.notes).unwrap();
    assert!(path.starts_with(notes));
}

#[test]
fn resolves_a_reference_under_the_attachments_folder() {
    let f = fixture();
    write_png(&f.notes.join("attachments").join("logo.png"));
    let resolved = resolve_inline_image(&f.notes, &f.note_dir, "logo.png").unwrap();
    assert_eq!(resolved.url_path, "attachments/logo.png");
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

/// A phone photo or a Retina screenshot. Refused as too large before the
/// editor drew from the scheme, when the cap was 2 MB.
#[test]
fn accepts_an_eight_megabyte_image() {
    assert_eq!(check_inline_image_stat(8 * MB, false), Ok(()));
}

#[test]
fn accepts_a_file_at_the_size_limit() {
    assert_eq!(check_inline_image_stat(ASSET_MAX_BYTES, false), Ok(()));
}

#[test]
fn refuses_a_file_over_the_size_limit() {
    assert_eq!(
        check_inline_image_stat(ASSET_MAX_BYTES + 1, false),
        Err(InlineImageRefusal::TooLarge),
    );
}

#[test]
fn refuses_a_placeholder_from_its_stat() {
    assert_eq!(
        check_inline_image_stat(10, true),
        Err(InlineImageRefusal::NotDownloaded),
    );
}

#[test]
fn names_a_large_placeholder_as_not_downloaded_rather_than_too_large() {
    assert_eq!(
        check_inline_image_stat(ASSET_MAX_BYTES * 4, true),
        Err(InlineImageRefusal::NotDownloaded),
    );
}

#[test]
fn types_a_png_by_its_leading_bytes() {
    assert_eq!(check_inline_image_head(PNG_HEADER), Ok("image/png"));
}

#[test]
fn refuses_bytes_that_are_not_an_image() {
    assert_eq!(
        check_inline_image_head(b"<!doctype html><html></html>"),
        Err(InlineImageRefusal::NotAnImage),
    );
}

#[test]
fn routes_a_resolved_image_through_the_note_asset_route() {
    let f = fixture();
    write_png(&f.note_dir.join("shot one.png"));
    let found = resolve_inline_image(&f.notes, &f.note_dir, "shot one.png").unwrap();
    let route = inline_image_route(&scope(&f, "buf-1", "tok_1"), &found).unwrap();
    assert_eq!(route, "_note-asset/buf-1/tok_1/n/daily/shot%20one.png");

    // The route splits back into the parts the handler re-resolves.
    let decoded = route.replace("%20", " ");
    let request = split_asset_request(&decoded).unwrap();
    assert_eq!(request.buffer_id, "buf-1");
    assert_eq!(request.token, "tok_1");
    assert_eq!(request.root, AssetRoot::Notes);
    assert_eq!(request.relative, "daily/shot one.png");
}

#[test]
fn mints_no_route_for_a_segment_that_would_move_the_split() {
    let f = fixture();
    write_png(&f.note_dir.join("shot.png"));
    let found = resolve_inline_image(&f.notes, &f.note_dir, "shot.png").unwrap();
    for (buffer_id, token) in [
        ("buf/1", "tok"),
        ("buf-1", "t%2F"),
        ("", "tok"),
        ("buf", ""),
    ] {
        assert_eq!(
            inline_image_route(&scope(&f, buffer_id, token), &found),
            None,
            "{buffer_id:?} {token:?}"
        );
    }
}

#[test]
fn names_every_refusal() {
    assert_eq!(InlineImageRefusal::OutsideRoot.as_str(), "outside_root");
    assert_eq!(InlineImageRefusal::NotFound.as_str(), "not_found");
    assert_eq!(InlineImageRefusal::NotDownloaded.as_str(), "not_downloaded");
    assert_eq!(InlineImageRefusal::TooLarge.as_str(), "too_large");
    assert_eq!(InlineImageRefusal::NotAnImage.as_str(), "not_an_image");
}
