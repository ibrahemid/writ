//! Saving a pasted or dropped image (ADR-035).

use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;
use writ_storage::image_store::{make_folder_within, save_image, ImageSaveError, SavedImage};

const PNG_A: &[u8] = b"\x89PNG\r\n\x1a\nAAAA";
const PNG_B: &[u8] = b"\x89PNG\r\n\x1a\nBBBB";

fn folder() -> (TempDir, PathBuf) {
    let dir = TempDir::new().expect("temp dir");
    let assets = dir.path().join("assets");
    (dir, assets)
}

fn no_stamp(_: &Path) {}

#[test]
fn a_missing_folder_is_made_and_the_image_written_under_its_name() {
    let (_dir, assets) = folder();
    let saved = save_image(&assets, "shot.png", PNG_A, no_stamp, no_stamp).unwrap();
    assert_eq!(
        saved,
        SavedImage::Written {
            name: "shot.png".to_string(),
            path: assets.join("shot.png")
        }
    );
    assert_eq!(fs::read(assets.join("shot.png")).unwrap(), PNG_A);
}

#[test]
fn a_name_holding_other_bytes_moves_the_image_to_a_number() {
    let (_dir, assets) = folder();
    fs::create_dir_all(&assets).unwrap();
    fs::write(assets.join("shot.png"), PNG_B).unwrap();
    fs::write(assets.join("shot-2.png"), b"other").unwrap();
    let saved = save_image(&assets, "shot.png", PNG_A, no_stamp, no_stamp).unwrap();
    assert_eq!(saved.name(), "shot-3.png");
    assert!(matches!(saved, SavedImage::Written { .. }));
    assert_eq!(fs::read(assets.join("shot.png")).unwrap(), PNG_B);
    assert_eq!(fs::read(assets.join("shot-3.png")).unwrap(), PNG_A);
}

#[test]
fn a_file_already_holding_the_bytes_is_reused() {
    let (_dir, assets) = folder();
    fs::create_dir_all(&assets).unwrap();
    fs::write(assets.join("shot.png"), PNG_B).unwrap();
    fs::write(assets.join("shot-2.png"), PNG_A).unwrap();
    let mut stamped = Vec::new();
    let saved = save_image(
        &assets,
        "shot.png",
        PNG_A,
        |path: &Path| stamped.push(path.to_path_buf()),
        no_stamp,
    )
    .unwrap();
    assert_eq!(
        saved,
        SavedImage::Reused {
            name: "shot-2.png".to_string(),
            path: assets.join("shot-2.png")
        }
    );
    assert!(stamped.is_empty());
    assert!(!assets.join("shot-3.png").exists());
}

#[test]
fn a_folder_under_the_name_is_passed_over() {
    let (_dir, assets) = folder();
    fs::create_dir_all(assets.join("shot.png")).unwrap();
    let saved = save_image(&assets, "shot.png", PNG_A, no_stamp, no_stamp).unwrap();
    assert_eq!(saved.name(), "shot-2.png");
}

#[test]
fn the_write_is_announced_before_it_lands() {
    let (_dir, assets) = folder();
    let mut stamped = Vec::new();
    save_image(
        &assets,
        "shot.png",
        PNG_A,
        |path: &Path| {
            assert!(!path.exists());
            stamped.push(path.to_path_buf());
        },
        no_stamp,
    )
    .unwrap();
    assert_eq!(stamped, vec![assets.join("shot.png")]);
}

#[test]
fn a_name_taken_between_the_check_and_the_write_takes_back_its_mark() {
    let (_dir, assets) = folder();
    let mut stamped = Vec::new();
    let mut lost = Vec::new();
    let saved = save_image(
        &assets,
        "shot.png",
        PNG_A,
        |path: &Path| {
            if path.ends_with("shot.png") {
                fs::write(path, PNG_B).unwrap();
            }
            stamped.push(path.to_path_buf());
        },
        |path: &Path| lost.push(path.to_path_buf()),
    )
    .unwrap();
    assert_eq!(saved.name(), "shot-2.png");
    assert_eq!(
        stamped,
        vec![assets.join("shot.png"), assets.join("shot-2.png")]
    );
    assert_eq!(lost, vec![assets.join("shot.png")]);
    assert_eq!(fs::read(assets.join("shot.png")).unwrap(), PNG_B);
}

