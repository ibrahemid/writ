//! Whether the macOS app moves its bundle to the new folder name, and what it
//! does instead when it may not (ADR-059 decision 8).
//!
//! Before the Tauri builder runs, the shell gathers [`BundleFacts`] and hands
//! them to [`plan_bundle_rename`], which answers with a [`BundlePlan`] or the
//! [`StayReason`] the app runs where it is. The per-user steps that follow a
//! rename are planned here too. The file system work is the shell's.

use std::cmp::Ordering;
use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf};

use semver::Version;

use crate::cli_install::{is_translocated, may_replace, InstalledEntry};
use crate::legacy_names;
use crate::name_migration::{MigrationStep, NameMigrationRecord, StepOutcome};
use crate::user_variables;

/// The bundle's folder name since the rename.
pub const BUNDLE_FOLDER: &str = "Jotra.app";

/// The bundle identifier, which the rename keeps.
pub const BUNDLE_IDENTIFIER: &str = "com.writ.editor";

/// The variable that keeps a launch from renaming its bundle.
pub const SKIP_VAR: &str = "JOTRA_SKIP_BUNDLE_RENAME";

/// The one folder the rename asks for an administrator's password in.
pub const ADMIN_FOLDER: &str = "/Applications";

/// The folder, under the home folder, macOS login item agents live in.
pub const LAUNCH_AGENTS_FOLDER: &str = "Library/LaunchAgents";

/// The folder Homebrew keeps installed casks in.
const CASKROOM_FOLDER: &str = "Caskroom";

/// The folder a bundle's executables sit in, inside [`CONTENTS_FOLDER`].
const EXECUTABLE_FOLDER: &str = "MacOS";

/// The folder inside a bundle that holds everything else.
const CONTENTS_FOLDER: &str = "Contents";

/// The extension of a bundle's folder name.
const BUNDLE_EXTENSION: &str = "app";

/// The start of the process serial number argument older macOS versions pass
/// an app the Finder starts.
const PROCESS_SERIAL_PREFIX: &str = "-psn_";

/// A bundle's identity, as its `Info.plist` gives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleInfo {
    /// `CFBundleIdentifier`.
    pub identifier: String,
    /// `CFBundleShortVersionString`.
    pub version: String,
}

/// What sits at the sibling path: the new name beside a running bundle under
/// the old one, or the old name beside a running bundle under the new one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SiblingState {
    /// Nothing.
    Absent,
    /// A symbolic link, such as the one a rename leaves.
    Link,
    /// A bundle whose `Info.plist` reads.
    Bundle(BundleInfo),
    /// A file, or a folder whose `Info.plist` does not read.
    Unreadable,
}

/// What the running app knows about where it runs, gathered before the Tauri
/// builder.
#[derive(Debug, Clone, PartialEq)]
pub struct BundleFacts {
    /// The executable's path, as the process was started, runs through a
    /// symbolic link.
    pub exe_through_link: bool,
    /// The bundle the executable is in once every link is followed.
    pub canonical_bundle: Option<PathBuf>,
    /// The bundle the executable is in, as the process was started, or `None`
    /// outside a bundle.
    pub bundle: Option<PathBuf>,
    /// The running bundle's identity, or `None` when its `Info.plist` does not
    /// read.
    pub running: Option<BundleInfo>,
    /// What sits at [`sibling_path`] of the bundle.
    pub sibling: SiblingState,
    /// The bundle is on a read-only volume.
    pub read_only_volume: bool,
    /// The folder holding the bundle is writable by this user without a
    /// prompt.
    pub folder_writable: bool,
    /// [`SKIP_VAR`] is set.
    pub skip_requested: bool,
    /// The per-user migration record.
    pub record: NameMigrationRecord,
}

/// Who moves the bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Privileges {
    /// This user, with `rename(2)`.
    User,
    /// An administrator, through the system's password prompt.
    Administrator,
}

/// A rename of the running bundle to the new name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenamePlan {
    /// The running bundle under the old name. The link takes this path.
    pub from: PathBuf,
    /// The new name in the same folder.
    pub to: PathBuf,
    /// An older bundle at `to` goes to the Trash first.
    pub trash_older: bool,
    /// Who moves the bundle.
    pub privileges: Privileges,
}

