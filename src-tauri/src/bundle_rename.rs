//! Moves the macOS bundle to its new name before the Tauri builder runs, and
//! runs the per-user steps that follow it (ADR-059 decision 8).
//!
//! `jotra_core::bundle_rename` decides; this module gathers the facts, does
//! the file system work and records each outcome in the name migration record.
//! Nothing here stops the launch: a step that fails is logged and recorded,
//! and the app runs from where it is.

use std::ffi::{CString, OsStr, OsString};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use jotra_core::bundle_rename::{
    bundle_of_executable, executable_in_bundle, info_plist_path, is_own_login_agent_program,
    is_rename_skipped, per_user_steps_apply, plan_bundle_rename, plan_command_links,
    plan_login_item, relaunch_arguments, relaunch_environment, sibling_path, BundleFacts,
    BundleInfo, BundlePlan, CommandLinkFacts, CommandLinkPlan, LegacyLoginAgent, Privileges,
    RenamePlan, SiblingState, LAUNCH_AGENTS_FOLDER, SKIP_VAR,
};
use jotra_core::cli_install::{COMMAND_NAME, SYSTEM_LINK};
use jotra_core::legacy_names;
use jotra_core::name_migration::{MigrationStep, NameMigrationRecord};
use jotra_storage::instance::InstanceLock;
use jotra_storage::name_migration::{
    read_name_migration_record, run_migration_step, StepCompletion, StepRun,
};
use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::ManagerExt;
use tracing::{info, warn};

use crate::commands::cli::{applescript_escape, inspect_entry, link_directly, shell_single_quote};
use crate::commands::login_item::{LoginItem, LoginItemError};
use crate::commands::update::dir_is_writable;
use crate::logging::LogGuard;
use crate::poison::recover_poison;
use crate::state::AppState;

const OSASCRIPT: &str = "/usr/bin/osascript";
const OPEN: &str = "/usr/bin/open";

/// Why a step of the bundle rename or the per-user steps did not finish.
#[derive(Debug, thiserror::Error)]
pub(crate) enum BundleRenameError {
    /// A file system call failed.
    #[error("{operation} {}: {source}", path.display())]
    Io {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    /// A bundle's `Info.plist` does not read.
    #[error("{} does not read: {reason}", path.display())]
    InfoPlist { path: PathBuf, reason: String },
    /// The rename under an administrator's password failed.
    #[error("the administrator rename failed: {0}")]
    Administrator(String),
    /// A bundle did not move to the Trash.
    #[error("{} did not move to the Trash: {reason}", path.display())]
    Trash { path: PathBuf, reason: String },
    /// Launch Services refused the bundle.
    #[error("Launch Services did not register {}: status {status}", path.display())]
    Register { path: PathBuf, status: i32 },
    /// A bundle did not open.
    #[error("{} did not open: {reason}", path.display())]
    Open { path: PathBuf, reason: String },
    /// A command or the link to it does not lead to a file.
    #[error("{} does not lead to a file", path.display())]
    Unresolved { path: PathBuf },
    /// The login item service refused.
    #[error("the login item: {0}")]
    LoginItem(#[from] LoginItemError),
}

fn io_error(operation: &'static str, path: &Path, source: io::Error) -> BundleRenameError {
    BundleRenameError::Io {
        operation,
        path: path.to_path_buf(),
        source,
    }
}

/// The system calls the rename makes that tests cannot make on this machine.
pub(crate) trait BundleHost {
    /// Moves `path` to the Trash.
    fn trash(&self, path: &Path) -> Result<(), BundleRenameError>;
    /// Renames `from` to `to` and leaves the hidden link at `from`, under an
    /// administrator's password. A cancelled prompt is a decline.
    fn rename_as_administrator(
        &self,
        from: &Path,
        to: &Path,
    ) -> Result<StepCompletion, BundleRenameError>;
    /// Registers `bundle` with Launch Services.
    fn register(&self, bundle: &Path) -> Result<(), BundleRenameError>;
    /// Opens a new instance of `bundle` with the variables `env` and `args`.
    fn open(
        &self,
        bundle: &Path,
        env: &[(OsString, OsString)],
        args: &[OsString],
    ) -> Result<(), BundleRenameError>;
}

/// The host that makes the real calls.
pub(crate) struct MacHost;

#[allow(non_snake_case)]
#[link(name = "CoreServices", kind = "framework")]
extern "C" {
    fn LSRegisterURL(inURL: core_foundation::url::CFURLRef, inUpdate: u8) -> i32;
}

impl BundleHost for MacHost {
    fn trash(&self, path: &Path) -> Result<(), BundleRenameError> {
        use objc2_foundation::{NSFileManager, NSString, NSURL};

        let text = path.to_str().ok_or_else(|| BundleRenameError::Trash {
            path: path.to_path_buf(),
            reason: "the path is not UTF-8".to_string(),
        })?;
        objc2::rc::autoreleasepool(|_| {
            let url = NSURL::fileURLWithPath(&NSString::from_str(text));
            NSFileManager::defaultManager()
                .trashItemAtURL_resultingItemURL_error(&url, None)
                .map_err(|error| BundleRenameError::Trash {
                    path: path.to_path_buf(),
                    reason: error.localizedDescription().to_string(),
                })
        })
    }

    fn rename_as_administrator(
        &self,
        from: &Path,
        to: &Path,
    ) -> Result<StepCompletion, BundleRenameError> {
        let script = administrator_rename_script(from, to)?;
        let output = Command::new(OSASCRIPT)
            .arg("-e")
            .arg(&script)
            .output()
            .map_err(|source| io_error("run", Path::new(OSASCRIPT), source))?;
        if output.status.success() {
            return Ok(StepCompletion::Done);
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        if is_cancelled(&stderr) {
            Ok(StepCompletion::Declined)
        } else {
            Err(BundleRenameError::Administrator(stderr.trim().to_string()))
        }
    }

    fn register(&self, bundle: &Path) -> Result<(), BundleRenameError> {
        use core_foundation::base::TCFType;
        use core_foundation::url::CFURL;

        let url = CFURL::from_path(bundle, true).ok_or_else(|| BundleRenameError::Register {
            path: bundle.to_path_buf(),
            status: -1,
        })?;
        let status = unsafe { LSRegisterURL(url.as_concrete_TypeRef(), 1) };
        if status == 0 {
            Ok(())
        } else {
            Err(BundleRenameError::Register {
                path: bundle.to_path_buf(),
                status,
            })
        }
    }

    fn open(
        &self,
        bundle: &Path,
        env: &[(OsString, OsString)],
        args: &[OsString],
    ) -> Result<(), BundleRenameError> {
        let status = Command::new(OPEN)
            .args(open_arguments(bundle, env, args))
            .status()
            .map_err(|source| io_error("run", Path::new(OPEN), source))?;
        if status.success() {
            Ok(())
        } else {
            Err(BundleRenameError::Open {
                path: bundle.to_path_buf(),
                reason: status.to_string(),
            })
        }
    }
}

/// The error number AppleScript reports for a cancelled prompt, as osascript
/// prints it.
const CANCELLED_TOKEN: &str = "(-128)";

/// Whether osascript's error output is a cancelled password prompt.
fn is_cancelled(stderr: &str) -> bool {
    stderr
        .split_whitespace()
        .any(|token| token == CANCELLED_TOKEN)
}

/// The arguments `open` takes to start a new instance of `bundle` with the
/// variables `env` and `args`.
pub(crate) fn open_arguments(
    bundle: &Path,
    env: &[(OsString, OsString)],
    args: &[OsString],
) -> Vec<OsString> {
    let mut all = vec![OsString::from("-n")];
    for (name, value) in env {
        let mut entry = name.clone();
        entry.push("=");
        entry.push(value);
        all.push(OsString::from("--env"));
        all.push(entry);
    }
    all.push(OsString::from("-a"));
    all.push(bundle.as_os_str().to_os_string());
    if !args.is_empty() {
        all.push(OsString::from("--args"));
        all.extend(args.iter().cloned());
    }
    all
}

/// The AppleScript that renames `from` to `to` and leaves the hidden link at
/// `from`, in one administrator prompt. It moves nothing when `to` exists.
pub(crate) fn administrator_rename_script(
    from: &Path,
    to: &Path,
) -> Result<String, BundleRenameError> {
    let quoted_to = shell_single_quote(&to.to_string_lossy());
    let shell = format!(
        "test ! -e {quoted_to} && test ! -L {quoted_to} && {}",
        move_and_link_shell(from, to)?
    );
    Ok(format!(
        "do shell script \"{}\" with administrator privileges",
        applescript_escape(&shell)
    ))
}

/// The shell commands that move `from` to `to` and leave the hidden link at
/// `from`. A move that lands inside a folder made at `to` after the check is
/// moved back and fails, and the link is left only once `to` holds the
/// bundle's `Info.plist`.
pub(crate) fn move_and_link_shell(from: &Path, to: &Path) -> Result<String, BundleRenameError> {
    let target = link_target(to)?;
    let nested = to.join(link_target(from)?);
    let plist = info_plist_path(to);
    let [from, to, target, nested, plist] = [from, to, target, nested.as_path(), plist.as_path()]
        .map(|path| shell_single_quote(&path.to_string_lossy()));
    Ok(format!(
        "/bin/mv {from} {to} && \
         if test -e {nested} || test -L {nested}; then \
         test ! -e {from} && test ! -L {from} && /bin/mv {nested} {from}; exit 1; \
         fi && \
         test -f {plist} && /bin/ln -s {target} {from} && /usr/bin/chflags -h hidden {from}"
    ))
}

/// The relative target of the link left beside `bundle`: its folder name.
fn link_target(bundle: &Path) -> Result<&Path, BundleRenameError> {
    bundle.file_name().map(Path::new).ok_or_else(|| {
        io_error(
            "name the link target",
            bundle,
            io::ErrorKind::InvalidInput.into(),
        )
    })
}

/// The identifier and version in `bundle`'s `Info.plist`.
pub(crate) fn read_bundle_info(bundle: &Path) -> Result<BundleInfo, BundleRenameError> {
    let path = info_plist_path(bundle);
    let invalid = |reason: String| BundleRenameError::InfoPlist {
        path: path.clone(),
        reason,
    };
    let value = plist::Value::from_file(&path).map_err(|error| invalid(error.to_string()))?;
    let dictionary = value
        .as_dictionary()
        .ok_or_else(|| invalid("the top level is not a dictionary".to_string()))?;
    let field = |key: &str| {
        dictionary
            .get(key)
            .and_then(plist::Value::as_string)
            .map(str::to_owned)
            .ok_or_else(|| invalid(format!("{key} is missing")))
    };
    Ok(BundleInfo {
        identifier: field("CFBundleIdentifier")?,
        version: field("CFBundleShortVersionString")?,
    })
}

fn inspect_sibling(path: &Path) -> SiblingState {
    match path.symlink_metadata() {
        Err(error) if error.kind() == io::ErrorKind::NotFound => SiblingState::Absent,
        Err(_) => SiblingState::Unreadable,
        Ok(metadata) if metadata.file_type().is_symlink() => SiblingState::Link,
        Ok(metadata) if metadata.is_dir() => read_bundle_info(path)
            .map(SiblingState::Bundle)
            .unwrap_or(SiblingState::Unreadable),
        Ok(_) => SiblingState::Unreadable,
    }
}

fn runs_through_link(exe: &Path) -> bool {
    exe.ancestors().any(|ancestor| {
        ancestor
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
    })
}

fn is_read_only_volume(path: &Path) -> bool {
    rustix::fs::statvfs(path)
        .is_ok_and(|stats| stats.f_flag.contains(rustix::fs::StatVfsMountFlags::RDONLY))
}

/// What the launch started as `exe` knows about its bundle.
pub(crate) fn gather_facts(
    exe: &Path,
    record: NameMigrationRecord,
    skip_requested: bool,
) -> BundleFacts {
    let bundle = bundle_of_executable(exe);
    let running = bundle.as_deref().and_then(|bundle| {
        read_bundle_info(bundle)
            .inspect_err(|error| warn!(operation = "read bundle info", %error))
            .ok()
    });
    BundleFacts {
        exe_through_link: runs_through_link(exe),
        canonical_bundle: exe
            .canonicalize()
            .ok()
            .as_deref()
            .and_then(bundle_of_executable),
        sibling: bundle
            .as_deref()
            .and_then(sibling_path)
            .map_or(SiblingState::Absent, |sibling| inspect_sibling(&sibling)),
        read_only_volume: bundle.as_deref().is_some_and(is_read_only_volume),
        folder_writable: bundle
            .as_deref()
            .and_then(Path::parent)
            .is_some_and(dir_is_writable),
        bundle,
        running,
        skip_requested,
        record,
    }
}

/// Renames `from` to `to`, failing rather than replacing anything at `to`.
pub(crate) fn rename_exclusive(from: &Path, to: &Path) -> Result<(), BundleRenameError> {
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        from,
        rustix::fs::CWD,
        to,
        rustix::fs::RenameFlags::NOREPLACE,
    )
    .map_err(|errno| io_error("rename", from, errno.into()))
}

/// Creates a link at `link` to the relative `target` and hides the link
/// itself from the Finder.
pub(crate) fn leave_hidden_link(link: &Path, target: &Path) -> Result<(), BundleRenameError> {
    std::os::unix::fs::symlink(target, link).map_err(|source| io_error("link", link, source))?;
    let path = CString::new(link.as_os_str().as_bytes())
        .map_err(|_| io_error("hide", link, io::ErrorKind::InvalidInput.into()))?;
    let status = unsafe { libc::lchflags(path.as_ptr(), libc::UF_HIDDEN) };
    if status == 0 {
        Ok(())
    } else {
        Err(io_error("hide", link, io::Error::last_os_error()))
    }
}

/// What the launch does once the plan has run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Execution {
    /// Carry on starting from this bundle.
    Continue,
    /// Open this bundle and end this launch.
    Relaunch(PathBuf),
}

fn rename_bundle(
    rename: &RenamePlan,
    host: &impl BundleHost,
) -> Result<StepCompletion, BundleRenameError> {
    if rename.trash_older {
        host.trash(&rename.to)?;
    }
    match rename.privileges {
        Privileges::Administrator => host.rename_as_administrator(&rename.from, &rename.to),
        Privileges::User => {
            rename_exclusive(&rename.from, &rename.to)?;
            Ok(StepCompletion::Done)
        }
    }
}

/// Leaves the hidden link to `bundle` at `link`, or finds it there already.
fn restore_link(link: &Path, bundle: &Path) -> Result<StepCompletion, BundleRenameError> {
    let target = link_target(bundle)?;
    if std::fs::read_link(link).is_ok_and(|existing| existing == target) {
        return Ok(StepCompletion::Done);
    }
    leave_hidden_link(link, target)?;
    Ok(StepCompletion::Done)
}

fn is_real_folder(path: &Path) -> bool {
    path.symlink_metadata()
        .is_ok_and(|metadata| metadata.is_dir())
}

fn is_absent(path: &Path) -> bool {
    path.symlink_metadata()
        .is_err_and(|error| error.kind() == io::ErrorKind::NotFound)
}

/// Carries out `plan` for app `version`, recording each step's outcome in the
/// record in `data_dir`: the move under the rename's entry and the hidden
/// link under its own.
///
/// A rename that moved the bundle relaunches from the new name even when the
/// link or the registration after it failed: the old path no longer holds a
/// bundle to carry on from.
pub(crate) fn execute_plan(
    plan: &BundlePlan,
    host: &impl BundleHost,
    data_dir: &Path,
    version: &str,
) -> Execution {
    match plan {
        BundlePlan::OpenCanonical(bundle) | BundlePlan::OpenSibling(bundle) => {
            Execution::Relaunch(bundle.clone())
        }
        BundlePlan::Rename(rename) => {
            run_migration_step(data_dir, MigrationStep::BundleRename, version, || {
                rename_bundle(rename, host)
            });
            if is_real_folder(&rename.from) || !is_real_folder(&rename.to) {
                return Execution::Continue;
            }
            run_migration_step(data_dir, MigrationStep::BundleLink, version, || {
                restore_link(&rename.from, &rename.to)
            });
            if let Err(error) = host.register(&rename.to) {
                warn!(operation = "register bundle", version, %error);
            }
            Execution::Relaunch(rename.to.clone())
        }
        BundlePlan::ReplaceLegacy { legacy, bundle } => {
            run_migration_step(data_dir, MigrationStep::BundleRename, version, || {
                host.trash(legacy).map(|()| StepCompletion::Done)
            });
            if is_absent(legacy) {
                run_migration_step(data_dir, MigrationStep::BundleLink, version, || {
                    restore_link(legacy, bundle)
                });
            }
            Execution::Continue
        }
        BundlePlan::RestoreLink { link, bundle } => {
            run_migration_step(data_dir, MigrationStep::BundleLink, version, || {
                restore_link(link, bundle)
            });
            Execution::Continue
        }
    }
}

/// What the per-user steps need from the launch that runs them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PerUserSteps {
    bundle: PathBuf,
    version: String,
    data_dir: PathBuf,
}

