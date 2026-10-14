//! One folder's listing as the sidebar's tree asks for it:
//! `workspace_store::list_dir` on a folder of tens of thousands of entries, and
//! on a folder the process may not read.

use tempfile::TempDir;
use writ_storage::workspace_store;

#[test]
fn a_folder_of_twenty_thousand_entries_lists_whole_and_in_order() {
    let root = TempDir::new().expect("tempdir");
    let folder = root.path().join("big");
    std::fs::create_dir(&folder).expect("folder");
    for index in 0..20_000 {
        std::fs::write(folder.join(format!("file-{index:05}.txt")), "").expect("write");
    }
    for index in 0..10 {
        std::fs::create_dir(folder.join(format!("dir-{index}"))).expect("dir");
    }

    let listing = workspace_store::list_dir(root.path(), &folder).expect("listing");

    assert_eq!(listing.entries.len(), 20_010);
    assert_eq!(listing.unreadable, 0);
    assert!(listing.entries[..10].iter().all(|entry| entry.is_dir));
    assert_eq!(listing.entries[10].name, "file-00000.txt");
    assert_eq!(listing.entries[20_009].name, "file-19999.txt");
}

#[cfg(unix)]
#[test]
fn a_folder_the_process_may_not_read_is_an_error_and_its_parent_still_lists_it() {
    use std::os::unix::fs::PermissionsExt;

    let root = TempDir::new().expect("tempdir");
    let locked = root.path().join("locked");
    std::fs::create_dir(&locked).expect("folder");
    std::fs::write(locked.join("inside.md"), "").expect("write");
    std::fs::write(root.path().join("open.md"), "").expect("write");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).expect("chmod");

    // A process allowed to read anything (root in a container) reads the
    // folder regardless, and there is nothing to assert about a refusal.
    let refused = std::fs::read_dir(&locked).is_err();
    let result = workspace_store::list_dir(root.path(), &locked);
    let parent = workspace_store::list_dir(root.path(), root.path());
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).expect("chmod back");

    if refused {
        assert!(
            result.is_err(),
            "an unreadable folder must fail its listing"
        );
    }
    let parent = parent.expect("the parent lists");
    let names: Vec<&str> = parent.entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["locked", "open.md"]);
    assert_eq!(parent.unreadable, 0);
}
