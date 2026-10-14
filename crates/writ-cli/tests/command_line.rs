//! The file-opening path's exits, run through the binary. Only invocations
//! that never reach the app run on every platform: on macOS any launch would
//! open the installed Writ.

use std::io::Write;
use std::process::{Command, Output, Stdio};

use tempfile::TempDir;

fn writ(dir: &TempDir) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_writ"));
    command
        .env_remove("WRIT_GUI_BIN")
        .env("WRIT_DATA_DIR", dir.path().join("data"))
        .env("WRIT_NOTES_DIR", dir.path().join("notes"))
        .current_dir(dir.path());
    command
}

fn run_with_stdin(mut command: Command, input: &[u8]) -> Output {
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("run writ");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(input)
        .expect("write stdin");
    child.wait_with_output().expect("wait for writ")
}

fn files_in(dir: &std::path::Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn two_folders_at_once_is_a_usage_error() {
    let dir = TempDir::new().expect("tempdir");
    std::fs::create_dir_all(dir.path().join("a")).expect("a");
    std::fs::create_dir_all(dir.path().join("b")).expect("b");
    let output = writ(&dir)
        .args(["a", "b"])
        .stdin(Stdio::null())
        .output()
        .expect("run writ");
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn a_file_and_a_folder_together_is_a_usage_error() {
    let dir = TempDir::new().expect("tempdir");
    std::fs::create_dir_all(dir.path().join("a")).expect("a");
    std::fs::write(dir.path().join("x.txt"), "").expect("x");
    let output = writ(&dir)
        .args(["x.txt", "a"])
        .stdin(Stdio::null())
        .output()
        .expect("run writ");
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn a_dash_with_nothing_piped_is_a_usage_error() {
    let dir = TempDir::new().expect("tempdir");
    let output = writ(&dir)
        .arg("-")
        .stdin(Stdio::null())
        .output()
        .expect("run writ");
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("nothing is piped"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn an_empty_pipe_writes_nothing_and_opens_nothing() {
    let dir = TempDir::new().expect("tempdir");
    let output = run_with_stdin(writ(&dir), b"  \n");
    assert_eq!(output.status.code(), Some(0));
    assert!(files_in(&dir.path().join("notes")).is_empty());
}

#[test]
fn the_binary_carries_the_signature_the_install_looks_for() {
    let bytes = std::fs::read(env!("CARGO_BIN_EXE_writ")).expect("read the binary");
    assert!(writ_core::cli_install::carries_cli_signature(&bytes));
}

/// Off macOS the app is resolved by binary, so a stub stands in for it.
#[cfg(all(unix, not(target_os = "macos")))]
mod with_a_stub_app {
    use super::*;

    #[test]
    fn stdin_that_is_not_a_pipe_launches_the_app() {
        let dir = TempDir::new().expect("tempdir");
        let output = writ(&dir)
            .env("WRIT_GUI_BIN", "/bin/true")
            .stdin(Stdio::null())
            .output()
            .expect("run writ");
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        assert!(files_in(&dir.path().join("notes")).is_empty());
    }

    #[test]
    fn piped_text_with_no_title_gets_a_minted_name() {
        let dir = TempDir::new().expect("tempdir");
        let mut command = writ(&dir);
        command.env("WRIT_GUI_BIN", "/bin/true");
        let output = run_with_stdin(command, b"test log\n");
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        let names = files_in(&dir.path().join("notes"));
        assert_eq!(names.len(), 1, "{names:?}");
        assert!(names[0].starts_with("writ-"), "{names:?}");
    }

    #[test]
    fn a_title_beyond_ascii_names_the_file() {
        let dir = TempDir::new().expect("tempdir");
        let mut command = writ(&dir);
        command
            .env("WRIT_GUI_BIN", "/bin/true")
            .args(["--title", "ملاحظات"]);
        let output = run_with_stdin(command, "نص\n".as_bytes());
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        assert_eq!(
            files_in(&dir.path().join("notes")),
            vec!["ملاحظات.txt".to_string()]
        );
    }
}
