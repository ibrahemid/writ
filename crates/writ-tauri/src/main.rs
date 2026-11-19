#![cfg_attr(windows, windows_subsystem = "windows")]

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const GUI_BINARY_STEM: &str = "jotra-tauri";

const STATUS_GUI_MISSING: u8 = 2;
const STATUS_OWN_PATH_UNKNOWN: u8 = 3;
const STATUS_GUI_NOT_STARTED: u8 = 4;

fn sibling_gui_path(launcher: &Path) -> Option<PathBuf> {
    let folder = launcher.parent()?;
    Some(folder.join(format!("{GUI_BINARY_STEM}{}", std::env::consts::EXE_SUFFIX)))
}

#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Started,
    OwnPathUnknown(String),
    GuiMissing(PathBuf),
    GuiNotStarted { gui: PathBuf, reason: String },
}

impl Outcome {
    fn status(&self) -> u8 {
        match self {
            Self::Started => 0,
            Self::GuiMissing(_) => STATUS_GUI_MISSING,
            Self::OwnPathUnknown(_) => STATUS_OWN_PATH_UNKNOWN,
            Self::GuiNotStarted { .. } => STATUS_GUI_NOT_STARTED,
        }
    }

    fn message(&self) -> Option<String> {
        match self {
            Self::Started => None,
            Self::OwnPathUnknown(reason) => {
                Some(format!("Cannot read this program's own path: {reason}"))
            }
            Self::GuiMissing(gui) => Some(format!("{} is missing.", gui.display())),
            Self::GuiNotStarted { gui, reason } => {
                Some(format!("Cannot start {}: {reason}", gui.display()))
            }
        }
    }
}

fn launch(
    launcher: io::Result<PathBuf>,
    args: Vec<OsString>,
    is_file: impl Fn(&Path) -> bool,
    start: impl FnOnce(&Path, Vec<OsString>) -> io::Result<()>,
) -> Outcome {
    let launcher = match launcher {
        Ok(path) => path,
        Err(error) => return Outcome::OwnPathUnknown(error.to_string()),
    };
    let Some(gui) = sibling_gui_path(&launcher) else {
        return Outcome::OwnPathUnknown(format!("{} has no folder", launcher.display()));
    };
    if !is_file(&gui) {
        return Outcome::GuiMissing(gui);
    }
    match start(&gui, args) {
        Ok(()) => Outcome::Started,
        Err(error) => Outcome::GuiNotStarted {
            gui,
            reason: error.to_string(),
        },
    }
}

fn main() -> ExitCode {
    let outcome = launch(
        std::env::current_exe(),
        std::env::args_os().skip(1).collect(),
        Path::is_file,
        |gui, args| Command::new(gui).args(args).spawn().map(drop),
    );
    if let Some(message) = outcome.message() {
        eprintln!("{message}");
    }
    ExitCode::from(outcome.status())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    fn gui_name() -> String {
        format!("jotra-tauri{}", std::env::consts::EXE_SUFFIX)
    }

    fn install_folder() -> PathBuf {
        std::env::temp_dir().join("install")
    }

    fn launcher_path() -> PathBuf {
        install_folder().join(format!("writ-tauri{}", std::env::consts::EXE_SUFFIX))
    }

    #[test]
    fn the_gui_is_looked_for_beside_the_launcher() {
        assert_eq!(
            sibling_gui_path(&launcher_path()),
            Some(install_folder().join(gui_name()))
        );
    }

    #[test]
    fn a_launcher_path_without_a_folder_has_no_sibling() {
        assert_eq!(sibling_gui_path(Path::new("")), None);
    }

    #[test]
    fn the_gui_starts_with_every_argument_in_order() {
        let started = RefCell::new(None);
        let args = vec![
            OsString::from("--login"),
            OsString::from("C:\\notes\\a b.txt"),
        ];

        let outcome = launch(
            Ok(launcher_path()),
            args.clone(),
            |_| true,
            |gui, passed| {
                *started.borrow_mut() = Some((gui.to_path_buf(), passed));
                Ok(())
            },
        );

        assert_eq!(outcome, Outcome::Started);
        assert_eq!(outcome.status(), 0);
        assert_eq!(outcome.message(), None);
        assert_eq!(
            started.into_inner(),
            Some((install_folder().join(gui_name()), args))
        );
    }

    #[test]
    fn a_missing_gui_is_not_started_and_exits_non_zero() {
        let mut called = false;

        let outcome = launch(
            Ok(launcher_path()),
            vec![OsString::from("--login")],
            |_| false,
            |_, _| {
                called = true;
                Ok(())
            },
        );

        assert!(!called);
        assert_eq!(
            outcome,
            Outcome::GuiMissing(install_folder().join(gui_name()))
        );
        assert_eq!(outcome.status(), STATUS_GUI_MISSING);
        let message = outcome.message().expect("a missing gui is reported");
        assert!(message.contains(&gui_name()), "{message}");
    }

    #[test]
    fn an_unknown_launcher_path_exits_non_zero() {
        let outcome = launch(
            Err(io::Error::new(io::ErrorKind::NotFound, "no path")),
            Vec::new(),
            |_| true,
            |_, _| Ok(()),
        );

        assert_eq!(outcome, Outcome::OwnPathUnknown("no path".to_owned()));
        assert_eq!(outcome.status(), STATUS_OWN_PATH_UNKNOWN);
    }

    #[test]
    fn a_failed_start_exits_non_zero() {
        let outcome = launch(
            Ok(launcher_path()),
            Vec::new(),
            |_| true,
            |_, _| Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied")),
        );

        assert_eq!(
            outcome,
            Outcome::GuiNotStarted {
                gui: install_folder().join(gui_name()),
                reason: "denied".to_owned(),
            }
        );
        assert_eq!(outcome.status(), STATUS_GUI_NOT_STARTED);
    }

    #[test]
    fn every_failure_has_its_own_non_zero_status() {
        let statuses = [
            STATUS_GUI_MISSING,
            STATUS_OWN_PATH_UNKNOWN,
            STATUS_GUI_NOT_STARTED,
        ];
        assert!(statuses.iter().all(|status| *status != 0));
        for (index, status) in statuses.iter().enumerate() {
            assert!(!statuses[index + 1..].contains(status));
        }
    }
}
