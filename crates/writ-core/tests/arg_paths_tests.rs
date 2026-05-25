use std::ffi::OsString;
use std::path::PathBuf;
use writ_core::file_ops::arg_paths_from_iter;

fn os(s: &str) -> OsString {
    OsString::from(s)
}

#[test]
fn returns_empty_for_empty_iter() {
    let paths: Vec<PathBuf> = arg_paths_from_iter(Vec::<OsString>::new());
    assert!(paths.is_empty());
}

#[test]
fn keeps_existing_real_files() {
    let dir = tempfile::TempDir::new().unwrap();
    let a = dir.path().join("a.txt");
    let b = dir.path().join("b.md");
    std::fs::write(&a, "alpha").unwrap();
    std::fs::write(&b, "beta").unwrap();

    let iter = vec![OsString::from(&a), OsString::from(&b)];
    let paths = arg_paths_from_iter(iter);

    assert_eq!(paths.len(), 2);
    assert!(paths.iter().any(|p| p.ends_with("a.txt")));
    assert!(paths.iter().any(|p| p.ends_with("b.md")));
}

#[test]
fn drops_non_existent_paths() {
    let dir = tempfile::TempDir::new().unwrap();
    let real = dir.path().join("real.txt");
    std::fs::write(&real, "x").unwrap();
    let bogus = dir.path().join("definitely-not-there.txt");

    let iter = vec![OsString::from(&bogus), OsString::from(&real)];
    let paths = arg_paths_from_iter(iter);

    assert_eq!(paths.len(), 1);
    assert!(paths[0].ends_with("real.txt"));
}

#[test]
fn drops_flag_arguments() {
    let dir = tempfile::TempDir::new().unwrap();
    let real = dir.path().join("real.txt");
    std::fs::write(&real, "x").unwrap();

    let iter = vec![
        os("--foo"),
        os("--verbose"),
        os("-h"),
        OsString::from(&real),
    ];
    let paths = arg_paths_from_iter(iter);

    assert_eq!(paths.len(), 1);
    assert!(paths[0].ends_with("real.txt"));
}

#[test]
fn drops_directories() {
    let dir = tempfile::TempDir::new().unwrap();
    let f = dir.path().join("file.txt");
    std::fs::write(&f, "x").unwrap();

    let iter = vec![OsString::from(dir.path()), OsString::from(&f)];
    let paths = arg_paths_from_iter(iter);

    assert_eq!(paths.len(), 1);
    assert!(paths[0].ends_with("file.txt"));
}

#[test]
fn absolute_inputs_remain_absolute() {
    let dir = tempfile::TempDir::new().unwrap();
    let f = dir.path().join("file.txt");
    std::fs::write(&f, "x").unwrap();

    let iter = vec![OsString::from(&f)];
    let paths = arg_paths_from_iter(iter);

    assert_eq!(paths.len(), 1);
    assert!(paths[0].is_absolute(), "expected absolute, got {:?}", paths[0]);
    assert!(paths[0].ends_with("file.txt"));
}

#[test]
fn preserves_order() {
    let dir = tempfile::TempDir::new().unwrap();
    let a = dir.path().join("a.txt");
    let b = dir.path().join("b.txt");
    let c = dir.path().join("c.txt");
    std::fs::write(&a, "1").unwrap();
    std::fs::write(&b, "2").unwrap();
    std::fs::write(&c, "3").unwrap();

    let iter = vec![
        OsString::from(&b),
        OsString::from(&a),
        OsString::from(&c),
    ];
    let paths = arg_paths_from_iter(iter);

    assert_eq!(paths.len(), 3);
    assert!(paths[0].ends_with("b.txt"));
    assert!(paths[1].ends_with("a.txt"));
    assert!(paths[2].ends_with("c.txt"));
}

#[test]
fn deduplicates_repeated_paths() {
    let dir = tempfile::TempDir::new().unwrap();
    let a = dir.path().join("a.txt");
    std::fs::write(&a, "1").unwrap();

    let iter = vec![
        OsString::from(&a),
        OsString::from(&a),
        OsString::from(&a),
    ];
    let paths = arg_paths_from_iter(iter);

    assert_eq!(paths.len(), 1);
}