/// What the launch does about its bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BundlePlan {
    /// Open this bundle, the one the executable's links resolve to, and exit.
    OpenCanonical(PathBuf),
    /// Open this bundle under the new name, as new as the running build or
    /// newer, and exit.
    OpenSibling(PathBuf),
    /// Move the running bundle to the new name, leave a hidden link at the old
    /// path, register the bundle, then open it and exit.
    Rename(RenamePlan),
    /// Move the bundle under the old name beside the running one to the Trash
    /// and leave a hidden link at its path.
    ReplaceLegacy {
        /// The bundle under the old name.
        legacy: PathBuf,
        /// The running bundle the link points at.
        bundle: PathBuf,
    },
    /// Leave the hidden link at the old name again, after a rename whose link
    /// did not land.
    RestoreLink {
        /// The old name's path, where the link goes.
        link: PathBuf,
        /// The running bundle the link points at.
        bundle: PathBuf,
    },
}

/// Why the launch leaves its bundle where it is.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StayReason {
    /// The executable runs through a link and the bundle it resolves to is
    /// unknown.
    #[error("the executable runs through a link to no known bundle")]
    CanonicalBundleUnknown,
    /// The executable is not inside a bundle.
    #[error("the executable is not inside an app bundle")]
    NotInBundle,
    /// [`SKIP_VAR`] is set.
    #[error("{SKIP_VAR} is set")]
    SkipRequested,
    /// The app runs from macOS App Translocation.
    #[error("the app runs from App Translocation")]
    Translocated,
    /// The bundle is on a read-only volume.
    #[error("the app is on a read-only volume")]
    ReadOnlyVolume,
    /// The bundle is inside a Homebrew Caskroom.
    #[error("the app is inside a Homebrew Caskroom")]
    InCaskroom,
    /// The running bundle's `Info.plist` does not read.
    #[error("the running bundle's Info.plist does not read")]
    RunningInfoUnreadable,
    /// The record says the rename is done.
    #[error("the record says the bundle rename is done")]
    AlreadyRenamed,
    /// The record holds a decline or a failure for this version.
    #[error("the bundle rename was {outcome:?} in version {version}")]
    SettledForVersion {
        /// How the rename ended.
        outcome: StepOutcome,
        /// The version it ended in.
        version: String,
    },
    /// The running bundle carries another identifier.
    #[error("the running bundle's identifier is {0}")]
    UnknownIdentifier(String),
    /// The bundle's folder carries neither the old nor the new name.
    #[error("the bundle's folder name is neither the old nor the new one")]
    UnrecognisedName,
    /// The sibling bundle carries another identifier.
    #[error("the bundle beside this one has the identifier {0}")]
    SiblingIdentifierDiffers(String),
    /// The new name beside the running bundle is a link.
    #[error("the new name beside this bundle is a link")]
    SiblingIsLink,
    /// The sibling path holds something other than a readable bundle.
    #[error("the path beside this bundle holds something other than a readable bundle")]
    SiblingUnreadable,
    /// A version does not parse as a semantic version.
    #[error("version {0} is not a semantic version")]
    VersionUnparsable(String),
    /// The bundle under the old name beside the running one is newer.
    #[error("the bundle under the old name is newer than this one")]
    SiblingNewer,
    /// The running bundle has the new name and nothing under the old name
    /// needs replacing.
    #[error("nothing to rename")]
    NothingToDo,
    /// The folder needs an administrator and the plan cannot ask for one.
    #[error("the bundle's folder is not writable")]
    FolderNotWritable,
}

/// Which name a bundle's folder carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BundleName {
    Legacy,
    Current,
}

fn bundle_name(bundle: &Path) -> Option<BundleName> {
    let name = bundle.file_name()?;
    if name == OsStr::new(legacy_names::BUNDLE_FOLDER) {
        Some(BundleName::Legacy)
    } else if name == OsStr::new(BUNDLE_FOLDER) {
        Some(BundleName::Current)
    } else {
        None
    }
}

/// The bundle `exe` sits in, as `<name>.app/Contents/MacOS/<exe>`, or `None`
/// for an executable outside a bundle.
pub fn bundle_of_executable(exe: &Path) -> Option<PathBuf> {
    let executables = exe.parent()?;
    let contents = executables.parent()?;
    let bundle = contents.parent()?;
    let named = |dir: &Path, name: &str| dir.file_name().is_some_and(|found| found == name);
    let is_bundle = bundle
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case(BUNDLE_EXTENSION));
    (named(executables, EXECUTABLE_FOLDER) && named(contents, CONTENTS_FOLDER) && is_bundle)
        .then(|| bundle.to_path_buf())
}

