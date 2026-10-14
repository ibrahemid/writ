use std::ffi::OsString;
use writ_core::file_ops::arg_paths_from_iter;
use writ_core::startup::{is_login_launch, LOGIN_LAUNCH_ARG};

#[test]
fn the_login_item_launch_is_recognised() {
    assert!(is_login_launch(["--login"]));
    assert!(is_login_launch([
        OsString::from("/tmp/a.txt"),
        OsString::from(LOGIN_LAUNCH_ARG)
    ]));
}

#[test]
fn an_ordinary_launch_is_not_a_login_launch() {
    assert!(!is_login_launch(Vec::<OsString>::new()));
    assert!(!is_login_launch(["/tmp/a.txt"]));
    assert!(!is_login_launch(["--login=1", "login"]));
}

#[test]
fn the_login_argument_is_never_read_as_a_file() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::write(dir.path().join(LOGIN_LAUNCH_ARG), "not a note").unwrap();
    let cwd = std::env::current_dir().unwrap();
    std::env::set_current_dir(dir.path()).unwrap();
    let paths = arg_paths_from_iter([OsString::from(LOGIN_LAUNCH_ARG)]);
    std::env::set_current_dir(cwd).unwrap();
    assert!(paths.is_empty());
}
