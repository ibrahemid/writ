//! Surfaces a startup failure that happens before any window exists.
//!
//! Release builds abort on panic, the window starts hidden, and the dialog
//! plugin is only registered once the Tauri builder runs, so a failure in
//! [`crate::state::AppState::initialize`] would otherwise end the process
//! with nothing on screen and nothing on disk. This module writes the
//! report, shows a native dialog, and exits.
//!
//! Policy (what the report says, where it goes) lives in
//! [`writ_core::startup`]; this module performs the writes and calls the
//! platform dialog.

use std::path::{Path, PathBuf};
use std::process::ExitStatus;

use writ_core::startup::{
    choose_report_path, format_failure_dialog, format_failure_report, StartupFailure,
    FAILURE_DIALOG_TITLE,
};

/// Timestamp used for the report and its file name.
pub fn timestamp() -> String {
    chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string()
}

/// Writes the report, shows a blocking dialog, and exits nonzero.
///
/// `logs_dir` is the preferred destination; pass `None` when the data
/// directory itself could not be resolved.
pub fn abort_with_report(failure: &StartupFailure, logs_dir: Option<&Path>) -> ! {
    let report_path = write_report(failure, logs_dir, &std::env::temp_dir());

    // A desktop launch discards this, but a terminal launch does not.
    eprintln!("{}", format_failure_report(failure, report_path.as_deref()));

    show_failure_dialog(
        FAILURE_DIALOG_TITLE,
        &format_failure_dialog(failure, report_path.as_deref()),
    );

    std::process::exit(1);
}

/// Writes the report to the logs directory, falling back to `temp_dir` when
/// that write fails. Returns where it landed, or `None` if neither took it.
fn write_report(
    failure: &StartupFailure,
    logs_dir: Option<&Path>,
    temp_dir: &Path,
) -> Option<PathBuf> {
    write_report_with(failure, logs_dir, temp_dir, |target, report| {
        std::fs::write(target, report)
    })
}

/// Tries each candidate location in turn and returns the first that took the
/// report.
///
/// The attempt is the test. A probe that succeeds proves a directory entry
/// can be created, not that the report's bytes will fit, so probing and then
/// writing once strands the report on a full disk even though the fallback
/// location would have taken it. Each attempt re-renders the report so its
/// `Report file:` line names the path it actually lands on.
fn write_report_with(
    failure: &StartupFailure,
    logs_dir: Option<&Path>,
    temp_dir: &Path,
    mut write: impl FnMut(&Path, &str) -> std::io::Result<()>,
) -> Option<PathBuf> {
    let file_name = failure.report_file_name();
    let fallback = choose_report_path(None, temp_dir, &file_name);
    let mut candidates = Vec::with_capacity(2);
    if let Some(dir) = logs_dir {
        candidates.push(choose_report_path(Some(dir), temp_dir, &file_name));
    }
    if !candidates.contains(&fallback) {
        candidates.push(fallback);
    }

    for target in candidates {
        let parent_ready = target
            .parent()
            .is_none_or(|dir| std::fs::create_dir_all(dir).is_ok());
        if !parent_ready {
            continue;
        }
        let report = format_failure_report(failure, Some(target.as_path()));
        if write(&target, &report).is_ok() {
            return Some(target);
        }
    }
    None
}

/// Shows a blocking native error dialog.
///
/// macOS and Windows use `rfd`, whose backends for those targets link no
/// system libraries beyond the ones Writ already uses. The Linux backend
/// would pull GTK in as a build dependency of this crate, so Linux spawns
/// whichever desktop dialog binary exists instead.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn show_failure_dialog(title: &str, body: &str) {
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title(title)
        .set_description(body)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn show_failure_dialog(title: &str, body: &str) {
    show_failure_dialog_via_command(title, body);
}

/// Best-effort dialog for targets with no compiled-in backend: the first of
/// `zenity`, `kdialog`, `xmessage` that shows one wins. Returns `false` when
/// none of them does, leaving the report file and standard error as the
/// record.
///
/// Compiled on every target so it typechecks and lints on this host; only
/// reached where no native backend is linked.
#[cfg_attr(any(target_os = "macos", target_os = "windows"), allow(dead_code))]
fn show_failure_dialog_via_command(title: &str, body: &str) -> bool {
    show_first_working_dialog(&dialog_candidates(title, body), |program, args| {
        std::process::Command::new(program).args(args).status()
    })
}