/// The path a command or executable named `name` has inside `bundle`.
pub fn executable_in_bundle(bundle: &Path, name: &str) -> PathBuf {
    bundle
        .join(CONTENTS_FOLDER)
        .join(EXECUTABLE_FOLDER)
        .join(name)
}

/// The path of `bundle`'s `Info.plist`.
pub fn info_plist_path(bundle: &Path) -> PathBuf {
    bundle.join(CONTENTS_FOLDER).join("Info.plist")
}

/// The other name beside `bundle`: the new name for a bundle under the old
/// one and the reverse, or `None` for a folder carrying neither.
pub fn sibling_path(bundle: &Path) -> Option<PathBuf> {
    let folder = bundle.parent()?;
    match bundle_name(bundle)? {
        BundleName::Legacy => Some(folder.join(BUNDLE_FOLDER)),
        BundleName::Current => Some(folder.join(legacy_names::BUNDLE_FOLDER)),
    }
}

/// Whether `path` lies inside a Homebrew Caskroom.
pub fn is_in_caskroom(path: &Path) -> bool {
    path.components()
        .any(|component| matches!(component, Component::Normal(name) if name == CASKROOM_FOLDER))
}

/// Whether [`SKIP_VAR`]'s `value` asks to skip the rename. A blank value
/// counts as unset.
pub fn is_rename_skipped(value: Option<&OsStr>) -> bool {
    value.is_some_and(|value| !value.to_string_lossy().trim().is_empty())
}

/// The arguments a relaunch hands the bundle it opens: the launch's own,
/// without the process serial number the Finder adds.
pub fn relaunch_arguments<I>(args: I) -> Vec<OsString>
where
    I: IntoIterator<Item = OsString>,
{
    args.into_iter()
        .filter(|arg| !arg.to_string_lossy().starts_with(PROCESS_SERIAL_PREFIX))
        .collect()
}

/// The environment a relaunch hands the bundle it opens: every variable whose
/// name starts with the current or the old prefix. `open` starts the bundle
/// without the launch's own environment.
pub fn relaunch_environment<I>(vars: I) -> Vec<(OsString, OsString)>
where
    I: IntoIterator<Item = (OsString, OsString)>,
{
    vars.into_iter()
        .filter(|(name, _)| is_product_variable(name))
        .collect()
}

fn is_product_variable(name: &OsStr) -> bool {
    let name = name.as_encoded_bytes();
    [
        user_variables::VARIABLE_PREFIX,
        legacy_names::VARIABLE_PREFIX,
    ]
    .iter()
    .any(|prefix| name.starts_with(prefix.as_bytes()))
}

/// What the launch does about its bundle, or why it leaves it where it is.
///
/// An executable reached through a link opens the bundle the link resolves
/// to, whatever else holds: the app cannot start through a link. Otherwise
/// the guards and the identifier are checked, in that order, before the
/// bundle and its sibling are compared. The record gates the moves only: a
/// bundle under the new name as new as this one opens whatever it holds, and
/// a link that did not land follows its own entry.
pub fn plan_bundle_rename(facts: &BundleFacts) -> Result<BundlePlan, StayReason> {
    if facts.exe_through_link {
        return facts
            .canonical_bundle
            .clone()
            .map(BundlePlan::OpenCanonical)
            .ok_or(StayReason::CanonicalBundleUnknown);
    }
    let bundle = facts.bundle.as_deref().ok_or(StayReason::NotInBundle)?;
    check_guards(facts, bundle)?;
    let running = facts
        .running
        .as_ref()
        .ok_or(StayReason::RunningInfoUnreadable)?;
    if running.identifier != BUNDLE_IDENTIFIER {
        return Err(StayReason::UnknownIdentifier(running.identifier.clone()));
    }
    let folder = bundle.parent().ok_or(StayReason::NotInBundle)?;
    match bundle_name(bundle) {
        Some(BundleName::Legacy) => plan_from_legacy(facts, bundle, folder, running),
        Some(BundleName::Current) => plan_from_current(facts, bundle, folder, running),
        None => Err(StayReason::UnrecognisedName),
    }
}