fn per_user_steps(facts: &BundleFacts, data_dir: &Path) -> Option<PerUserSteps> {
    if !per_user_steps_apply(facts) {
        return None;
    }
    Some(PerUserSteps {
        bundle: facts.bundle.clone()?,
        version: facts.running.as_ref()?.version.clone(),
        data_dir: data_dir.to_path_buf(),
    })
}

/// Plans and carries out the rename for the launch started as `exe`, and
/// whether the per-user steps run once the app is set up.
pub(crate) fn plan_and_execute(
    exe: &Path,
    data_dir: &Path,
    skip_requested: bool,
    host: &impl BundleHost,
) -> (Execution, Option<PerUserSteps>) {
    let facts = gather_facts(exe, read_name_migration_record(data_dir), skip_requested);
    let per_user = per_user_steps(&facts, data_dir);
    let plan = match plan_bundle_rename(&facts) {
        Ok(plan) => plan,
        Err(reason) => {
            info!(%reason, "the bundle stays where it is");
            return (Execution::Continue, per_user);
        }
    };
    info!(?plan, "bundle rename");
    let version = facts
        .running
        .as_ref()
        .map_or(env!("CARGO_PKG_VERSION"), |running| {
            running.version.as_str()
        });
    (execute_plan(&plan, host, data_dir, version), per_user)
}

/// Runs the bundle rename at launch, in a release build, with the data folder
/// `jotra_dir` locked by `lock`.
///
/// A plan that opens another bundle releases the lock first, so the new
/// process does not hand its launch back to this one, and ends the process
/// once `open` has answered. When `open` fails the lock is claimed again and
/// the launch carries on here.
pub(crate) fn rename_at_launch(
    jotra_dir: &Path,
    lock: Option<InstanceLock>,
    log_guard: &mut LogGuard,
) -> (Option<InstanceLock>, Option<PerUserSteps>) {
    if cfg!(debug_assertions) {
        return (lock, None);
    }
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(error) => {
            warn!(operation = "bundle rename", %error, "the executable's path is unknown");
            return (lock, None);
        }
    };
    let skip = is_rename_skipped(std::env::var_os(SKIP_VAR).as_deref());
    let host = MacHost;
    let (execution, per_user) = plan_and_execute(&exe, jotra_dir, skip, &host);
    let Execution::Relaunch(bundle) = execution else {
        return (lock, per_user);
    };
    let env = relaunch_environment(std::env::vars_os());
    let args = relaunch_arguments(std::env::args_os().skip(1));
    match relaunch(&host, &bundle, &env, &args, lock, || {
        reclaim_lock(jotra_dir)
    }) {
        RelaunchOutcome::Opened => {
            info!(bundle = %bundle.display(), "opened the bundle; this launch ends");
            log_guard.flush();
            std::process::exit(0);
        }
        RelaunchOutcome::CarriedOn(lock) => (lock, per_user),
    }
}

/// How a relaunch ended for the launch that asked for it.
pub(crate) enum RelaunchOutcome {
    /// The bundle opened; this launch ends.
    Opened,
    /// The bundle did not open; the launch carries on here with this lock.
    CarriedOn(Option<InstanceLock>),
}