#[test]
fn a_write_that_fails_takes_back_its_mark() {
    let (_dir, assets) = folder();
    let mut lost = Vec::new();
    let result = save_image(
        &assets,
        "shot.png",
        PNG_A,
        |path: &Path| fs::remove_dir(path.parent().unwrap()).unwrap(),
        |path: &Path| lost.push(path.to_path_buf()),
    );
    assert!(result.is_err(), "{result:?}");
    assert_eq!(lost, vec![assets.join("shot.png")]);
}

#[test]
fn a_folder_inside_the_root_is_made_part_by_part() {
    let dir = TempDir::new().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    let made = make_folder_within(&root, &root.join("media").join("shots")).unwrap();
    assert_eq!(made, root.join("media").join("shots"));
    assert!(made.is_dir());
    assert_eq!(make_folder_within(&root, &root).unwrap(), root);
}

#[test]
fn a_folder_not_under_the_root_is_refused() {
    let dir = TempDir::new().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap().join("note");
    fs::create_dir_all(&root).unwrap();
    let elsewhere = root.parent().unwrap().join("elsewhere");
    assert!(matches!(
        make_folder_within(&root, &elsewhere),
        Err(ImageSaveError::OutsideRoot)
    ));
    assert!(!elsewhere.exists());
}

#[cfg(unix)]
#[test]
fn a_linked_folder_that_resolves_outside_the_root_is_refused() {
    let dir = TempDir::new().unwrap();
    let base = fs::canonicalize(dir.path()).unwrap();
    let root = base.join("note");
    let outside = base.join("outside");
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("assets")).unwrap();
    let nested = make_folder_within(&root, &root.join("assets").join("shots"));
    assert!(
        matches!(nested, Err(ImageSaveError::OutsideRoot)),
        "{nested:?}"
    );
    assert!(!outside.join("shots").exists());
    assert!(matches!(
        make_folder_within(&root, &root.join("assets")),
        Err(ImageSaveError::OutsideRoot)
    ));
}

#[cfg(unix)]
#[test]
fn a_linked_folder_that_stays_inside_the_root_is_used_where_it_resolves() {
    let dir = TempDir::new().unwrap();
    let root = fs::canonicalize(dir.path()).unwrap();
    fs::create_dir_all(root.join("media")).unwrap();
    std::os::unix::fs::symlink(root.join("media"), root.join("assets")).unwrap();
    assert_eq!(
        make_folder_within(&root, &root.join("assets")).unwrap(),
        root.join("media")
    );
}

#[test]
fn a_file_where_the_folder_should_be_is_not_writable() {
    let (dir, _) = folder();
    let blocked = dir.path().join("assets");
    fs::write(&blocked, b"file").unwrap();
    let err = save_image(&blocked, "shot.png", PNG_A, no_stamp, no_stamp).unwrap_err();
    assert!(
        matches!(
            err,
            ImageSaveError::FolderNotWritable | ImageSaveError::Io(_)
        ),
        "{err:?}"
    );
}

#[cfg(unix)]
#[test]
fn a_folder_without_write_permission_is_named() {
    use std::os::unix::fs::PermissionsExt;
    let (_dir, assets) = folder();
    fs::create_dir_all(&assets).unwrap();
    fs::set_permissions(&assets, fs::Permissions::from_mode(0o555)).unwrap();
    let result = save_image(&assets, "shot.png", PNG_A, no_stamp, no_stamp);
    fs::set_permissions(&assets, fs::Permissions::from_mode(0o755)).unwrap();
    if running_as_root() {
        return;
    }
    assert!(matches!(result, Err(ImageSaveError::FolderNotWritable)));
}

/// Root ignores the permission bits this test relies on.
#[cfg(unix)]
fn running_as_root() -> bool {
    // SAFETY: `geteuid` reads the calling process's effective user id and
    // touches nothing.
    unsafe { libc::geteuid() == 0 }
}
