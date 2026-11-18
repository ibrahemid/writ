//! The Linux login item and AppImage files after the rename (ADR-059
//! decision 11), carried out over the plans in
//! [`jotra_core::linux_rename`].

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use jotra_core::legacy_names;
use jotra_core::linux_rename::{
    appimage_command_copies, autostart_file_name, is_stale_command_copy, plan_appimage_entry,
    plan_autostart, user_applications_dir, AutostartFacts, AutostartPlan, CommandCopy,
    APPIMAGE_ENTRY_FILE, AUTOSTART_FOLDER,
};
use jotra_core::name_migration::MigrationStep;
use jotra_storage::atomic::{write_atomic, write_new, AtomicWriteError};
use jotra_storage::name_migration::{run_migration_step, StepCompletion, StepRun};
use tauri::{AppHandle, Manager};
use tracing::{info, warn};

use crate::commands::cli::{bundled_cli_path, copy_command, legacy_sidecar_name};
use crate::state::AppState;

/// Why a Linux rename step did not finish.
#[derive(Debug, thiserror::Error)]
pub(crate) enum LinuxRenameError {
    /// A file system call failed.
    #[error("{operation} {}: {source}", path.display())]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    /// A desktop entry was not written.
    #[error("write {}: {source}", path.display())]
    Write {
        path: PathBuf,
        #[source]
        source: AtomicWriteError,
    },
}

fn io_error(operation: &'static str, path: &Path, source: io::Error) -> LinuxRenameError {
    LinuxRenameError::Io {
        operation,
        path: path.to_path_buf(),
        source,
    }
}

fn write_error(path: &Path, source: AtomicWriteError) -> LinuxRenameError {
    LinuxRenameError::Write {
        path: path.to_path_buf(),
        source,
    }
}

/// Whether `path` is a regular file, not following a link.
fn is_regular_file(path: &Path) -> Result<bool, LinuxRenameError> {
    match path.symlink_metadata() {
        Ok(metadata) => Ok(metadata.is_file()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(source) => Err(io_error("inspect", path, source)),
    }
}

/// The text of the regular file at `path`; `None` when there is none or it
/// is not UTF-8.
fn read_regular_text(path: &Path) -> Result<Option<String>, LinuxRenameError> {
    if !is_regular_file(path)? {
        return Ok(None);
    }
    let bytes = fs::read(path).map_err(|source| io_error("read", path, source))?;
    Ok(String::from_utf8(bytes).ok())
}

fn remove_if_present(path: &Path) -> Result<(), LinuxRenameError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(io_error("remove", path, source)),
    }
}

/// Moves the login item's autostart entry in `autostart_dir` from the old
/// product name to `app_name`, starting `app_path`.
pub(crate) fn migrate_autostart(
    autostart_dir: &Path,
    app_name: &str,
    app_path: &Path,
) -> Result<StepCompletion, LinuxRenameError> {
    let legacy_path = autostart_dir.join(autostart_file_name(legacy_names::PRODUCT_NAME));
    let current_path = autostart_dir.join(autostart_file_name(app_name));
    let legacy_entry = read_regular_text(&legacy_path)?;
    let plan = plan_autostart(&AutostartFacts {
        legacy_entry: legacy_entry.as_deref(),
        current_present: current_path.symlink_metadata().is_ok(),
        app_name,
        app_path: app_path.to_str(),
    });
    match plan {
        AutostartPlan::Untouched => Ok(StepCompletion::Done),
        AutostartPlan::ForeignProgram | AutostartPlan::PathNotWritable => {
            Ok(StepCompletion::Declined)
        }
        AutostartPlan::RemoveLegacy => {
            remove_if_present(&legacy_path)?;
            Ok(StepCompletion::Done)
        }
        AutostartPlan::Replace { entry } => {
            write_new(&current_path, entry.as_bytes())
                .map_err(|source| write_error(&current_path, source))?;
            remove_if_present(&legacy_path)?;
            Ok(StepCompletion::Done)
        }
    }
}

