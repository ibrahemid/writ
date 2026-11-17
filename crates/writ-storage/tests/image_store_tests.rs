//! Saving a pasted or dropped image (ADR-035).

use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;
use writ_storage::image_store::{make_folder_within, save_image, ImageSaveError, SavedImage};
use writ_storage::paths::strip_verbatim_prefix;

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

/// The temp folder as `canonicalize` spells it (`\\?\C:\…` on Windows) and as
/// the rest of the app spells it. The two are the same path elsewhere.
fn both_spellings(dir: &TempDir) -> (PathBuf, PathBuf) {
    let canonical = fs::canonicalize(dir.path()).unwrap();
    let plain = strip_verbatim_prefix(canonical.clone());
    (canonical, plain)
}

/// What Windows answers a process that may not make links.
const ERROR_PRIVILEGE_NOT_HELD: i32 = 1314;

/// Links `link` to the folder `original`, or answers `false` when the system
/// does not let this process make links.
fn link_folder(original: &Path, link: &Path) -> bool {
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(original, link);
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_dir(original, link);
    match made {
        Ok(()) => true,
        Err(err) if cfg!(windows) && err.raw_os_error() == Some(ERROR_PRIVILEGE_NOT_HELD) => false,
        Err(err) => panic!("cannot link {}: {err}", link.display()),
    }
}

#[test]
fn a_folder_inside_the_root_is_made_part_by_part() {
    let dir = TempDir::new().unwrap();
    let (root, plain) = both_spellings(&dir);
    let made = make_folder_within(&root, &root.join("media").join("shots")).unwrap();
    assert_eq!(made, plain.join("media").join("shots"));
    assert!(plain.join("media").is_dir());
    assert!(made.is_dir());
    assert_eq!(make_folder_within(&root, &root).unwrap(), plain);
}

#[test]
fn a_root_and_a_folder_in_either_spelling_give_the_folder_without_the_prefix() {
    let dir = TempDir::new().unwrap();
    let (canonical, plain) = both_spellings(&dir);
    let expected = plain.join("media").join("shots");
    for root in [&canonical, &plain] {
        for base in [&canonical, &plain] {
            let folder = base.join("media").join("shots");
            assert_eq!(
                make_folder_within(root, &folder).unwrap(),
                expected,
                "root {root:?}, folder {folder:?}"
            );
        }
    }
    assert!(expected.is_dir());
}

#[cfg(windows)]
#[test]
fn a_verbatim_root_on_windows_gives_the_folder_without_the_prefix() {
    let dir = TempDir::new().unwrap();
    let (canonical, plain) = both_spellings(&dir);
    assert!(
        canonical.to_string_lossy().starts_with(r"\\?\"),
        "{canonical:?}"
    );
    assert!(!plain.to_string_lossy().starts_with(r"\\?\"), "{plain:?}");
    let made = make_folder_within(&canonical, &canonical.join("media")).unwrap();
    assert!(!made.to_string_lossy().starts_with(r"\\?\"), "{made:?}");
    assert_eq!(made, plain.join("media"));
    assert_eq!(
        make_folder_within(&plain, &canonical.join("media")).unwrap(),
        made
    );
    assert_eq!(
        make_folder_within(&canonical, &plain.join("media")).unwrap(),
        made
    );
}

#[test]
fn a_folder_not_under_the_root_is_refused() {
    let dir = TempDir::new().unwrap();
    let (canonical, plain) = both_spellings(&dir);
    fs::create_dir_all(plain.join("note")).unwrap();
    for root in [canonical.join("note"), plain.join("note")] {
        for base in [&canonical, &plain] {
            let elsewhere = base.join("elsewhere");
            let climbing = base.join("note").join("..").join("elsewhere");
            for folder in [&elsewhere, &climbing] {
                let refused = make_folder_within(&root, folder);
                assert!(
                    matches!(refused, Err(ImageSaveError::OutsideRoot)),
                    "root {root:?}, folder {folder:?}: {refused:?}"
                );
            }
            assert!(make_folder_within(&root, &base.join("note").join("media")).is_ok());
        }
    }
    assert!(!plain.join("elsewhere").exists());
}

#[test]
fn a_linked_folder_that_resolves_outside_the_root_is_refused() {
    let dir = TempDir::new().unwrap();
    let (canonical, plain) = both_spellings(&dir);
    let outside = plain.join("outside");
    fs::create_dir_all(plain.join("note")).unwrap();
    fs::create_dir_all(&outside).unwrap();
    if !link_folder(&outside, &plain.join("note").join("assets")) {
        return;
    }
    for base in [&canonical, &plain] {
        let root = base.join("note");
        let nested = make_folder_within(&root, &root.join("assets").join("shots"));
        assert!(
            matches!(nested, Err(ImageSaveError::OutsideRoot)),
            "{nested:?}"
        );
        assert!(matches!(
            make_folder_within(&root, &root.join("assets")),
            Err(ImageSaveError::OutsideRoot)
        ));
        assert!(make_folder_within(&root, &root.join("media")).is_ok());
    }
    assert!(!outside.join("shots").exists());
}

#[test]
fn a_linked_folder_that_stays_inside_the_root_is_used_where_it_resolves() {
    let dir = TempDir::new().unwrap();
    let (canonical, plain) = both_spellings(&dir);
    fs::create_dir_all(plain.join("media")).unwrap();
    if !link_folder(&plain.join("media"), &plain.join("assets")) {
        return;
    }
    for root in [&canonical, &plain] {
        assert_eq!(
            make_folder_within(root, &root.join("assets")).unwrap(),
            plain.join("media")
        );
    }
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
