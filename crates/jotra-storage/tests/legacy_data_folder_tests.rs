//! The data folder found on a real home folder (ADR-059 decision 3).

use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use jotra_core::data_folder::{DataFolderOrigin, DataFolderSources, Lineage};
use jotra_core::user_variables::{read_user_variable, UserVariable};
use jotra_storage::data_folder::{locate_data_folder, locate_index};

fn lay_out(root: &Path, dirs: &[&str], files: &[&str]) {
    for dir in dirs {
        fs::create_dir_all(root.join(dir)).expect("dir");
    }
    for file in files {
        let path = root.join(file);
        fs::create_dir_all(path.parent().expect("parent")).expect("dir");
        fs::write(path, b"").expect("file");
    }
}

fn tree(root: &Path) -> BTreeSet<PathBuf> {
    let mut found = BTreeSet::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir).expect("read dir") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                pending.push(path.clone());
            }
            found.insert(path);
        }
    }
    found
}

struct HomeRow {
    name: &'static str,
    dirs: &'static [&'static str],
    files: &'static [&'static str],
    folder: &'static str,
    index: &'static str,
    notes: &'static str,
    lineage: Lineage,
}

#[test]
fn the_home_folder_table_on_disk() {
    let rows = [
        HomeRow {
            name: "fresh home",
            dirs: &[],
            files: &[],
            folder: ".jotra",
            index: ".jotra/jotra.db",
            notes: "Jotra",
            lineage: Lineage::Current,
        },
        HomeRow {
            name: "fresh home with an unrelated Writ folder",
            dirs: &["Writ"],
            files: &["Writ/groceries.txt"],
            folder: ".jotra",
            index: ".jotra/jotra.db",
            notes: "Jotra",
            lineage: Lineage::Current,
        },
        HomeRow {
            name: "an install from before the rename",
            dirs: &["Writ"],
            files: &[".writ/writ.db", ".writ/config.toml"],
            folder: ".writ",
            index: ".writ/writ.db",
            notes: "Writ",
            lineage: Lineage::Legacy,
        },
        HomeRow {
            name: "both indexes",
            dirs: &[],
            files: &[".jotra/jotra.db", ".writ/writ.db"],
            folder: ".jotra",
            index: ".jotra/jotra.db",
            notes: "Jotra",
            lineage: Lineage::Current,
        },
        HomeRow {
            name: "an old folder whose index was deleted for a rebuild",
            dirs: &[".writ"],
            files: &[".writ/config.toml"],
            folder: ".writ",
            index: ".writ/writ.db",
            notes: "Writ",
            lineage: Lineage::Legacy,
        },
        HomeRow {
            name: "an old index beside an empty new folder",
            dirs: &[".jotra"],
            files: &[".writ/writ.db"],
            folder: ".writ",
            index: ".writ/writ.db",
            notes: "Writ",
            lineage: Lineage::Legacy,
        },
    ];

    for row in rows {
        let home = tempfile::tempdir().expect("home");
        lay_out(home.path(), row.dirs, row.files);
        let before = tree(home.path());

        let resolved = locate_data_folder(DataFolderSources {
            variable: None,
            home: Some(home.path()),
        })
        .expect("a home folder");

        assert_eq!(
            resolved.folder,
            home.path().join(row.folder),
            "{}",
            row.name
        );
        assert_eq!(resolved.index, home.path().join(row.index), "{}", row.name);
        assert_eq!(
            resolved.notes_default,
            home.path().join(row.notes),
            "{}",
            row.name
        );
        assert_eq!(resolved.lineage, row.lineage, "{}", row.name);
        assert_eq!(resolved.origin, DataFolderOrigin::Home, "{}", row.name);
        assert_eq!(tree(home.path()), before, "{} changed the disk", row.name);
    }
}

struct VariableRow {
    name: &'static str,
    files: &'static [&'static str],
    dirs: &'static [&'static str],
    index: &'static str,
    notes: &'static str,
}

#[test]
fn the_variable_table_on_disk() {
    let rows = [
        VariableRow {
            name: "an empty folder",
            files: &[],
            dirs: &[],
            index: "jotra.db",
            notes: "Jotra",
        },
        VariableRow {
            name: "a folder from before the rename",
            files: &["writ.db"],
            dirs: &[],
            index: "writ.db",
            notes: "Jotra",
        },
        VariableRow {
            name: "a folder with both indexes",
            files: &["jotra.db", "writ.db"],
            dirs: &[],
            index: "jotra.db",
            notes: "Jotra",
        },
        VariableRow {
            name: "a folder holding its notes under the old name",
            files: &["writ.db"],
            dirs: &["Writ"],
            index: "writ.db",
            notes: "Writ",
        },
    ];

    for row in rows {
        let data = tempfile::tempdir().expect("data");
        lay_out(data.path(), row.dirs, row.files);
        let before = tree(data.path());

        let resolved = locate_data_folder(DataFolderSources {
            variable: Some(data.path()),
            home: None,
        })
        .expect("a variable");

        assert_eq!(resolved.folder, data.path(), "{}", row.name);
        assert_eq!(resolved.index, data.path().join(row.index), "{}", row.name);
        assert_eq!(
            resolved.notes_default,
            data.path().join(row.notes),
            "{}",
            row.name
        );
        assert_eq!(resolved.lineage, Lineage::Current, "{}", row.name);
        assert!(resolved.is_override(), "{}", row.name);
        assert_eq!(tree(data.path()), before, "{} changed the disk", row.name);
    }
}

#[test]
fn the_old_variable_name_points_the_resolver_at_its_folder() {
    let home = tempfile::tempdir().expect("home");
    lay_out(home.path(), &[], &[".writ/writ.db", "dev/writ.db"]);
    let dev = home.path().join("dev");
    let variable = read_user_variable(UserVariable::DataDir, |name| {
        (name == "WRIT_DATA_DIR").then(|| OsString::from(dev.as_os_str()))
    })
    .map(PathBuf::from);

    let resolved = locate_data_folder(DataFolderSources {
        variable: variable.as_deref(),
        home: Some(home.path()),
    })
    .expect("a variable");

    assert_eq!(resolved.folder, dev);
    assert_eq!(resolved.index, dev.join("writ.db"));
    assert!(resolved.is_override());
}

#[test]
fn the_index_of_a_folder_on_disk() {
    let data = tempfile::tempdir().expect("data");
    assert_eq!(locate_index(data.path()), data.path().join("jotra.db"));

    lay_out(data.path(), &[], &["writ.db"]);
    assert_eq!(locate_index(data.path()), data.path().join("writ.db"));

    lay_out(data.path(), &[], &["jotra.db"]);
    assert_eq!(locate_index(data.path()), data.path().join("jotra.db"));
}