/// Rewrites the AppImage's desktop entry in `applications_dir` for `app_name`
/// when it starts `image`. Whether it was rewritten.
pub(crate) fn refresh_appimage_entry(
    applications_dir: &Path,
    image: &Path,
    app_name: &str,
) -> Result<bool, LinuxRenameError> {
    let path = applications_dir.join(APPIMAGE_ENTRY_FILE);
    let Some(entry) = read_regular_text(&path)? else {
        return Ok(false);
    };
    let Some(updated) = image
        .to_str()
        .and_then(|image| plan_appimage_entry(&entry, image, app_name))
    else {
        return Ok(false);
    };
    write_atomic(&path, updated.as_bytes()).map_err(|source| write_error(&path, source))?;
    Ok(true)
}

fn refresh_command_copy(path: &Path, source: &Path) -> Result<bool, LinuxRenameError> {
    if !is_regular_file(path)? {
        return Ok(false);
    }
    let copy = fs::read(path).map_err(|error| io_error("read", path, error))?;
    let bundled = fs::read(source).map_err(|error| io_error("read", source, error))?;
    if !is_stale_command_copy(&copy, &bundled) {
        return Ok(false);
    }
    copy_command(source, path).map_err(|error| io_error("copy", path, error))?;
    Ok(true)
}

/// Replaces each of `copies` that is another build of the command with
/// `bundled`, or with `legacy_bundled` for a copy under the old name when the
/// image carries one. The paths refreshed.
pub(crate) fn refresh_command_copies(
    copies: &[CommandCopy],
    bundled: &Path,
    legacy_bundled: Option<&Path>,
) -> Vec<PathBuf> {
    let mut refreshed = Vec::new();
    for copy in copies {
        let source = match legacy_bundled {
            Some(legacy) if copy.legacy_name => legacy,
            _ => bundled,
        };
        match refresh_command_copy(&copy.path, source) {
            Ok(true) => refreshed.push(copy.path.clone()),
            Ok(false) => {}
            Err(error) => warn!(%error, "the command copy was left as it was"),
        }
    }
    refreshed
}

/// The files of the AppImage this launch runs from.
#[derive(Debug, Clone)]
pub(crate) struct AppImageFiles {
    /// The image, as the AppImage runtime names it.
    pub(crate) image: PathBuf,
    /// The user's applications folder, when it is known.
    pub(crate) applications_dir: Option<PathBuf>,
    /// The command inside the running image, when it is there.
    pub(crate) bundled_cli: Option<PathBuf>,
}

/// What the Linux rename steps act on.
#[derive(Debug, Clone)]
pub(crate) struct LinuxRenameSteps {
    /// The data folder holding the migration record.
    pub(crate) data_dir: PathBuf,
    /// The running version, as the record stores it.
    pub(crate) version: String,
    /// The product name this build registers its login item under.
    pub(crate) app_name: String,
    /// The home folder, when it is known.
    pub(crate) home: Option<PathBuf>,
    /// The path the login item starts: the image when running from one.
    pub(crate) app_path: Option<PathBuf>,
    /// The AppImage this launch runs from, when it runs from one.
    pub(crate) appimage: Option<AppImageFiles>,
}

fn refresh_appimage(files: &AppImageFiles, home: Option<&Path>, app_name: &str) {
    if let Some(applications_dir) = &files.applications_dir {
        match refresh_appimage_entry(applications_dir, &files.image, app_name) {
            Ok(true) => info!("the AppImage desktop entry was rewritten"),
            Ok(false) => {}
            Err(error) => warn!(%error, "the AppImage desktop entry was left as it was"),
        }
    }
    let Some(bundled) = &files.bundled_cli else {
        warn!("the image carries no command; its copies were left as they were");
        return;
    };
    let legacy = bundled.with_file_name(legacy_sidecar_name());
    let legacy = legacy.is_file().then_some(legacy.as_path());
    let copies = appimage_command_copies(home, &files.image);
    let refreshed = refresh_command_copies(&copies, bundled, legacy);
    if !refreshed.is_empty() {
        info!(?refreshed, "command copies refreshed from the image");
    }
}