/// The dialog binaries to try, in order, each with the arguments it takes.
#[cfg_attr(any(target_os = "macos", target_os = "windows"), allow(dead_code))]
fn dialog_candidates(title: &str, body: &str) -> [(&'static str, Vec<String>); 3] {
    let text = format!("{title}\n\n{body}");
    [
        (
            "zenity",
            vec![
                "--error".to_string(),
                "--no-wrap".to_string(),
                format!("--title={title}"),
                format!("--text={text}"),
            ],
        ),
        (
            "kdialog",
            vec![
                "--title".to_string(),
                title.to_string(),
                "--error".to_string(),
                body.to_string(),
            ],
        ),
        ("xmessage", vec!["-center".to_string(), text]),
    ]
}

/// Runs candidates until one of them exits cleanly.
///
/// Spawning is not showing: zenity against a broken GTK or a dead `DISPLAY`
/// starts and then exits nonzero, so a chain that stops at the first spawn it
/// manages never reaches the tool that would have worked.
#[cfg_attr(any(target_os = "macos", target_os = "windows"), allow(dead_code))]
fn show_first_working_dialog(
    candidates: &[(&str, Vec<String>)],
    mut run: impl FnMut(&str, &[String]) -> std::io::Result<ExitStatus>,
) -> bool {
    candidates
        .iter()
        .any(|(program, args)| run(program, args).is_ok_and(|status| status.success()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use writ_core::startup::StartupStage;

    fn failure() -> StartupFailure {
        StartupFailure::new(
            StartupStage::AppState,
            "Permission denied (os error 13)",
            Some(PathBuf::from("/home/user/.writ")),
            "20260730-101500",
        )
    }

    #[test]
    fn report_lands_in_the_logs_dir_when_it_is_writable() {
        let logs = tempfile::tempdir().unwrap();
        let temp = tempfile::tempdir().unwrap();

        let written = write_report(&failure(), Some(logs.path()), temp.path()).unwrap();

        assert_eq!(written, logs.path().join("writ-crash-20260730-101500.txt"));
        assert!(std::fs::read_to_string(&written)
            .unwrap()
            .contains("Error: Permission denied (os error 13)"));
    }

    #[test]
    fn report_names_its_own_location() {
        let logs = tempfile::tempdir().unwrap();
        let temp = tempfile::tempdir().unwrap();

        let written = write_report(&failure(), Some(logs.path()), temp.path()).unwrap();

        let contents = std::fs::read_to_string(&written).unwrap();
        assert!(contents.contains(&format!("Report file: {}", written.display())));
    }

    #[test]
    fn report_falls_back_to_the_temp_dir_when_there_is_no_logs_dir() {
        let temp = tempfile::tempdir().unwrap();

        let written = write_report(&failure(), None, temp.path()).unwrap();

        assert_eq!(written, temp.path().join("writ-crash-20260730-101500.txt"));
        assert!(written.exists());
    }

    #[test]
    #[cfg(unix)]
    fn report_falls_back_to_the_temp_dir_when_the_logs_dir_is_unwritable() {
        use std::os::unix::fs::PermissionsExt;

        let parent = tempfile::tempdir().unwrap();
        let logs = parent.path().join("logs");
        std::fs::create_dir(&logs).unwrap();
        std::fs::set_permissions(&logs, std::fs::Permissions::from_mode(0o555)).unwrap();
        let temp = tempfile::tempdir().unwrap();

        let written = write_report(&failure(), Some(&logs), temp.path()).unwrap();

        assert_eq!(written, temp.path().join("writ-crash-20260730-101500.txt"));
        std::fs::set_permissions(&logs, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn report_creates_a_logs_dir_that_does_not_exist_yet() {
        let parent = tempfile::tempdir().unwrap();
        let logs = parent.path().join("logs");
        let temp = tempfile::tempdir().unwrap();

        let written = write_report(&failure(), Some(&logs), temp.path()).unwrap();

        assert_eq!(written, logs.join("writ-crash-20260730-101500.txt"));
        assert!(written.exists());
    }

    #[test]
    fn report_falls_back_to_the_temp_dir_when_the_logs_dir_write_fails() {
        let logs = tempfile::tempdir().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let logs_target = logs.path().join("writ-crash-20260730-101500.txt");

        let written = write_report_with(
            &failure(),
            Some(logs.path()),
            temp.path(),
            |target, report| {
                if target == logs_target {
                    return Err(std::io::Error::other("No space left on device"));
                }
                std::fs::write(target, report)
            },
        )
        .unwrap();

        assert_eq!(written, temp.path().join("writ-crash-20260730-101500.txt"));
        let contents = std::fs::read_to_string(&written).unwrap();
        assert!(contents.contains(&format!("Report file: {}", written.display())));
        assert!(!contents.contains(&logs.path().display().to_string()));
        assert!(!logs_target.exists());
    }

    #[test]
    fn no_report_path_when_every_location_refuses_the_write() {
        let logs = tempfile::tempdir().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let mut attempted = Vec::new();

        let written = write_report_with(&failure(), Some(logs.path()), temp.path(), |target, _| {
            attempted.push(target.to_path_buf());
            Err(std::io::Error::other("No space left on device"))
        });

        assert!(written.is_none());
        assert_eq!(
            attempted,
            [
                logs.path().join("writ-crash-20260730-101500.txt"),
                temp.path().join("writ-crash-20260730-101500.txt"),
            ]
        );
    }

    #[test]
    #[cfg(unix)]
    fn dialog_chain_moves_past_a_tool_that_exits_nonzero() {
        use std::os::unix::process::ExitStatusExt;

        let candidates = dialog_candidates(FAILURE_DIALOG_TITLE, "Body");
        let mut attempted = Vec::new();

        let shown = show_first_working_dialog(&candidates, |program, _| {
            attempted.push(program.to_string());
            let code = if program == "kdialog" { 0 } else { 1 << 8 };
            Ok(ExitStatus::from_raw(code))
        });

        assert!(shown);
        assert_eq!(attempted, ["zenity", "kdialog"]);
    }

    #[test]
    #[cfg(unix)]
    fn dialog_chain_reports_nothing_shown_when_no_tool_works() {
        use std::os::unix::process::ExitStatusExt;

        let candidates = dialog_candidates(FAILURE_DIALOG_TITLE, "Body");
        let mut attempted = Vec::new();

        let shown = show_first_working_dialog(&candidates, |program, _| {
            attempted.push(program.to_string());
            match program {
                "zenity" => Ok(ExitStatus::from_raw(1 << 8)),
                _ => Err(std::io::Error::other("No such file or directory")),
            }
        });

        assert!(!shown);
        assert_eq!(attempted, ["zenity", "kdialog", "xmessage"]);
    }
}