fn check_guards(facts: &BundleFacts, bundle: &Path) -> Result<(), StayReason> {
    if facts.skip_requested {
        return Err(StayReason::SkipRequested);
    }
    if is_translocated(bundle) {
        return Err(StayReason::Translocated);
    }
    if facts.read_only_volume {
        return Err(StayReason::ReadOnlyVolume);
    }
    if is_in_caskroom(bundle) {
        return Err(StayReason::InCaskroom);
    }
    Ok(())
}

fn check_record(record: &NameMigrationRecord, version: &str) -> Result<(), StayReason> {
    let Some(entry) = record.entry(MigrationStep::BundleRename) else {
        return Ok(());
    };
    match entry.outcome {
        StepOutcome::Done => Err(StayReason::AlreadyRenamed),
        outcome if entry.settles(version) => Err(StayReason::SettledForVersion {
            outcome,
            version: entry.version,
        }),
        _ => Ok(()),
    }
}

fn plan_from_legacy(
    facts: &BundleFacts,
    bundle: &Path,
    folder: &Path,
    running: &BundleInfo,
) -> Result<BundlePlan, StayReason> {
    let to = folder.join(BUNDLE_FOLDER);
    let trash_older = match &facts.sibling {
        SiblingState::Absent => false,
        SiblingState::Link => return Err(StayReason::SiblingIsLink),
        SiblingState::Unreadable => return Err(StayReason::SiblingUnreadable),
        SiblingState::Bundle(sibling) => {
            check_sibling_identifier(sibling)?;
            if compare_versions(&sibling.version, &running.version)? != Ordering::Less {
                return Ok(BundlePlan::OpenSibling(to));
            }
            true
        }
    };
    check_record(&facts.record, &running.version)?;
    let privileges = if facts.folder_writable {
        Privileges::User
    } else if !trash_older && folder == Path::new(ADMIN_FOLDER) {
        Privileges::Administrator
    } else {
        return Err(StayReason::FolderNotWritable);
    };
    Ok(BundlePlan::Rename(RenamePlan {
        from: bundle.to_path_buf(),
        to,
        trash_older,
        privileges,
    }))
}

fn plan_from_current(
    facts: &BundleFacts,
    bundle: &Path,
    folder: &Path,
    running: &BundleInfo,
) -> Result<BundlePlan, StayReason> {
    if facts.sibling == SiblingState::Absent {
        return plan_link_restore(facts, bundle, folder, running);
    }
    check_record(&facts.record, &running.version)?;
    let sibling = match &facts.sibling {
        SiblingState::Absent | SiblingState::Link => return Err(StayReason::NothingToDo),
        SiblingState::Unreadable => return Err(StayReason::SiblingUnreadable),
        SiblingState::Bundle(sibling) => sibling,
    };
    check_sibling_identifier(sibling)?;
    if compare_versions(&sibling.version, &running.version)? == Ordering::Greater {
        return Err(StayReason::SiblingNewer);
    }
    if !facts.folder_writable {
        return Err(StayReason::FolderNotWritable);
    }
    Ok(BundlePlan::ReplaceLegacy {
        legacy: folder.join(legacy_names::BUNDLE_FOLDER),
        bundle: bundle.to_path_buf(),
    })
}

fn plan_link_restore(
    facts: &BundleFacts,
    bundle: &Path,
    folder: &Path,
    running: &BundleInfo,
) -> Result<BundlePlan, StayReason> {
    let link_pending = facts.record.entry(MigrationStep::BundleLink).is_some()
        && facts
            .record
            .should_run(MigrationStep::BundleLink, &running.version);
    if !link_pending {
        return Err(StayReason::NothingToDo);
    }
    if !facts.folder_writable {
        return Err(StayReason::FolderNotWritable);
    }
    Ok(BundlePlan::RestoreLink {
        link: folder.join(legacy_names::BUNDLE_FOLDER),
        bundle: bundle.to_path_buf(),
    })
}

fn check_sibling_identifier(sibling: &BundleInfo) -> Result<(), StayReason> {
    if sibling.identifier == BUNDLE_IDENTIFIER {
        Ok(())
    } else {
        Err(StayReason::SiblingIdentifierDiffers(
            sibling.identifier.clone(),
        ))
    }
}

fn compare_versions(sibling: &str, running: &str) -> Result<Ordering, StayReason> {
    let parse = |version: &str| {
        Version::parse(version.trim())
            .map_err(|_| StayReason::VersionUnparsable(version.to_string()))
    };
    Ok(parse(sibling)?.cmp(&parse(running)?))
}

