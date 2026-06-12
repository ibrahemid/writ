use std::path::PathBuf;

#[cfg(unix)]
fn create_fake_sidecar(dir: &tempfile::TempDir) -> PathBuf {
    let bin = dir.path().join("writ-fake");
    std::fs::write(&bin, "#!/bin/sh\necho writ").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    bin
}

#[cfg(unix)]
#[test]
fn symlink_creates_and_points_to_sidecar() {
    let src_dir = tempfile::TempDir::new().unwrap();
    let dest_dir = tempfile::TempDir::new().unwrap();

    let sidecar = create_fake_sidecar(&src_dir);
    let symlink_path = dest_dir.path().join("writ");

    std::os::unix::fs::symlink(&sidecar, &symlink_path).unwrap();

    assert!(symlink_path.symlink_metadata().is_ok(), "symlink exists");
    let target = std::fs::read_link(&symlink_path).unwrap();
    assert_eq!(target, sidecar);
}

#[cfg(unix)]
#[test]
fn symlink_replace_removes_existing_first() {
    let src_dir = tempfile::TempDir::new().unwrap();
    let dest_dir = tempfile::TempDir::new().unwrap();

    let old_target = src_dir.path().join("writ-old");
    std::fs::write(&old_target, "old").unwrap();
    let new_target = create_fake_sidecar(&src_dir);
    let symlink_path = dest_dir.path().join("writ");

    std::os::unix::fs::symlink(&old_target, &symlink_path).unwrap();
    assert!(symlink_path.symlink_metadata().is_ok());

    std::fs::remove_file(&symlink_path).unwrap();
    std::os::unix::fs::symlink(&new_target, &symlink_path).unwrap();

    let target = std::fs::read_link(&symlink_path).unwrap();
    assert_eq!(target, new_target, "symlink should point to new target after replace");
}