/// Runs the autostart step once per the record, then the AppImage refresh on
/// every launch from an image. The autostart step's run, when it could run.
pub(crate) fn run_rename_steps(steps: &LinuxRenameSteps) -> Option<StepRun> {
    let autostart = match (&steps.home, &steps.app_path) {
        (Some(home), Some(app_path)) => Some(run_migration_step(
            &steps.data_dir,
            MigrationStep::LinuxAutostart,
            &steps.version,
            || migrate_autostart(&home.join(AUTOSTART_FOLDER), &steps.app_name, app_path),
        )),
        _ => {
            warn!(
                step = MigrationStep::LinuxAutostart.key(),
                "the home folder or the app's path is unknown"
            );
            None
        }
    };
    if let Some(files) = &steps.appimage {
        refresh_appimage(files, steps.home.as_deref(), &steps.app_name);
    }
    autostart
}

/// Runs [`run_rename_steps`] off the main thread in a release build.
pub(crate) fn spawn_rename_steps(handle: &AppHandle) {
    if cfg!(debug_assertions) {
        return;
    }
    let package = handle.package_info();
    let home = dirs::home_dir();
    let image = std::env::var_os("APPIMAGE")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let data_home = std::env::var("XDG_DATA_HOME").ok();
    let steps = LinuxRenameSteps {
        data_dir: handle.state::<AppState>().jotra_dir.clone(),
        version: package.version.to_string(),
        app_name: package.name.clone(),
        app_path: image.clone().or_else(|| std::env::current_exe().ok()),
        appimage: image.map(|image| AppImageFiles {
            image,
            applications_dir: user_applications_dir(data_home.as_deref(), home.as_deref()),
            bundled_cli: bundled_cli_path(),
        }),
        home,
    };
    std::thread::spawn(move || {
        run_rename_steps(&steps);
    });
}

#[cfg(test)]
mod tests {
    use jotra_core::cli_install::CLI_SIGNATURE;
    use jotra_core::linux_rename::desktop_entry_value;
    use jotra_core::name_migration::StepOutcome;
    use jotra_storage::name_migration::read_name_migration_record;

    use super::*;

    const PLUGIN_ENTRY: &str = "[Desktop Entry]\nType=Application\nVersion=1.0\nName=Writ\nComment=Writ startup script\nExec=/usr/bin/writ-tauri --login\nStartupNotify=false\nTerminal=false";

    fn autostart_dir(home: &Path) -> PathBuf {
        let dir = home.join(AUTOSTART_FOLDER);
        fs::create_dir_all(&dir).expect("autostart folder");
        dir
    }

    fn signed(tag: &str) -> Vec<u8> {
        [tag.as_bytes(), CLI_SIGNATURE].concat()
    }

    #[test]
    fn the_autostart_entry_moves_and_stays_turned_off() {
        let home = tempfile::tempdir().expect("home");
        let dir = autostart_dir(home.path());
        let legacy = dir.join("Writ.desktop");
        fs::write(&legacy, format!("{PLUGIN_ENTRY}\nHidden=true\n")).expect("legacy entry");

        let completion =
            migrate_autostart(&dir, "Jotra", Path::new("/usr/bin/jotra-tauri")).expect("migrate");

        assert_eq!(completion, StepCompletion::Done);
        assert!(!legacy.exists());
        let entry = fs::read_to_string(dir.join("Jotra.desktop")).expect("current entry");
        assert_eq!(
            desktop_entry_value(&entry, "Exec"),
            Some("/usr/bin/jotra-tauri --login")
        );
        assert_eq!(desktop_entry_value(&entry, "Hidden"), Some("true"));
        assert_eq!(desktop_entry_value(&entry, "Name"), Some("Jotra"));
    }