/// Opens `bundle` with `env` and `args`, releasing `lock` first so the new
/// process does not hand its launch back to this one. When `open` fails and
/// the lock was held, `reclaim` claims it again.
pub(crate) fn relaunch(
    host: &impl BundleHost,
    bundle: &Path,
    env: &[(OsString, OsString)],
    args: &[OsString],
    lock: Option<InstanceLock>,
    reclaim: impl FnOnce() -> Option<InstanceLock>,
) -> RelaunchOutcome {
    let held = lock.is_some();
    drop(lock);
    match host.open(bundle, env, args) {
        Ok(()) => RelaunchOutcome::Opened,
        Err(error) => {
            warn!(operation = "open bundle", %error, "the launch carries on here");
            RelaunchOutcome::CarriedOn(if held { reclaim() } else { None })
        }
    }
}

fn reclaim_lock(jotra_dir: &Path) -> Option<InstanceLock> {
    crate::instance::claim_or_hand_over(jotra_dir)
        .inspect_err(|error| warn!(%error, "running without the data folder lock"))
        .ok()
}

/// What sits at `agent`, the login agent's path under the old name, judged by
/// the program it starts. An agent that does not read is not the app's own.
fn inspect_login_agent(agent: &Path, app_binary_name: Option<&OsStr>) -> LegacyLoginAgent {
    match agent.symlink_metadata() {
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => return LegacyLoginAgent::Absent,
        Err(_) => return LegacyLoginAgent::Foreign,
    }
    let program = plist::Value::from_file(agent)
        .ok()
        .as_ref()
        .and_then(login_agent_program);
    match program {
        Some(program) if is_own_login_agent_program(&program, app_binary_name) => {
            LegacyLoginAgent::Own
        }
        _ => LegacyLoginAgent::Foreign,
    }
}

/// The program a launch agent starts: its `Program`, or else the first of its
/// `ProgramArguments`.
fn login_agent_program(agent: &plist::Value) -> Option<PathBuf> {
    let dictionary = agent.as_dictionary()?;
    dictionary
        .get("Program")
        .and_then(plist::Value::as_string)
        .or_else(|| {
            dictionary
                .get("ProgramArguments")
                .and_then(plist::Value::as_array)
                .and_then(|arguments| arguments.first())
                .and_then(plist::Value::as_string)
        })
        .map(PathBuf::from)
}

/// Removes the login item made under the old name when it starts this app
/// and, when the config asks for one, registers it under the new name. An
/// agent at the old name that starts something else stays, as a decline.
pub(crate) fn migrate_login_item(
    login: &impl LoginItem,
    legacy_agent: &Path,
    launch_at_login: bool,
    app_binary_name: Option<&OsStr>,
) -> Result<StepCompletion, BundleRenameError> {
    let plan = plan_login_item(
        inspect_login_agent(legacy_agent, app_binary_name),
        launch_at_login,
    );
    if plan.remove_legacy_agent {
        match std::fs::remove_file(legacy_agent) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(source) => return Err(io_error("remove", legacy_agent, source)),
        }
    }
    if plan.register {
        login.set(true)?;
    }
    if plan.keep_foreign_agent {
        return Ok(StepCompletion::Declined);
    }
    Ok(StepCompletion::Done)
}

/// Points `link` at `source`, failing when `source` is not a file or the link
/// does not lead to one afterwards.
fn link_command(source: &Path, link: &Path) -> Result<(), BundleRenameError> {
    if !source.is_file() {
        return Err(BundleRenameError::Unresolved {
            path: source.to_path_buf(),
        });
    }
    link_directly(source, link).map_err(|error| io_error("link", link, error))?;
    if link.is_file() {
        Ok(())
    } else {
        Err(BundleRenameError::Unresolved {
            path: link.to_path_buf(),
        })
    }
}

/// Points the command's legacy name in `folder` at `bundle` and adds the
/// command's own name beside it, when the legacy name is Jotra's command, a
/// link left dangling by the rename among them, and the folder is writable
/// without a prompt. A folder that needs a password is a decline: the install
/// in Settings asks for it. So is a bundle the rename stays out of, which no
/// link is pointed into.
pub(crate) fn migrate_command_links(
    folder: &Path,
    bundle: &Path,
    app_binary_name: Option<&OsStr>,
) -> Result<StepCompletion, BundleRenameError> {
    let legacy = folder.join(legacy_names::COMMAND_NAME);
    let current = folder.join(COMMAND_NAME);
    let facts = CommandLinkFacts {
        legacy: inspect_entry(&legacy, app_binary_name),
        current: inspect_entry(&current, app_binary_name),
        folder_writable: dir_is_writable(folder),
        bundle: bundle.to_path_buf(),
        bundle_read_only: is_read_only_volume(bundle),
    };
    match plan_command_links(&facts) {
        CommandLinkPlan::BundleGuarded | CommandLinkPlan::NeedsPermission => {
            Ok(StepCompletion::Declined)
        }
        CommandLinkPlan::Untouched => Ok(StepCompletion::Done),
        CommandLinkPlan::Relink { add_current } => {
            let command = executable_in_bundle(bundle, COMMAND_NAME);
            let legacy_command = executable_in_bundle(bundle, legacy_names::COMMAND_NAME);
            let legacy_source = if legacy_command.is_file() {
                &legacy_command
            } else {
                &command
            };
            link_command(legacy_source, &legacy)?;
            if add_current {
                link_command(&command, &current)?;
            }
            Ok(StepCompletion::Done)
        }
    }
}

/// Runs the login item and command links steps, each once per the record.
pub(crate) fn run_per_user_steps(
    steps: &PerUserSteps,
    login: &impl LoginItem,
    home: Option<&Path>,
    launch_at_login: bool,
    command_folder: &Path,
    app_binary_name: Option<&OsStr>,
) -> (Option<StepRun>, StepRun) {
    let login_run = match home {
        Some(home) => {
            let legacy_agent = home
                .join(LAUNCH_AGENTS_FOLDER)
                .join(legacy_names::LOGIN_AGENT_FILE);
            Some(run_migration_step(
                &steps.data_dir,
                MigrationStep::LoginItem,
                &steps.version,
                || migrate_login_item(login, &legacy_agent, launch_at_login, app_binary_name),
            ))
        }
        None => {
            warn!(
                step = MigrationStep::LoginItem.key(),
                "the home folder is unknown"
            );
            None
        }
    };
    let links_run = run_migration_step(
        &steps.data_dir,
        MigrationStep::CommandLinks,
        &steps.version,
        || migrate_command_links(command_folder, &steps.bundle, app_binary_name),
    );
    (login_run, links_run)
}