/// Whether the per-user steps run on this launch: it runs from the bundle
/// under the new name, with the kept identifier, and from none of the places
/// the rename stays out of.
pub fn per_user_steps_apply(facts: &BundleFacts) -> bool {
    let from_current_bundle = facts.bundle.as_deref().is_some_and(|bundle| {
        bundle_name(bundle) == Some(BundleName::Current)
            && !is_translocated(bundle)
            && !is_in_caskroom(bundle)
    });
    let kept_identifier = facts
        .running
        .as_ref()
        .is_some_and(|running| running.identifier == BUNDLE_IDENTIFIER);
    !facts.exe_through_link && !facts.read_only_volume && from_current_bundle && kept_identifier
}

/// What sits at the login item agent's path under the old name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyLoginAgent {
    /// Nothing.
    Absent,
    /// An agent that starts this app from a bundle under the old or the new
    /// name.
    Own,
    /// An agent that starts something else, or one that does not read.
    Foreign,
}

/// Whether a login agent whose program is `program` starts this app: the
/// program is the app binary, under its name from before the rename or
/// `app_binary_name`, inside a bundle under the old or the new name.
pub fn is_own_login_agent_program(program: &Path, app_binary_name: Option<&OsStr>) -> bool {
    let Some(file_name) = program.file_name() else {
        return false;
    };
    let is_app_binary = file_name == OsStr::new(legacy_names::GUI_BINARY_STEM)
        || app_binary_name.is_some_and(|name| file_name == name);
    is_app_binary
        && bundle_of_executable(program).is_some_and(|bundle| bundle_name(&bundle).is_some())
}

/// What the login item step does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoginItemPlan {
    /// Remove the agent made under the old name.
    pub remove_legacy_agent: bool,
    /// Register the agent under the new name.
    pub register: bool,
    /// The agent at the old name starts something else and stays.
    pub keep_foreign_agent: bool,
}

/// The login item step for a machine where the old agent's path holds
/// `legacy_agent` and the config's `general.launch_at_login` is
/// `launch_at_login`. Only the app's own agent is removed.
pub fn plan_login_item(legacy_agent: LegacyLoginAgent, launch_at_login: bool) -> LoginItemPlan {
    LoginItemPlan {
        remove_legacy_agent: legacy_agent == LegacyLoginAgent::Own,
        register: launch_at_login,
        keep_foreign_agent: legacy_agent == LegacyLoginAgent::Foreign,
    }
}

/// What the command links step knows about the PATH folder and the bundle the
/// links would point into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandLinkFacts {
    /// What sits at the command's legacy name.
    pub legacy: InstalledEntry,
    /// What sits at the command's own name.
    pub current: InstalledEntry,
    /// The folder holding both names is writable without a prompt.
    pub folder_writable: bool,
    /// The running bundle.
    pub bundle: PathBuf,
    /// The running bundle is on a read-only volume.
    pub bundle_read_only: bool,
}

/// What the command links step does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandLinkPlan {
    /// The running bundle is one the rename stays out of, so no link is
    /// pointed into it.
    BundleGuarded,
    /// The legacy name is not Jotra's command, so neither name is touched.
    Untouched,
    /// The folder needs an administrator; the install in Settings asks for
    /// one.
    NeedsPermission,
    /// Point the legacy name at the bundle under the new name.
    Relink {
        /// Also put the command in place under its own name.
        add_current: bool,
    },
}

/// The command links step for the PATH folder `facts` describes. A legacy
/// name [`may_replace`] counts as Jotra's, a link left dangling by the rename
/// among them, is pointed at the running bundle, unless that bundle is one
/// the rename stays out of. The command's own name is added beside it only
/// where [`may_replace`] allows.
pub fn plan_command_links(facts: &CommandLinkFacts) -> CommandLinkPlan {
    if facts.bundle_read_only || is_translocated(&facts.bundle) || is_in_caskroom(&facts.bundle) {
        return CommandLinkPlan::BundleGuarded;
    }
    let legacy_is_jotra = facts.legacy != InstalledEntry::Absent && may_replace(&facts.legacy);
    if !legacy_is_jotra {
        return CommandLinkPlan::Untouched;
    }
    if !facts.folder_writable {
        return CommandLinkPlan::NeedsPermission;
    }
    CommandLinkPlan::Relink {
        add_current: may_replace(&facts.current),
    }
}
