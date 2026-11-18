//! The data-folder rules of ADR-059 decision 3, over a described home folder.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use jotra_core::data_folder::{
    data_folders_on_machine, index_in_folder, resolve_data_folder, DataFolder, DataFolderError,
    DataFolderOrigin, DataFolderSources, Lineage, DATA_FOLDER, INDEX_FILE,
};

const HOME: &str = "/home/u";

fn home() -> PathBuf {
    PathBuf::from(HOME)
}

fn on_disk(paths: &[&str]) -> HashSet<PathBuf> {
    paths.iter().map(PathBuf::from).collect()
}

fn resolve_in_home(disk: &HashSet<PathBuf>) -> DataFolder {
    let home = home();
    resolve_data_folder(
        DataFolderSources {
            variable: None,
            home: Some(&home),
        },
        |path| disk.contains(path),
    )
    .expect("a home folder")
}

fn current_home() -> DataFolder {
    DataFolder {
        folder: home().join(".jotra"),
        index: home().join(".jotra/jotra.db"),
        lineage: Lineage::Current,
        origin: DataFolderOrigin::Home,
        notes_default: home().join("Jotra"),
    }
}

fn legacy_home() -> DataFolder {
    DataFolder {
        folder: home().join(".writ"),
        index: home().join(".writ/writ.db"),
        lineage: Lineage::Legacy,
        origin: DataFolderOrigin::Home,
        notes_default: home().join("Writ"),
    }
}

#[test]
fn the_home_folder_table() {
    let rows: [(&str, &[&str], DataFolder); 9] = [
        ("fresh home", &[], current_home()),
        (
            "fresh home with an unrelated Writ folder",
            &["/home/u/Writ"],
            current_home(),
        ),
        (
            "an install from before the rename",
            &["/home/u/.writ", "/home/u/.writ/writ.db", "/home/u/Writ"],
            legacy_home(),
        ),
        (
            "an install from after the rename",
            &["/home/u/.jotra", "/home/u/.jotra/jotra.db"],
            current_home(),
        ),
        (
            "both indexes",
            &[
                "/home/u/.jotra",
                "/home/u/.jotra/jotra.db",
                "/home/u/.writ",
                "/home/u/.writ/writ.db",
            ],
            current_home(),
        ),
        (
            "an old index beside an empty new folder",
            &["/home/u/.jotra", "/home/u/.writ", "/home/u/.writ/writ.db"],
            legacy_home(),
        ),
        (
            "an old folder whose index was deleted for a rebuild",
            &["/home/u/.writ"],
            legacy_home(),
        ),
        (
            "both folders without an index",
            &["/home/u/.jotra", "/home/u/.writ"],
            current_home(),
        ),
        (
            "a new folder whose index was deleted",
            &["/home/u/.jotra"],
            current_home(),
        ),
    ];

    for (name, paths, expected) in rows {
        assert_eq!(resolve_in_home(&on_disk(paths)), expected, "{name}");
    }
}

fn resolve_variable(folder: &str, disk: &HashSet<PathBuf>) -> DataFolder {
    let home = home();
    resolve_data_folder(
        DataFolderSources {
            variable: Some(Path::new(folder)),
            home: Some(&home),
        },
        |path| disk.contains(path),
    )
    .expect("a variable")
}

#[test]
fn the_variable_table() {
    let rows: [(&str, &[&str], &str, &str); 5] = [
        ("an empty folder", &[], "/data/jotra.db", "/data/Jotra"),
        (
            "a folder from before the rename",
            &["/data/writ.db", "/data/Writ"],
            "/data/writ.db",
            "/data/Writ",
        ),
        (
            "a folder with both indexes",
            &["/data/jotra.db", "/data/writ.db"],
            "/data/jotra.db",
            "/data/Jotra",
        ),
        (
            "an old notes folder beside a new index",
            &["/data/jotra.db", "/data/Writ"],
            "/data/jotra.db",
            "/data/Writ",
        ),
        (
            "both notes folders",
            &["/data/jotra.db", "/data/Jotra", "/data/Writ"],
            "/data/jotra.db",
            "/data/Writ",
        ),
    ];

    for (name, paths, index, notes) in rows {
        let resolved = resolve_variable("/data", &on_disk(paths));
        assert_eq!(resolved.folder, PathBuf::from("/data"), "{name}");
        assert_eq!(resolved.index, PathBuf::from(index), "{name}");
        assert_eq!(resolved.notes_default, PathBuf::from(notes), "{name}");
        assert_eq!(resolved.lineage, Lineage::Current, "{name}");
        assert_eq!(resolved.origin, DataFolderOrigin::Variable, "{name}");
        assert!(resolved.is_override(), "{name}");
    }
}

#[test]
fn a_variable_wins_over_an_install_from_before_the_rename() {
    let disk = on_disk(&["/home/u/.writ", "/home/u/.writ/writ.db"]);
    let resolved = resolve_variable("/data", &disk);
    assert_eq!(resolved.folder, PathBuf::from("/data"));
    assert_eq!(resolved.index, PathBuf::from("/data/jotra.db"));
}

#[test]
fn a_home_folder_is_not_an_override() {
    assert!(!resolve_in_home(&HashSet::new()).is_override());
    assert!(!resolve_in_home(&on_disk(&["/home/u/.writ"])).is_override());
}

#[test]
fn no_home_and_no_variable_is_an_error() {
    let resolved = resolve_data_folder(DataFolderSources::default(), |_| true);
    assert_eq!(resolved, Err(DataFolderError::NoHome));
}

#[test]
fn a_variable_needs_no_home() {
    let resolved = resolve_data_folder(
        DataFolderSources {
            variable: Some(Path::new("/data")),
            home: None,
        },
        |_| false,
    )
    .expect("a variable");
    assert_eq!(resolved.index, PathBuf::from("/data/jotra.db"));
}

#[test]
fn the_index_prefers_the_new_name() {
    let folder = Path::new("/data");
    let disk = on_disk(&["/data/jotra.db", "/data/writ.db"]);
    assert_eq!(
        index_in_folder(folder, |path| disk.contains(path)),
        folder.join(INDEX_FILE)
    );
    let disk = on_disk(&["/data/writ.db"]);
    assert_eq!(
        index_in_folder(folder, |path| disk.contains(path)),
        folder.join("writ.db")
    );
    assert_eq!(index_in_folder(folder, |_| false), folder.join(INDEX_FILE));
}

#[test]
fn every_data_folder_on_the_machine_is_listed_once() {
    assert_eq!(
        data_folders_on_machine(&home().join(DATA_FOLDER), Some(&home())),
        vec![home().join(".jotra"), home().join(".writ")]
    );
    assert_eq!(
        data_folders_on_machine(&home().join(".writ"), Some(&home())),
        vec![home().join(".writ"), home().join(".jotra")]
    );
    assert_eq!(
        data_folders_on_machine(Path::new("/data"), Some(&home())),
        vec![
            PathBuf::from("/data"),
            home().join(".jotra"),
            home().join(".writ")
        ]
    );
    assert_eq!(
        data_folders_on_machine(Path::new("/data"), None),
        vec![PathBuf::from("/data")]
    );
}