    #[test]
    fn an_existing_current_entry_is_kept_and_the_old_one_removed() {
        let home = tempfile::tempdir().expect("home");
        let dir = autostart_dir(home.path());
        fs::write(dir.join("Writ.desktop"), PLUGIN_ENTRY).expect("legacy entry");
        fs::write(dir.join("Jotra.desktop"), "kept").expect("current entry");

        migrate_autostart(&dir, "Jotra", Path::new("/usr/bin/jotra-tauri")).expect("migrate");

        assert!(!dir.join("Writ.desktop").exists());
        assert_eq!(
            fs::read_to_string(dir.join("Jotra.desktop")).expect("current"),
            "kept"
        );
    }

    #[test]
    fn another_programs_entry_is_left_alone() {
        let home = tempfile::tempdir().expect("home");
        let dir = autostart_dir(home.path());
        let other = PLUGIN_ENTRY.replace(" --login", "");
        fs::write(dir.join("Writ.desktop"), &other).expect("other entry");

        let completion =
            migrate_autostart(&dir, "Jotra", Path::new("/usr/bin/jotra-tauri")).expect("migrate");

        assert_eq!(completion, StepCompletion::Done);
        assert_eq!(
            fs::read_to_string(dir.join("Writ.desktop")).expect("other"),
            other
        );
        assert!(!dir.join("Jotra.desktop").exists());
    }

    #[test]
    fn a_login_entry_for_another_program_declines_and_moves_nothing() {
        let home = tempfile::tempdir().expect("home");
        let dir = autostart_dir(home.path());
        let other = PLUGIN_ENTRY.replace("/usr/bin/writ-tauri", "/opt/other/launcher");
        fs::write(dir.join("Writ.desktop"), &other).expect("other entry");

        let completion =
            migrate_autostart(&dir, "Jotra", Path::new("/usr/bin/jotra-tauri")).expect("migrate");

        assert_eq!(completion, StepCompletion::Declined);
        assert_eq!(
            fs::read_to_string(dir.join("Writ.desktop")).expect("other"),
            other
        );
        assert!(!dir.join("Jotra.desktop").exists());
    }

    #[test]
    fn a_path_the_entry_cannot_hold_declines_and_moves_nothing() {
        let home = tempfile::tempdir().expect("home");
        let dir = autostart_dir(home.path());
        fs::write(dir.join("Writ.desktop"), PLUGIN_ENTRY).expect("legacy entry");

        let completion =
            migrate_autostart(&dir, "Jotra", Path::new("/home/a/$x/Jotra.AppImage")).expect("run");

        assert_eq!(completion, StepCompletion::Declined);
        assert!(dir.join("Writ.desktop").exists());
        assert!(!dir.join("Jotra.desktop").exists());
    }

    #[test]
    fn no_autostart_folder_is_nothing_to_do() {
        let home = tempfile::tempdir().expect("home");

        let completion = migrate_autostart(
            &home.path().join(AUTOSTART_FOLDER),
            "Jotra",
            Path::new("/usr/bin/jotra-tauri"),
        )
        .expect("migrate");

        assert_eq!(completion, StepCompletion::Done);
        assert!(!home.path().join(AUTOSTART_FOLDER).exists());
    }

    #[test]
    fn the_autostart_step_is_recorded_once() {
        let home = tempfile::tempdir().expect("home");
        let data = tempfile::tempdir().expect("data");
        let dir = autostart_dir(home.path());
        fs::write(dir.join("Writ.desktop"), PLUGIN_ENTRY).expect("legacy entry");
        let steps = LinuxRenameSteps {
            data_dir: data.path().to_path_buf(),
            version: "1.1.0".to_owned(),
            app_name: "Jotra".to_owned(),
            home: Some(home.path().to_path_buf()),
            app_path: Some(PathBuf::from("/usr/bin/jotra-tauri")),
            appimage: None,
        };

        assert_eq!(run_rename_steps(&steps), Some(StepRun::Done));
        assert_eq!(run_rename_steps(&steps), Some(StepRun::AlreadySettled));
        let record = read_name_migration_record(data.path());
        assert_eq!(
            record
                .entry(MigrationStep::LinuxAutostart)
                .map(|entry| entry.outcome),
            Some(StepOutcome::Done)
        );
        assert!(dir.join("Jotra.desktop").exists());
    }