/// Runs [`run_per_user_steps`] off the main thread once the app is set up.
pub(crate) fn spawn_per_user_steps(handle: &AppHandle, steps: PerUserSteps) {
    let handle = handle.clone();
    std::thread::spawn(move || {
        let launch_at_login = recover_poison(
            handle.state::<AppState>().config.lock(),
            "bundle rename per-user steps",
        )
        .general
        .launch_at_login;
        let login = handle.autolaunch();
        let home = dirs::home_dir();
        let exe = std::env::current_exe().ok();
        let command_folder = Path::new(SYSTEM_LINK)
            .parent()
            .unwrap_or_else(|| Path::new("/"));
        run_per_user_steps(
            &steps,
            login.inner(),
            home.as_deref(),
            launch_at_login,
            command_folder,
            exe.as_deref().and_then(Path::file_name),
        );
    });
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::os::macos::fs::MetadataExt;
    use std::os::unix::fs::PermissionsExt;

    use jotra_core::bundle_rename::{BUNDLE_FOLDER, BUNDLE_IDENTIFIER};
    use jotra_core::cli_install::{InstalledEntry, CLI_SIGNATURE};
    use jotra_core::name_migration::StepOutcome;
    use tempfile::TempDir;

    use super::*;

    const VERSION: &str = "1.1.0";
    const APP_BINARY: &str = "jotra-app";

    struct Folder {
        _dir: TempDir,
        root: PathBuf,
    }

    fn folder() -> Folder {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().canonicalize().expect("canonical temp dir");
        Folder { _dir: dir, root }
    }

    fn info_plist(identifier: &str, version: &str) -> String {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
             <plist version=\"1.0\"><dict>\
             <key>CFBundleIdentifier</key><string>{identifier}</string>\
             <key>CFBundleShortVersionString</key><string>{version}</string>\
             </dict></plist>\n"
        )
    }

    fn make_bundle(path: &Path, identifier: &str, version: &str) -> PathBuf {
        let exe = executable_in_bundle(path, APP_BINARY);
        std::fs::create_dir_all(exe.parent().expect("executables folder")).expect("bundle");
        std::fs::write(&exe, b"app").expect("executable");
        std::fs::write(info_plist_path(path), info_plist(identifier, version)).expect("plist");
        exe
    }

    fn legacy_bundle(root: &Path) -> PathBuf {
        root.join(legacy_names::BUNDLE_FOLDER)
    }

    fn current_bundle(root: &Path) -> PathBuf {
        root.join(BUNDLE_FOLDER)
    }

    fn entry(data_dir: &Path, step: MigrationStep) -> Option<StepOutcome> {
        read_name_migration_record(data_dir)
            .entry(step)
            .map(|entry| entry.outcome)
    }

    fn is_hidden_link(path: &Path) -> bool {
        let metadata = path.symlink_metadata().expect("link metadata");
        metadata.file_type().is_symlink() && metadata.st_flags() & libc::UF_HIDDEN != 0
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Admin {
        Perform,
        /// Moves the bundle, leaves no link and leaves the folder read-only.
        MoveAndLock,
        Decline,
        Fail,
    }

    type Opened = (PathBuf, Vec<(OsString, OsString)>, Vec<OsString>);

    struct FakeHost {
        trash_folder: PathBuf,
        fail_trash: bool,
        fail_register: bool,
        fail_open: bool,
        admin: Admin,
        lock_dir: Option<PathBuf>,
        trashed: RefCell<Vec<PathBuf>>,
        registered: RefCell<Vec<PathBuf>>,
        administrator_renames: RefCell<usize>,
        opened: RefCell<Vec<Opened>>,
        lock_free_at_open: RefCell<Vec<bool>>,
    }

    impl FakeHost {
        fn new(root: &Path) -> Self {
            let trash_folder = root.join("Trash");
            std::fs::create_dir_all(&trash_folder).expect("trash");
            Self {
                trash_folder,
                fail_trash: false,
                fail_register: false,
                fail_open: false,
                admin: Admin::Perform,
                lock_dir: None,
                trashed: RefCell::new(Vec::new()),
                registered: RefCell::new(Vec::new()),
                administrator_renames: RefCell::new(0),
                opened: RefCell::new(Vec::new()),
                lock_free_at_open: RefCell::new(Vec::new()),
            }
        }
    }

    impl BundleHost for FakeHost {
        fn trash(&self, path: &Path) -> Result<(), BundleRenameError> {
            if self.fail_trash {
                return Err(BundleRenameError::Trash {
                    path: path.to_path_buf(),
                    reason: "refused".to_string(),
                });
            }
            let name = path.file_name().expect("trashed name");
            std::fs::rename(path, self.trash_folder.join(name))
                .map_err(|source| io_error("trash", path, source))?;
            self.trashed.borrow_mut().push(path.to_path_buf());
            Ok(())
        }

        fn rename_as_administrator(
            &self,
            from: &Path,
            to: &Path,
        ) -> Result<StepCompletion, BundleRenameError> {
            *self.administrator_renames.borrow_mut() += 1;
            match self.admin {
                Admin::Perform => {
                    rename_exclusive(from, to)?;
                    leave_hidden_link(from, link_target(to)?)?;
                    Ok(StepCompletion::Done)
                }
                Admin::MoveAndLock => {
                    rename_exclusive(from, to)?;
                    let folder = to.parent().expect("folder");
                    std::fs::set_permissions(folder, std::fs::Permissions::from_mode(0o555))
                        .map_err(|source| io_error("lock", folder, source))?;
                    Ok(StepCompletion::Done)
                }
                Admin::Decline => Ok(StepCompletion::Declined),
                Admin::Fail => Err(BundleRenameError::Administrator("refused".to_string())),
            }
        }

        fn register(&self, bundle: &Path) -> Result<(), BundleRenameError> {
            self.registered.borrow_mut().push(bundle.to_path_buf());
            if self.fail_register {
                return Err(BundleRenameError::Register {
                    path: bundle.to_path_buf(),
                    status: -10814,
                });
            }
            Ok(())
        }

        fn open(
            &self,
            bundle: &Path,
            env: &[(OsString, OsString)],
            args: &[OsString],
        ) -> Result<(), BundleRenameError> {
            if let Some(lock_dir) = &self.lock_dir {
                let free = jotra_storage::instance::acquire(lock_dir).is_ok();
                self.lock_free_at_open.borrow_mut().push(free);
            }
            self.opened
                .borrow_mut()
                .push((bundle.to_path_buf(), env.to_vec(), args.to_vec()));
            if self.fail_open {
                return Err(BundleRenameError::Open {
                    path: bundle.to_path_buf(),
                    reason: "refused".to_string(),
                });
            }
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakeLogin {
        fail: bool,
        set_calls: RefCell<Vec<bool>>,
    }

    impl LoginItem for FakeLogin {
        fn is_enabled(&self) -> Result<bool, LoginItemError> {
            Ok(self.set_calls.borrow().last().copied().unwrap_or(false))
        }

        fn set(&self, on: bool) -> Result<(), LoginItemError> {
            if self.fail {
                return Err(LoginItemError::Os("refused".to_string()));
            }
            self.set_calls.borrow_mut().push(on);
            Ok(())
        }
    }

    fn rename_plan(root: &Path, trash_older: bool, privileges: Privileges) -> BundlePlan {
        BundlePlan::Rename(RenamePlan {
            from: legacy_bundle(root),
            to: current_bundle(root),
            trash_older,
            privileges,
        })
    }

    #[test]
    fn a_bundles_identifier_and_version_read_from_its_info_plist() {
        let folder = folder();
        let bundle = current_bundle(&folder.root);
        make_bundle(&bundle, BUNDLE_IDENTIFIER, VERSION);
        let info = read_bundle_info(&bundle).expect("info");
        assert_eq!(info.identifier, BUNDLE_IDENTIFIER);
        assert_eq!(info.version, VERSION);
    }

    #[test]
    fn a_malformed_or_incomplete_info_plist_is_an_error() {
        let folder = folder();
        let bundle = current_bundle(&folder.root);
        make_bundle(&bundle, BUNDLE_IDENTIFIER, VERSION);
        std::fs::write(info_plist_path(&bundle), "not a plist").expect("plist");
        assert!(matches!(
            read_bundle_info(&bundle),
            Err(BundleRenameError::InfoPlist { .. })
        ));
        std::fs::write(
            info_plist_path(&bundle),
            "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict>\
             <key>CFBundleIdentifier</key><string>x</string></dict></plist>",
        )
        .expect("plist");
        let error = read_bundle_info(&bundle).expect_err("missing version");
        assert!(error.to_string().contains("CFBundleShortVersionString"));
        assert!(read_bundle_info(&folder.root.join("Missing.app")).is_err());
    }

    #[test]
    fn the_facts_describe_a_bundle_under_the_old_name() {
        let folder = folder();
        let exe = make_bundle(&legacy_bundle(&folder.root), BUNDLE_IDENTIFIER, VERSION);
        let facts = gather_facts(&exe, NameMigrationRecord::default(), true);
        assert!(!facts.exe_through_link);
        assert_eq!(facts.bundle, Some(legacy_bundle(&folder.root)));
        assert_eq!(facts.canonical_bundle, Some(legacy_bundle(&folder.root)));
        assert_eq!(
            facts.running,
            Some(BundleInfo {
                identifier: BUNDLE_IDENTIFIER.to_string(),
                version: VERSION.to_string(),
            })
        );
        assert_eq!(facts.sibling, SiblingState::Absent);
        assert!(!facts.read_only_volume);
        assert!(facts.folder_writable);
        assert!(facts.skip_requested);
    }

    #[test]
    fn the_facts_see_an_executable_reached_through_a_link() {
        let folder = folder();
        make_bundle(&current_bundle(&folder.root), BUNDLE_IDENTIFIER, VERSION);
        leave_hidden_link(&legacy_bundle(&folder.root), Path::new(BUNDLE_FOLDER)).expect("link");
        let exe = executable_in_bundle(&legacy_bundle(&folder.root), APP_BINARY);
        let facts = gather_facts(&exe, NameMigrationRecord::default(), false);
        assert!(facts.exe_through_link);
        assert_eq!(facts.canonical_bundle, Some(current_bundle(&folder.root)));
        assert_eq!(
            plan_bundle_rename(&facts),
            Ok(BundlePlan::OpenCanonical(current_bundle(&folder.root)))
        );
    }

    #[test]
    fn the_facts_tell_each_kind_of_sibling_apart() {
        let folder = folder();
        let exe = make_bundle(&current_bundle(&folder.root), BUNDLE_IDENTIFIER, VERSION);
        let legacy = legacy_bundle(&folder.root);

        std::os::unix::fs::symlink(BUNDLE_FOLDER, &legacy).expect("link");
        let facts = gather_facts(&exe, NameMigrationRecord::default(), false);
        assert_eq!(facts.sibling, SiblingState::Link);
        std::fs::remove_file(&legacy).expect("remove link");

        std::fs::write(&legacy, b"file").expect("file");
        let facts = gather_facts(&exe, NameMigrationRecord::default(), false);
        assert_eq!(facts.sibling, SiblingState::Unreadable);
        std::fs::remove_file(&legacy).expect("remove file");

        std::fs::create_dir(&legacy).expect("empty folder");
        let facts = gather_facts(&exe, NameMigrationRecord::default(), false);
        assert_eq!(facts.sibling, SiblingState::Unreadable);
        std::fs::remove_dir(&legacy).expect("remove folder");

        make_bundle(&legacy, BUNDLE_IDENTIFIER, "1.0.0");
        let facts = gather_facts(&exe, NameMigrationRecord::default(), false);
        assert_eq!(
            facts.sibling,
            SiblingState::Bundle(BundleInfo {
                identifier: BUNDLE_IDENTIFIER.to_string(),
                version: "1.0.0".to_string(),
            })
        );
    }

    #[test]
    fn the_facts_see_a_folder_that_is_not_writable() {
        let folder = folder();
        let apps = folder.root.join("Apps");
        let exe = make_bundle(&legacy_bundle(&apps), BUNDLE_IDENTIFIER, VERSION);
        std::fs::set_permissions(&apps, std::fs::Permissions::from_mode(0o555)).expect("lock");
        let facts = gather_facts(&exe, NameMigrationRecord::default(), false);
        std::fs::set_permissions(&apps, std::fs::Permissions::from_mode(0o755)).expect("unlock");
        assert!(!facts.folder_writable);
    }

    #[test]
    fn an_executable_outside_a_bundle_has_no_bundle_facts() {
        let folder = folder();
        let exe = folder.root.join(APP_BINARY);
        std::fs::write(&exe, b"app").expect("executable");
        let facts = gather_facts(&exe, NameMigrationRecord::default(), false);
        assert_eq!(facts.bundle, None);
        assert_eq!(facts.running, None);
        assert_eq!(facts.sibling, SiblingState::Absent);
        assert!(!facts.folder_writable);
    }

    #[test]
    fn an_exclusive_rename_never_replaces_what_is_there() {
        let folder = folder();
        let from = folder.root.join("a");
        let to = folder.root.join("b");
        std::fs::create_dir(&from).expect("from");
        std::fs::create_dir(&to).expect("to");
        let error = rename_exclusive(&from, &to).expect_err("refused");
        assert!(matches!(
            error,
            BundleRenameError::Io {
                operation: "rename",
                ..
            }
        ));
        assert!(from.is_dir());
        std::fs::remove_dir(&to).expect("clear");
        rename_exclusive(&from, &to).expect("renamed");
        assert!(!from.exists());
        assert!(to.is_dir());
    }

    #[test]
    fn the_link_left_behind_is_relative_and_hidden() {
        let folder = folder();
        std::fs::create_dir(current_bundle(&folder.root)).expect("bundle");
        let link = legacy_bundle(&folder.root);
        leave_hidden_link(&link, Path::new(BUNDLE_FOLDER)).expect("link");
        assert_eq!(
            std::fs::read_link(&link).expect("target"),
            PathBuf::from(BUNDLE_FOLDER)
        );
        assert!(is_hidden_link(&link));
        assert_eq!(
            link.canonicalize().expect("resolves"),
            current_bundle(&folder.root)
        );
        assert!(leave_hidden_link(&link, Path::new(BUNDLE_FOLDER)).is_err());
    }

    #[test]
    fn a_rename_moves_the_bundle_leaves_the_link_and_relaunches() {
        let folder = folder();
        let data = folder.root.join("data");
        std::fs::create_dir(&data).expect("data");
        make_bundle(&legacy_bundle(&folder.root), BUNDLE_IDENTIFIER, VERSION);
        let host = FakeHost::new(&folder.root);
        let plan = rename_plan(&folder.root, false, Privileges::User);

        let execution = execute_plan(&plan, &host, &data, VERSION);

        assert_eq!(execution, Execution::Relaunch(current_bundle(&folder.root)));
        assert!(is_real_folder(&current_bundle(&folder.root)));
        assert!(is_hidden_link(&legacy_bundle(&folder.root)));
        assert_eq!(
            std::fs::read_link(legacy_bundle(&folder.root)).expect("target"),
            PathBuf::from(BUNDLE_FOLDER)
        );
        assert_eq!(
            *host.registered.borrow(),
            vec![current_bundle(&folder.root)]
        );
        assert_eq!(
            entry(&data, MigrationStep::BundleRename),
            Some(StepOutcome::Done)
        );
        assert_eq!(
            entry(&data, MigrationStep::BundleLink),
            Some(StepOutcome::Done)
        );
    }

    #[test]
    fn a_link_that_fails_after_the_move_is_recorded_on_its_own_and_the_launch_relaunches() {
        let folder = folder();
        let apps = folder.root.join("Apps");
        let data = folder.root.join("data");
        std::fs::create_dir(&data).expect("data");
        make_bundle(&legacy_bundle(&apps), BUNDLE_IDENTIFIER, VERSION);
        let mut host = FakeHost::new(&folder.root);
        host.admin = Admin::MoveAndLock;
        let plan = rename_plan(&apps, false, Privileges::Administrator);

        let execution = execute_plan(&plan, &host, &data, VERSION);
        std::fs::set_permissions(&apps, std::fs::Permissions::from_mode(0o755)).expect("unlock");

        assert_eq!(execution, Execution::Relaunch(current_bundle(&apps)));
        assert!(is_absent(&legacy_bundle(&apps)));
        assert_eq!(*host.registered.borrow(), vec![current_bundle(&apps)]);
        assert_eq!(
            entry(&data, MigrationStep::BundleRename),
            Some(StepOutcome::Done)
        );
        let link = read_name_migration_record(&data)
            .entry(MigrationStep::BundleLink)
            .expect("link entry");
        assert_eq!(link.outcome, StepOutcome::Failed);
        assert_eq!(link.version, VERSION);
        assert!(link.reason.is_some());
    }

    #[test]
    fn an_administrator_rename_finds_its_link_and_leaves_no_second_one() {
        let folder = folder();
        let data = folder.root.join("data");
        std::fs::create_dir(&data).expect("data");
        make_bundle(&legacy_bundle(&folder.root), BUNDLE_IDENTIFIER, VERSION);
        let host = FakeHost::new(&folder.root);
        let plan = rename_plan(&folder.root, false, Privileges::Administrator);

        execute_plan(&plan, &host, &data, VERSION);

        assert!(is_hidden_link(&legacy_bundle(&folder.root)));
        assert_eq!(
            std::fs::read_link(legacy_bundle(&folder.root)).expect("target"),
            PathBuf::from(BUNDLE_FOLDER)
        );
        assert!(is_absent(&current_bundle(&folder.root).join(BUNDLE_FOLDER)));
        assert_eq!(
            entry(&data, MigrationStep::BundleLink),
            Some(StepOutcome::Done)
        );
    }

    #[test]
    fn a_link_that_did_not_land_is_left_on_a_later_launch_from_the_new_name() {
        let folder = folder();
        let data = folder.root.join("data");
        std::fs::create_dir(&data).expect("data");
        let exe = make_bundle(&current_bundle(&folder.root), BUNDLE_IDENTIFIER, VERSION);
        for (step, outcome) in [
            (MigrationStep::BundleRename, StepOutcome::Done),
            (MigrationStep::BundleLink, StepOutcome::Failed),
        ] {
            let failed = jotra_core::name_migration::StepEntry {
                outcome,
                version: "1.0.0".to_string(),
                at: chrono::Utc::now(),
                reason: None,
            };
            jotra_storage::name_migration::record_migration_step(&data, step, &failed)
                .expect("record");
        }
        let host = FakeHost::new(&folder.root);

        let (execution, _) = plan_and_execute(&exe, &data, false, &host);

        assert_eq!(execution, Execution::Continue);
        assert!(is_hidden_link(&legacy_bundle(&folder.root)));
        assert_eq!(
            legacy_bundle(&folder.root)
                .canonicalize()
                .expect("resolves"),
            current_bundle(&folder.root)
        );
        let link = read_name_migration_record(&data)
            .entry(MigrationStep::BundleLink)
            .expect("link entry");
        assert_eq!(link.outcome, StepOutcome::Done);
        assert_eq!(link.version, VERSION);
        assert!(host.registered.borrow().is_empty());
        assert!(host.trashed.borrow().is_empty());
    }

    #[test]
    fn a_link_restored_in_a_locked_folder_fails_again() {
        let folder = folder();
        let apps = folder.root.join("Apps");
        let data = folder.root.join("data");
        std::fs::create_dir(&data).expect("data");
        make_bundle(&current_bundle(&apps), BUNDLE_IDENTIFIER, VERSION);
        let host = FakeHost::new(&folder.root);
        let plan = BundlePlan::RestoreLink {
            link: legacy_bundle(&apps),
            bundle: current_bundle(&apps),
        };

        std::fs::set_permissions(&apps, std::fs::Permissions::from_mode(0o555)).expect("lock");
        let execution = execute_plan(&plan, &host, &data, VERSION);
        std::fs::set_permissions(&apps, std::fs::Permissions::from_mode(0o755)).expect("unlock");

        assert_eq!(execution, Execution::Continue);
        assert!(is_absent(&legacy_bundle(&apps)));
        assert_eq!(
            entry(&data, MigrationStep::BundleLink),
            Some(StepOutcome::Failed)
        );
    }

    #[test]
    fn a_registration_that_fails_still_relaunches_from_the_new_name() {
        let folder = folder();
        let data = folder.root.join("data");
        std::fs::create_dir(&data).expect("data");
        make_bundle(&legacy_bundle(&folder.root), BUNDLE_IDENTIFIER, VERSION);
        let mut host = FakeHost::new(&folder.root);
        host.fail_register = true;
        let plan = rename_plan(&folder.root, false, Privileges::User);

        assert_eq!(
            execute_plan(&plan, &host, &data, VERSION),
            Execution::Relaunch(current_bundle(&folder.root))
        );
        assert_eq!(
            *host.registered.borrow(),
            vec![current_bundle(&folder.root)]
        );
        assert_eq!(
            entry(&data, MigrationStep::BundleRename),
            Some(StepOutcome::Done)
        );
    }

    #[test]
    fn a_rename_that_stays_put_registers_nothing() {
        let folder = folder();
        let data = folder.root.join("data");
        std::fs::create_dir(&data).expect("data");
        make_bundle(&legacy_bundle(&folder.root), BUNDLE_IDENTIFIER, VERSION);
        let mut host = FakeHost::new(&folder.root);
        host.admin = Admin::Decline;
        let plans = [
            rename_plan(&folder.root, false, Privileges::Administrator),
            BundlePlan::OpenCanonical(current_bundle(&folder.root)),
            BundlePlan::OpenSibling(current_bundle(&folder.root)),
        ];
        for plan in &plans {
            execute_plan(plan, &host, &data, VERSION);
        }
        assert!(host.registered.borrow().is_empty());
    }

    #[test]
    fn an_older_bundle_at_the_new_name_goes_to_the_trash_first() {
        let folder = folder();
        let data = folder.root.join("data");
        std::fs::create_dir(&data).expect("data");
        make_bundle(&legacy_bundle(&folder.root), BUNDLE_IDENTIFIER, VERSION);
        make_bundle(&current_bundle(&folder.root), BUNDLE_IDENTIFIER, "1.0.0");
        let host = FakeHost::new(&folder.root);
        let plan = rename_plan(&folder.root, true, Privileges::User);

        let execution = execute_plan(&plan, &host, &data, VERSION);

        assert_eq!(execution, Execution::Relaunch(current_bundle(&folder.root)));
        assert_eq!(*host.trashed.borrow(), vec![current_bundle(&folder.root)]);
        assert_eq!(
            read_bundle_info(&current_bundle(&folder.root))
                .expect("info")
                .version,
            VERSION
        );
        assert!(is_hidden_link(&legacy_bundle(&folder.root)));
    }

    #[test]
    fn a_rename_that_finds_the_new_name_taken_fails_and_carries_on() {
        let folder = folder();
        let data = folder.root.join("data");
        std::fs::create_dir(&data).expect("data");
        make_bundle(&legacy_bundle(&folder.root), BUNDLE_IDENTIFIER, VERSION);
        std::fs::create_dir(current_bundle(&folder.root)).expect("taken");
        let host = FakeHost::new(&folder.root);
        let plan = rename_plan(&folder.root, false, Privileges::User);

        let execution = execute_plan(&plan, &host, &data, VERSION);

        assert_eq!(execution, Execution::Continue);
        assert!(is_real_folder(&legacy_bundle(&folder.root)));
        assert!(host.registered.borrow().is_empty());
        let recorded = read_name_migration_record(&data)
            .entry(MigrationStep::BundleRename)
            .expect("entry");
        assert_eq!(recorded.outcome, StepOutcome::Failed);
        assert_eq!(recorded.version, VERSION);
        assert!(recorded.reason.is_some());
    }

    #[test]
    fn a_trash_that_fails_leaves_both_bundles() {
        let folder = folder();
        let data = folder.root.join("data");
        std::fs::create_dir(&data).expect("data");
        make_bundle(&legacy_bundle(&folder.root), BUNDLE_IDENTIFIER, VERSION);
        make_bundle(&current_bundle(&folder.root), BUNDLE_IDENTIFIER, "1.0.0");
        let mut host = FakeHost::new(&folder.root);
        host.fail_trash = true;
        let plan = rename_plan(&folder.root, true, Privileges::User);

        assert_eq!(
            execute_plan(&plan, &host, &data, VERSION),
            Execution::Continue
        );
        assert!(is_real_folder(&legacy_bundle(&folder.root)));
        assert!(is_real_folder(&current_bundle(&folder.root)));
        assert_eq!(
            entry(&data, MigrationStep::BundleRename),
            Some(StepOutcome::Failed)
        );
    }

    #[test]
    fn a_declined_administrator_prompt_is_recorded_and_the_launch_carries_on() {
        let folder = folder();
        let data = folder.root.join("data");
        std::fs::create_dir(&data).expect("data");
        make_bundle(&legacy_bundle(&folder.root), BUNDLE_IDENTIFIER, VERSION);
        let mut host = FakeHost::new(&folder.root);
        host.admin = Admin::Decline;
        let plan = rename_plan(&folder.root, false, Privileges::Administrator);

        assert_eq!(
            execute_plan(&plan, &host, &data, VERSION),
            Execution::Continue
        );
        assert_eq!(*host.administrator_renames.borrow(), 1);
        assert!(is_real_folder(&legacy_bundle(&folder.root)));
        assert_eq!(
            entry(&data, MigrationStep::BundleRename),
            Some(StepOutcome::Declined)
        );
    }

    #[test]
    fn a_failed_administrator_rename_is_recorded_as_failed() {
        let folder = folder();
        let data = folder.root.join("data");
        std::fs::create_dir(&data).expect("data");
        make_bundle(&legacy_bundle(&folder.root), BUNDLE_IDENTIFIER, VERSION);
        let mut host = FakeHost::new(&folder.root);
        host.admin = Admin::Fail;
        let plan = rename_plan(&folder.root, false, Privileges::Administrator);

        assert_eq!(
            execute_plan(&plan, &host, &data, VERSION),
            Execution::Continue
        );
        assert_eq!(
            entry(&data, MigrationStep::BundleRename),
            Some(StepOutcome::Failed)
        );
    }

    #[test]
    fn an_administrator_rename_relaunches_from_the_new_name() {
        let folder = folder();
        let data = folder.root.join("data");
        std::fs::create_dir(&data).expect("data");
        make_bundle(&legacy_bundle(&folder.root), BUNDLE_IDENTIFIER, VERSION);
        let host = FakeHost::new(&folder.root);
        let plan = rename_plan(&folder.root, false, Privileges::Administrator);

        assert_eq!(
            execute_plan(&plan, &host, &data, VERSION),
            Execution::Relaunch(current_bundle(&folder.root))
        );
        assert_eq!(
            *host.registered.borrow(),
            vec![current_bundle(&folder.root)]
        );
        assert_eq!(
            entry(&data, MigrationStep::BundleRename),
            Some(StepOutcome::Done)
        );
    }

    #[test]
    fn an_old_name_bundle_beside_the_new_one_is_replaced_by_the_link() {
        let folder = folder();
        let data = folder.root.join("data");
        std::fs::create_dir(&data).expect("data");
        make_bundle(&current_bundle(&folder.root), BUNDLE_IDENTIFIER, VERSION);
        make_bundle(&legacy_bundle(&folder.root), BUNDLE_IDENTIFIER, "1.0.0");
        let host = FakeHost::new(&folder.root);
        let plan = BundlePlan::ReplaceLegacy {
            legacy: legacy_bundle(&folder.root),
            bundle: current_bundle(&folder.root),
        };

        assert_eq!(
            execute_plan(&plan, &host, &data, VERSION),
            Execution::Continue
        );
        assert_eq!(*host.trashed.borrow(), vec![legacy_bundle(&folder.root)]);
        assert!(is_hidden_link(&legacy_bundle(&folder.root)));
        assert_eq!(
            entry(&data, MigrationStep::BundleRename),
            Some(StepOutcome::Done)
        );
        assert_eq!(
            entry(&data, MigrationStep::BundleLink),
            Some(StepOutcome::Done)
        );
        assert!(host.registered.borrow().is_empty());
    }

    #[test]
    fn a_replacement_whose_trash_fails_leaves_the_old_bundle() {
        let folder = folder();
        let data = folder.root.join("data");
        std::fs::create_dir(&data).expect("data");
        make_bundle(&current_bundle(&folder.root), BUNDLE_IDENTIFIER, VERSION);
        make_bundle(&legacy_bundle(&folder.root), BUNDLE_IDENTIFIER, "1.0.0");
        let mut host = FakeHost::new(&folder.root);
        host.fail_trash = true;
        let plan = BundlePlan::ReplaceLegacy {
            legacy: legacy_bundle(&folder.root),
            bundle: current_bundle(&folder.root),
        };

        assert_eq!(
            execute_plan(&plan, &host, &data, VERSION),
            Execution::Continue
        );
        assert!(is_real_folder(&legacy_bundle(&folder.root)));
        assert_eq!(
            entry(&data, MigrationStep::BundleRename),
            Some(StepOutcome::Failed)
        );
        assert_eq!(entry(&data, MigrationStep::BundleLink), None);
    }

    #[test]
    fn opening_another_bundle_records_nothing() {
        let folder = folder();
        let host = FakeHost::new(&folder.root);
        for plan in [
            BundlePlan::OpenCanonical(current_bundle(&folder.root)),
            BundlePlan::OpenSibling(current_bundle(&folder.root)),
        ] {
            assert_eq!(
                execute_plan(&plan, &host, &folder.root, VERSION),
                Execution::Relaunch(current_bundle(&folder.root))
            );
        }
        assert!(read_name_migration_record(&folder.root)
            .entry(MigrationStep::BundleRename)
            .is_none());
    }

    #[test]
    fn a_launch_from_the_old_name_renames_once() {
        let folder = folder();
        let data = folder.root.join("data");
        std::fs::create_dir(&data).expect("data");
        let exe = make_bundle(&legacy_bundle(&folder.root), BUNDLE_IDENTIFIER, VERSION);
        let host = FakeHost::new(&folder.root);

        let (execution, per_user) = plan_and_execute(&exe, &data, false, &host);
        assert_eq!(execution, Execution::Relaunch(current_bundle(&folder.root)));
        assert_eq!(per_user, None);
        assert_eq!(
            *host.registered.borrow(),
            vec![current_bundle(&folder.root)]
        );

        let exe = executable_in_bundle(&current_bundle(&folder.root), APP_BINARY);
        let (execution, per_user) = plan_and_execute(&exe, &data, false, &host);
        assert_eq!(execution, Execution::Continue);
        assert_eq!(host.registered.borrow().len(), 1);
        assert_eq!(
            per_user,
            Some(PerUserSteps {
                bundle: current_bundle(&folder.root),
                version: VERSION.to_string(),
                data_dir: data.clone(),
            })
        );
    }

    #[test]
    fn a_launch_with_the_skip_variable_leaves_the_old_name() {
        let folder = folder();
        let exe = make_bundle(&legacy_bundle(&folder.root), BUNDLE_IDENTIFIER, VERSION);
        let host = FakeHost::new(&folder.root);
        let (execution, per_user) = plan_and_execute(&exe, &folder.root, true, &host);
        assert_eq!(execution, Execution::Continue);
        assert_eq!(per_user, None);
        assert!(is_real_folder(&legacy_bundle(&folder.root)));
    }

    #[test]
    fn open_starts_a_new_instance_and_passes_arguments_only_when_there_are_some() {
        let bundle = Path::new("/Applications").join(BUNDLE_FOLDER);
        assert_eq!(
            open_arguments(&bundle, &[], &[]),
            vec![
                OsString::from("-n"),
                OsString::from("-a"),
                bundle.clone().into_os_string(),
            ]
        );
        let args = [OsString::from("/tmp/a b.md")];
        assert_eq!(
            open_arguments(&bundle, &[], &args),
            vec![
                OsString::from("-n"),
                OsString::from("-a"),
                bundle.clone().into_os_string(),
                OsString::from("--args"),
                OsString::from("/tmp/a b.md"),
            ]
        );
    }

    #[test]
    fn open_hands_each_variable_to_the_new_instance_whole() {
        let bundle = Path::new("/Applications").join(BUNDLE_FOLDER);
        let env = relaunch_environment([
            (
                OsString::from("JOTRA_DATA_DIR"),
                OsString::from("/Users/a b/x=y"),
            ),
            (OsString::from("PATH"), OsString::from("/usr/bin")),
            (
                OsString::from(legacy_names::NOTES_DIR_VAR),
                OsString::from(""),
            ),
        ]);
        assert_eq!(
            open_arguments(&bundle, &env, &[OsString::from("note.md")]),
            vec![
                OsString::from("-n"),
                OsString::from("--env"),
                OsString::from("JOTRA_DATA_DIR=/Users/a b/x=y"),
                OsString::from("--env"),
                OsString::from(format!("{}=", legacy_names::NOTES_DIR_VAR)),
                OsString::from("-a"),
                bundle.clone().into_os_string(),
                OsString::from("--args"),
                OsString::from("note.md"),
            ]
        );
    }

    #[test]
    fn the_administrator_script_quotes_both_paths_for_the_shell_and_applescript() {
        let from = Path::new("/Apps/O'Brien \"x\"").join(legacy_names::BUNDLE_FOLDER);
        let to = Path::new("/Apps/O'Brien \"x\"").join(BUNDLE_FOLDER);
        let script = administrator_rename_script(&from, &to).expect("script");
        assert!(script.starts_with("do shell script \""));
        assert!(script.ends_with("\" with administrator privileges"));
        assert!(script.contains("/bin/mv "));
        assert!(script.contains("/bin/ln -s 'Jotra.app' "));
        assert!(script.contains("/usr/bin/chflags -h hidden "));
        assert!(script.contains("O'\\\\''Brien \\\"x\\\""));
        assert!(administrator_rename_script(&from, Path::new("/")).is_err());
    }

    /// The text of the AppleScript string literal in `do shell script "…"`,
    /// read the way AppleScript reads it. Any quote the script does not escape
    /// ends the literal early and fails the test.
    fn shell_text_of(script: &str) -> String {
        let body = script
            .strip_prefix("do shell script \"")
            .and_then(|rest| rest.strip_suffix("\" with administrator privileges"))
            .expect("script shape");
        let mut text = String::new();
        let mut chars = body.chars();
        while let Some(character) = chars.next() {
            match character {
                '\\' => match chars.next() {
                    Some(escaped @ ('\\' | '"')) => text.push(escaped),
                    other => panic!("unexpected escape {other:?} in {script}"),
                },
                '"' => panic!("an unescaped quote ends the literal early: {script}"),
                other => text.push(other),
            }
        }
        text
    }

    fn run_shell(text: &str, cwd: &Path) -> std::process::ExitStatus {
        Command::new("/bin/sh")
            .arg("-c")
            .arg(text)
            .current_dir(cwd)
            .status()
            .expect("sh")
    }

    #[test]
    fn the_administrator_script_runs_on_paths_with_quotes_slashes_and_substitutions() {
        let folder = folder();
        let apps = folder
            .root
            .join("a \"b\" 'c' \\d $(touch pwned) `touch tick`");
        make_bundle(&legacy_bundle(&apps), BUNDLE_IDENTIFIER, VERSION);
        let script = administrator_rename_script(&legacy_bundle(&apps), &current_bundle(&apps))
            .expect("script");

        let status = run_shell(&shell_text_of(&script), &folder.root);

        assert!(status.success(), "{status}");
        assert!(is_real_folder(&current_bundle(&apps)));
        assert!(info_plist_path(&current_bundle(&apps)).is_file());
        assert!(is_hidden_link(&legacy_bundle(&apps)));
        assert_eq!(
            std::fs::read_link(legacy_bundle(&apps)).expect("target"),
            PathBuf::from(BUNDLE_FOLDER)
        );
        for place in [&folder.root, &apps] {
            assert!(!place.join("pwned").exists());
            assert!(!place.join("tick").exists());
        }
    }

    #[test]
    fn the_administrator_script_moves_nothing_when_the_new_name_is_taken() {
        let folder = folder();
        make_bundle(&legacy_bundle(&folder.root), BUNDLE_IDENTIFIER, VERSION);
        std::fs::create_dir(current_bundle(&folder.root)).expect("taken");
        let script = administrator_rename_script(
            &legacy_bundle(&folder.root),
            &current_bundle(&folder.root),
        )
        .expect("script");

        let status = run_shell(&shell_text_of(&script), &folder.root);

        assert!(!status.success());
        assert!(is_real_folder(&legacy_bundle(&folder.root)));
        assert!(is_absent(
            &current_bundle(&folder.root).join(legacy_names::BUNDLE_FOLDER)
        ));
    }

    #[test]
    fn a_move_into_a_folder_made_at_the_new_name_after_the_check_is_undone() {
        let folder = folder();
        let elsewhere = folder.root.join("elsewhere");
        std::fs::create_dir(&elsewhere).expect("elsewhere");
        let make_targets: [fn(&Path, &Path); 2] = [
            |to, _| std::fs::create_dir(to).expect("folder"),
            |to, elsewhere| std::os::unix::fs::symlink(elsewhere, to).expect("link to a folder"),
        ];
        for make_target in make_targets {
            let from = legacy_bundle(&folder.root);
            let to = current_bundle(&folder.root);
            make_bundle(&from, BUNDLE_IDENTIFIER, VERSION);
            make_target(&to, &elsewhere);

            let shell = move_and_link_shell(&from, &to).expect("shell");
            let status = run_shell(&shell, &folder.root);

            assert!(!status.success());
            assert!(is_real_folder(&from));
            assert!(info_plist_path(&from).is_file());
            assert!(is_absent(&to.join(legacy_names::BUNDLE_FOLDER)));
            assert!(is_absent(&elsewhere.join(legacy_names::BUNDLE_FOLDER)));

            std::fs::remove_dir_all(&from).expect("clear bundle");
            if to.is_symlink() {
                std::fs::remove_file(&to).expect("clear link");
            } else {
                std::fs::remove_dir(&to).expect("clear folder");
            }
        }
    }

    #[test]
    fn a_cancelled_prompt_reads_as_cancelled() {
        assert!(is_cancelled(
            "0:120: execution error: User canceled. (-128)\n"
        ));
        assert!(is_cancelled("execution error: User canceled. (-128)"));
        assert!(!is_cancelled("error -128"));
        assert!(!is_cancelled("execution error: an error occurred. (-1280)"));
        assert!(!is_cancelled("execution error: mv: permission denied (1)"));
    }

    #[test]
    fn a_relaunch_lets_go_of_the_lock_before_opening_and_ends_when_it_opens() {
        let folder = folder();
        let lock_dir = folder.root.join("data");
        let mut host = FakeHost::new(&folder.root);
        host.lock_dir = Some(lock_dir.clone());
        let lock = jotra_storage::instance::acquire(&lock_dir).expect("lock");
        let env = vec![(OsString::from("JOTRA_DATA_DIR"), OsString::from("/x"))];
        let args = vec![OsString::from("note.md")];
        let reclaimed = std::cell::Cell::new(false);

        let outcome = relaunch(
            &host,
            &current_bundle(&folder.root),
            &env,
            &args,
            Some(lock),
            || {
                reclaimed.set(true);
                None
            },
        );

        assert!(matches!(outcome, RelaunchOutcome::Opened));
        assert_eq!(*host.lock_free_at_open.borrow(), vec![true]);
        assert_eq!(
            *host.opened.borrow(),
            vec![(current_bundle(&folder.root), env, args)]
        );
        assert!(!reclaimed.get());
    }

    #[test]
    fn a_relaunch_that_does_not_open_claims_the_lock_again() {
        let folder = folder();
        let lock_dir = folder.root.join("data");
        let mut host = FakeHost::new(&folder.root);
        host.lock_dir = Some(lock_dir.clone());
        host.fail_open = true;
        let lock = jotra_storage::instance::acquire(&lock_dir).expect("lock");

        let outcome = relaunch(
            &host,
            &current_bundle(&folder.root),
            &[],
            &[],
            Some(lock),
            || jotra_storage::instance::acquire(&lock_dir).ok(),
        );

        assert_eq!(*host.lock_free_at_open.borrow(), vec![true]);
        let RelaunchOutcome::CarriedOn(Some(_lock)) = outcome else {
            panic!("the lock is claimed again");
        };
        assert!(matches!(
            jotra_storage::instance::acquire(&lock_dir),
            Err(jotra_storage::instance::InstanceLockError::Held)
        ));
    }

    #[test]
    fn a_relaunch_without_the_lock_claims_none_when_it_does_not_open() {
        let folder = folder();
        let mut host = FakeHost::new(&folder.root);
        host.fail_open = true;
        let reclaimed = std::cell::Cell::new(false);

        let outcome = relaunch(&host, &current_bundle(&folder.root), &[], &[], None, || {
            reclaimed.set(true);
            None
        });

        assert!(matches!(outcome, RelaunchOutcome::CarriedOn(None)));
        assert!(!reclaimed.get());
        assert_eq!(host.opened.borrow().len(), 1);
    }

    /// Writes a launch agent at `path` whose first program argument is
    /// `program`, the shape the login item service writes.
    fn write_login_agent(path: &Path, program: &Path) {
        let mut arguments = plist::Dictionary::new();
        arguments.insert(
            "Label".to_string(),
            plist::Value::from(legacy_names::PRODUCT_NAME),
        );
        arguments.insert(
            "ProgramArguments".to_string(),
            plist::Value::Array(vec![plist::Value::from(
                program.to_string_lossy().into_owned(),
            )]),
        );
        arguments.insert("RunAtLoad".to_string(), plist::Value::Boolean(true));
        plist::Value::Dictionary(arguments)
            .to_file_xml(path)
            .expect("agent");
    }

    fn own_legacy_agent(root: &Path) -> PathBuf {
        let agent = root.join(legacy_names::LOGIN_AGENT_FILE);
        let program = executable_in_bundle(&legacy_bundle(root), legacy_names::GUI_BINARY_STEM);
        write_login_agent(&agent, &program);
        agent
    }

    #[test]
    fn the_login_item_step_removes_the_old_agent_and_registers_when_asked() {
        let folder = folder();
        let agent = own_legacy_agent(&folder.root);
        let login = FakeLogin::default();
        assert_eq!(
            migrate_login_item(&login, &agent, true, None).expect("step"),
            StepCompletion::Done
        );
        assert!(!agent.exists());
        assert_eq!(*login.set_calls.borrow(), vec![true]);
    }

    #[test]
    fn the_login_item_step_registers_nothing_the_config_does_not_ask_for() {
        let folder = folder();
        let agent = own_legacy_agent(&folder.root);
        let login = FakeLogin::default();
        assert_eq!(
            migrate_login_item(&login, &agent, false, None).expect("step"),
            StepCompletion::Done
        );
        assert!(!agent.exists());
        assert!(login.set_calls.borrow().is_empty());
        assert_eq!(
            migrate_login_item(&login, &agent, false, None).expect("step"),
            StepCompletion::Done
        );
    }

    #[test]
    fn an_agent_that_starts_the_app_from_the_new_name_is_removed_too() {
        let folder = folder();
        let agent = folder.root.join(legacy_names::LOGIN_AGENT_FILE);
        write_login_agent(
            &agent,
            &executable_in_bundle(&current_bundle(&folder.root), APP_BINARY),
        );
        let login = FakeLogin::default();
        assert_eq!(
            migrate_login_item(&login, &agent, false, Some(OsStr::new(APP_BINARY))).expect("step"),
            StepCompletion::Done
        );
        assert!(is_absent(&agent));
    }

    #[test]
    fn an_agent_at_the_old_name_that_starts_something_else_stays() {
        let folder = folder();
        let agent = folder.root.join(legacy_names::LOGIN_AGENT_FILE);
        let login = FakeLogin::default();
        for program in [
            folder
                .root
                .join("bin")
                .join(format!("{}-sync", legacy_names::COMMAND_NAME)),
            executable_in_bundle(
                &folder.root.join("Other.app"),
                legacy_names::GUI_BINARY_STEM,
            ),
            executable_in_bundle(&legacy_bundle(&folder.root), "helper"),
        ] {
            write_login_agent(&agent, &program);
            assert_eq!(
                migrate_login_item(&login, &agent, true, Some(OsStr::new(APP_BINARY)))
                    .expect("step"),
                StepCompletion::Declined,
                "{}",
                program.display()
            );
            assert!(agent.is_file(), "{}", program.display());
        }
        assert_eq!(*login.set_calls.borrow(), vec![true, true, true]);
    }

    #[test]
    fn an_agent_at_the_old_name_that_does_not_read_stays() {
        let folder = folder();
        let agent = folder.root.join(legacy_names::LOGIN_AGENT_FILE);
        std::fs::write(&agent, b"not a plist").expect("agent");
        let login = FakeLogin::default();
        assert_eq!(
            migrate_login_item(&login, &agent, false, None).expect("step"),
            StepCompletion::Declined
        );
        assert_eq!(std::fs::read(&agent).expect("agent"), b"not a plist");
    }

    #[test]
    fn a_refused_login_item_fails_the_step() {
        let folder = folder();
        let agent = folder.root.join(legacy_names::LOGIN_AGENT_FILE);
        let login = FakeLogin {
            fail: true,
            ..FakeLogin::default()
        };
        assert!(matches!(
            migrate_login_item(&login, &agent, true, None),
            Err(BundleRenameError::LoginItem(_))
        ));
    }

    fn command_bundle_at(bundle: &Path) -> PathBuf {
        for name in [COMMAND_NAME, legacy_names::COMMAND_NAME] {
            let command = executable_in_bundle(bundle, name);
            std::fs::create_dir_all(command.parent().expect("folder")).expect("bundle");
            std::fs::write(&command, CLI_SIGNATURE).expect("command");
        }
        bundle.to_path_buf()
    }

    fn command_bundle(root: &Path) -> PathBuf {
        command_bundle_at(&current_bundle(root))
    }

    fn command_folder(root: &Path) -> PathBuf {
        let bin = root.join("bin");
        std::fs::create_dir(&bin).expect("bin");
        bin
    }

    #[test]
    fn a_legacy_command_is_relinked_and_the_new_name_added() {
        let folder = folder();
        let bundle = command_bundle(&folder.root);
        let bin = command_folder(&folder.root);
        std::fs::write(bin.join(legacy_names::COMMAND_NAME), CLI_SIGNATURE).expect("legacy");

        assert_eq!(
            migrate_command_links(&bin, &bundle, None).expect("step"),
            StepCompletion::Done
        );
        assert_eq!(
            std::fs::read_link(bin.join(legacy_names::COMMAND_NAME)).expect("legacy link"),
            executable_in_bundle(&bundle, legacy_names::COMMAND_NAME)
        );
        assert_eq!(
            std::fs::read_link(bin.join(COMMAND_NAME)).expect("command link"),
            executable_in_bundle(&bundle, COMMAND_NAME)
        );
    }

    #[test]
    fn another_programs_command_name_is_left_alone() {
        let folder = folder();
        let bundle = command_bundle(&folder.root);
        let bin = command_folder(&folder.root);
        std::fs::write(bin.join(legacy_names::COMMAND_NAME), CLI_SIGNATURE).expect("legacy");
        std::fs::write(bin.join(COMMAND_NAME), b"another program").expect("foreign");

        assert_eq!(
            migrate_command_links(&bin, &bundle, None).expect("step"),
            StepCompletion::Done
        );
        assert_eq!(
            inspect_entry(&bin.join(COMMAND_NAME), None),
            InstalledEntry::File {
                is_jotra_command: false
            }
        );
        assert!(bin
            .join(legacy_names::COMMAND_NAME)
            .symlink_metadata()
            .expect("legacy")
            .file_type()
            .is_symlink());
    }

    #[test]
    fn a_legacy_name_that_is_not_jotras_command_is_untouched() {
        let folder = folder();
        let bundle = command_bundle(&folder.root);
        let bin = command_folder(&folder.root);
        std::fs::write(bin.join(legacy_names::COMMAND_NAME), b"another program").expect("legacy");

        assert_eq!(
            migrate_command_links(&bin, &bundle, None).expect("step"),
            StepCompletion::Done
        );
        assert_eq!(
            std::fs::read(bin.join(legacy_names::COMMAND_NAME)).expect("legacy"),
            b"another program"
        );
        assert!(!bin.join(COMMAND_NAME).exists());
    }

    #[test]
    fn a_command_folder_that_needs_a_password_is_a_decline() {
        let folder = folder();
        let bundle = command_bundle(&folder.root);
        let bin = command_folder(&folder.root);
        std::fs::write(bin.join(legacy_names::COMMAND_NAME), CLI_SIGNATURE).expect("legacy");
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o555)).expect("lock");
        let result = migrate_command_links(&bin, &bundle, None);
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).expect("unlock");
        assert_eq!(result.expect("step"), StepCompletion::Declined);
        assert!(!bin.join(COMMAND_NAME).exists());
    }

    #[test]
    fn a_legacy_link_left_dangling_by_the_rename_is_pointed_at_the_new_name() {
        let folder = folder();
        let bundle = command_bundle(&folder.root);
        let bin = command_folder(&folder.root);
        let gone = executable_in_bundle(&legacy_bundle(&folder.root), legacy_names::COMMAND_NAME);
        std::os::unix::fs::symlink(&gone, bin.join(legacy_names::COMMAND_NAME)).expect("link");

        assert_eq!(
            migrate_command_links(&bin, &bundle, None).expect("step"),
            StepCompletion::Done
        );
        assert_eq!(
            std::fs::read_link(bin.join(legacy_names::COMMAND_NAME)).expect("legacy link"),
            executable_in_bundle(&bundle, legacy_names::COMMAND_NAME)
        );
        assert!(bin.join(legacy_names::COMMAND_NAME).is_file());
        assert!(bin.join(COMMAND_NAME).is_file());
    }

    #[test]
    fn a_bundle_without_the_command_fails_the_step_and_keeps_the_legacy_name() {
        let folder = folder();
        let bundle = current_bundle(&folder.root);
        make_bundle(&bundle, BUNDLE_IDENTIFIER, VERSION);
        let bin = command_folder(&folder.root);
        std::fs::write(bin.join(legacy_names::COMMAND_NAME), CLI_SIGNATURE).expect("legacy");

        assert!(matches!(
            migrate_command_links(&bin, &bundle, None),
            Err(BundleRenameError::Unresolved { .. })
        ));
        assert_eq!(
            std::fs::read(bin.join(legacy_names::COMMAND_NAME)).expect("legacy"),
            CLI_SIGNATURE
        );
        assert!(is_absent(&bin.join(COMMAND_NAME)));
    }

    #[test]
    fn no_command_is_pointed_into_a_translocated_or_cask_bundle() {
        let folder = folder();
        let bin = command_folder(&folder.root);
        let legacy = bin.join(legacy_names::COMMAND_NAME);
        std::fs::write(&legacy, CLI_SIGNATURE).expect("legacy");
        for place in [
            folder.root.join("AppTranslocation").join("A1B2").join("d"),
            folder.root.join("Caskroom").join("jotra").join(VERSION),
        ] {
            let bundle = command_bundle_at(&current_bundle(&place));
            assert_eq!(
                migrate_command_links(&bin, &bundle, None).expect("step"),
                StepCompletion::Declined,
                "{}",
                bundle.display()
            );
            assert_eq!(std::fs::read(&legacy).expect("legacy"), CLI_SIGNATURE);
            assert!(is_absent(&bin.join(COMMAND_NAME)));
        }
    }

    #[test]
    fn the_per_user_steps_run_once_and_are_recorded() {
        let folder = folder();
        let bundle = command_bundle(&folder.root);
        let bin = command_folder(&folder.root);
        let data = folder.root.join("data");
        std::fs::create_dir(&data).expect("data");
        let home = folder.root.join("home");
        let agents = home.join(LAUNCH_AGENTS_FOLDER);
        std::fs::create_dir_all(&agents).expect("agents");
        write_login_agent(
            &agents.join(legacy_names::LOGIN_AGENT_FILE),
            &executable_in_bundle(&legacy_bundle(&folder.root), legacy_names::GUI_BINARY_STEM),
        );
        std::fs::write(bin.join(legacy_names::COMMAND_NAME), CLI_SIGNATURE).expect("legacy");
        let steps = PerUserSteps {
            bundle,
            version: VERSION.to_string(),
            data_dir: data.clone(),
        };
        let login = FakeLogin::default();

        let runs = run_per_user_steps(&steps, &login, Some(&home), true, &bin, None);
        assert_eq!(runs, (Some(StepRun::Done), StepRun::Done));
        assert!(!agents.join(legacy_names::LOGIN_AGENT_FILE).exists());
        assert_eq!(
            entry(&data, MigrationStep::LoginItem),
            Some(StepOutcome::Done)
        );
        assert_eq!(
            entry(&data, MigrationStep::CommandLinks),
            Some(StepOutcome::Done)
        );

        let runs = run_per_user_steps(&steps, &login, Some(&home), true, &bin, None);
        assert_eq!(
            runs,
            (Some(StepRun::AlreadySettled), StepRun::AlreadySettled)
        );
        assert_eq!(*login.set_calls.borrow(), vec![true]);
    }

    #[test]
    fn the_login_step_waits_without_a_home_folder() {
        let folder = folder();
        let bundle = command_bundle(&folder.root);
        let bin = command_folder(&folder.root);
        let steps = PerUserSteps {
            bundle,
            version: VERSION.to_string(),
            data_dir: folder.root.clone(),
        };
        let runs = run_per_user_steps(&steps, &FakeLogin::default(), None, true, &bin, None);
        assert_eq!(runs.0, None);
        assert_eq!(entry(&folder.root, MigrationStep::LoginItem), None);
    }
}