    #[test]
    fn without_a_home_folder_the_step_does_not_run() {
        let data = tempfile::tempdir().expect("data");
        let steps = LinuxRenameSteps {
            data_dir: data.path().to_path_buf(),
            version: "1.1.0".to_owned(),
            app_name: "Jotra".to_owned(),
            home: None,
            app_path: Some(PathBuf::from("/usr/bin/jotra-tauri")),
            appimage: None,
        };

        assert_eq!(run_rename_steps(&steps), None);
        assert!(read_name_migration_record(data.path())
            .entry(MigrationStep::LinuxAutostart)
            .is_none());
    }

    #[test]
    fn only_older_signed_copies_are_refreshed() {
        let root = tempfile::tempdir().expect("root");
        let image_dir = root.path().join("image");
        fs::create_dir_all(&image_dir).expect("image folder");
        let bundled = image_dir.join("jotra");
        fs::write(&bundled, signed("new")).expect("bundled");
        let stale = root.path().join("stale");
        fs::write(&stale, signed("old")).expect("stale");
        let current = root.path().join("current");
        fs::write(&current, signed("new")).expect("current");
        let wrapper = root.path().join("wrapper");
        fs::write(&wrapper, "#!/bin/sh\nexec jotra-cli \"$@\"\n").expect("wrapper");
        let missing = root.path().join("missing");
        let copies: Vec<CommandCopy> = [&stale, &current, &wrapper, &missing]
            .into_iter()
            .map(|path| CommandCopy {
                path: path.clone(),
                legacy_name: false,
            })
            .collect();

        let refreshed = refresh_command_copies(&copies, &bundled, None);

        assert_eq!(refreshed, vec![stale.clone()]);
        assert_eq!(fs::read(&stale).expect("stale"), signed("new"));
        assert!(fs::read_to_string(&wrapper)
            .expect("wrapper")
            .starts_with("#!/bin/sh"));
        assert!(!missing.exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_linked_copy_is_left_alone_and_a_refreshed_copy_runs() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().expect("root");
        let bundled = root.path().join("jotra");
        fs::write(&bundled, signed("new")).expect("bundled");
        let elsewhere = root.path().join("elsewhere");
        fs::write(&elsewhere, signed("old")).expect("elsewhere");
        let link = root.path().join("link");
        std::os::unix::fs::symlink(&elsewhere, &link).expect("link");
        let stale = root.path().join("stale");
        fs::write(&stale, signed("old")).expect("stale");
        let copies = [&link, &stale].map(|path| CommandCopy {
            path: path.clone(),
            legacy_name: false,
        });

        let refreshed = refresh_command_copies(&copies, &bundled, None);

        assert_eq!(refreshed, vec![stale.clone()]);
        assert_eq!(fs::read(&elsewhere).expect("elsewhere"), signed("old"));
        let mode = fs::metadata(&stale).expect("stale").permissions().mode();
        assert_eq!(mode & 0o111, 0o111);
    }

    #[test]
    fn a_copy_under_the_old_name_takes_the_bundled_old_name() {
        let root = tempfile::tempdir().expect("root");
        let bundled = root.path().join("jotra");
        fs::write(&bundled, signed("new jotra")).expect("bundled");
        let legacy_bundled = root.path().join("writ");
        fs::write(&legacy_bundled, signed("new writ")).expect("legacy bundled");
        let legacy_copy = root.path().join("writ-cli");
        fs::write(&legacy_copy, signed("old")).expect("legacy copy");
        let copy = root.path().join("jotra-cli");
        fs::write(&copy, signed("old")).expect("copy");
        let copies = [
            CommandCopy {
                path: legacy_copy.clone(),
                legacy_name: true,
            },
            CommandCopy {
                path: copy.clone(),
                legacy_name: false,
            },
        ];

        refresh_command_copies(&copies, &bundled, Some(&legacy_bundled));
        assert_eq!(fs::read(&legacy_copy).expect("legacy"), signed("new writ"));
        assert_eq!(fs::read(&copy).expect("copy"), signed("new jotra"));

        fs::write(&legacy_copy, signed("old")).expect("legacy copy");
        refresh_command_copies(&copies, &bundled, None);
        assert_eq!(fs::read(&legacy_copy).expect("legacy"), signed("new jotra"));
    }

    #[test]
    fn the_appimage_entry_is_rewritten_for_the_running_image() {
        let root = tempfile::tempdir().expect("root");
        let applications = root.path().join("applications");
        fs::create_dir_all(&applications).expect("applications");
        let image = Path::new("/home/a/Apps/Writ.AppImage");
        let entry_path = applications.join(APPIMAGE_ENTRY_FILE);
        fs::write(
            &entry_path,
            "[Desktop Entry]\nName=Writ\nExec=/home/a/Apps/Writ.AppImage %F\n",
        )
        .expect("entry");

        assert!(refresh_appimage_entry(&applications, image, "Jotra").expect("refresh"));
        let entry = fs::read_to_string(&entry_path).expect("entry");
        assert_eq!(desktop_entry_value(&entry, "Name"), Some("Jotra"));
        assert!(!refresh_appimage_entry(&applications, image, "Jotra").expect("again"));
    }

    #[test]
    fn another_images_entry_or_none_is_left_alone() {
        let root = tempfile::tempdir().expect("root");
        let applications = root.path().join("applications");
        let image = root.path().join("Writ.AppImage");

        assert!(!refresh_appimage_entry(&applications, &image, "Jotra").expect("absent"));

        fs::create_dir_all(&applications).expect("applications");
        let other = "[Desktop Entry]\nName=Writ\nExec=/opt/other/Writ.AppImage %F\n";
        fs::write(applications.join(APPIMAGE_ENTRY_FILE), other).expect("entry");
        assert!(!refresh_appimage_entry(&applications, &image, "Jotra").expect("other"));
        assert_eq!(
            fs::read_to_string(applications.join(APPIMAGE_ENTRY_FILE)).expect("entry"),
            other
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_appimage_launch_refreshes_its_files() {
        let root = tempfile::tempdir().expect("root");
        let data = tempfile::tempdir().expect("data");
        let home = root.path().join("home");
        let image_dir = home.join(".local/share/writ");
        fs::create_dir_all(&image_dir).expect("image folder");
        let image = image_dir.join("Writ.AppImage");
        let mount = root.path().join("mount");
        fs::create_dir_all(&mount).expect("mount");
        let bundled = mount.join("jotra");
        fs::write(&bundled, signed("new")).expect("bundled");
        let beside = image_dir.join("writ-cli");
        fs::write(&beside, signed("old")).expect("beside");
        let applications = home.join(".local/share/applications");
        fs::create_dir_all(&applications).expect("applications");
        fs::write(
            applications.join(APPIMAGE_ENTRY_FILE),
            format!("[Desktop Entry]\nName=Writ\nExec={}\n", image.display()),
        )
        .expect("entry");
        let steps = LinuxRenameSteps {
            data_dir: data.path().to_path_buf(),
            version: "1.1.0".to_owned(),
            app_name: "Jotra".to_owned(),
            home: Some(home.clone()),
            app_path: Some(image.clone()),
            appimage: Some(AppImageFiles {
                image: image.clone(),
                applications_dir: Some(applications.clone()),
                bundled_cli: Some(bundled),
            }),
        };

        assert_eq!(run_rename_steps(&steps), Some(StepRun::Done));

        assert_eq!(fs::read(&beside).expect("beside"), signed("new"));
        let entry = fs::read_to_string(applications.join(APPIMAGE_ENTRY_FILE)).expect("entry");
        assert_eq!(desktop_entry_value(&entry, "Name"), Some("Jotra"));
    }
}
