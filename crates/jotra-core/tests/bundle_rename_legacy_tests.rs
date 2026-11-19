//! The macOS bundle moves to its new name only where nothing guards against
//! it, and every reason it stays has its own answer (ADR-059 decision 8).

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use chrono::Utc;
use jotra_core::bundle_rename::{
    bundle_of_executable, executable_in_bundle, info_plist_path, is_in_caskroom,
    is_own_login_agent_program, is_rename_skipped, per_user_steps_apply, plan_bundle_rename,
    plan_command_links, plan_login_item, relaunch_arguments, relaunch_environment, sibling_path,
    BundleFacts, BundleInfo, BundlePlan, CommandLinkFacts, CommandLinkPlan, LegacyLoginAgent,
    LoginItemPlan, Privileges, RenamePlan, SiblingState, StayReason, ADMIN_FOLDER, BUNDLE_FOLDER,
    BUNDLE_IDENTIFIER, LAUNCH_AGENTS_FOLDER, SKIP_VAR,
};
use jotra_core::cli_install::InstalledEntry;
use jotra_core::legacy_names;
use jotra_core::name_migration::{MigrationStep, NameMigrationRecord, StepEntry, StepOutcome};
use jotra_core::user_variables::{self, UserVariable};

const VERSION: &str = "1.1.0";
const USER_APPS: &str = "/Users/u/Applications";

fn info(identifier: &str, version: &str) -> BundleInfo {
    BundleInfo {
        identifier: identifier.to_string(),
        version: version.to_string(),
    }
}

fn kept(version: &str) -> BundleInfo {
    info(BUNDLE_IDENTIFIER, version)
}

fn legacy_in(folder: &str) -> PathBuf {
    Path::new(folder).join(legacy_names::BUNDLE_FOLDER)
}

fn current_in(folder: &str) -> PathBuf {
    Path::new(folder).join(BUNDLE_FOLDER)
}

fn facts_for(bundle: PathBuf, sibling: SiblingState) -> BundleFacts {
    BundleFacts {
        exe_through_link: false,
        canonical_bundle: Some(bundle.clone()),
        bundle: Some(bundle),
        running: Some(kept(VERSION)),
        sibling,
        read_only_volume: false,
        folder_writable: true,
        skip_requested: false,
        record: NameMigrationRecord::default(),
    }
}

fn from_legacy(sibling: SiblingState) -> BundleFacts {
    facts_for(legacy_in(USER_APPS), sibling)
}

fn from_current(sibling: SiblingState) -> BundleFacts {
    facts_for(current_in(USER_APPS), sibling)
}

fn record_with(outcome: StepOutcome, version: &str) -> NameMigrationRecord {
    with_entry(
        NameMigrationRecord::default(),
        MigrationStep::BundleRename,
        outcome,
        version,
    )
}

fn with_entry(
    mut record: NameMigrationRecord,
    step: MigrationStep,
    outcome: StepOutcome,
    version: &str,
) -> NameMigrationRecord {
    record
        .record(
            step,
            &StepEntry {
                outcome,
                version: version.to_string(),
                at: Utc::now(),
                reason: None,
            },
        )
        .expect("record the entry");
    record
}

fn renamed_with_link(outcome: StepOutcome, version: &str) -> NameMigrationRecord {
    with_entry(
        record_with(StepOutcome::Done, "1.0.0"),
        MigrationStep::BundleLink,
        outcome,
        version,
    )
}

fn user_rename(trash_older: bool) -> BundlePlan {
    BundlePlan::Rename(RenamePlan {
        from: legacy_in(USER_APPS),
        to: current_in(USER_APPS),
        trash_older,
        privileges: Privileges::User,
    })
}

#[test]
fn an_executable_inside_a_bundle_names_that_bundle() {
    let bundle = legacy_in(USER_APPS);
    let exe = executable_in_bundle(&bundle, "jotra-app");
    assert_eq!(bundle_of_executable(&exe), Some(bundle));
}

#[test]
fn a_bundle_extension_in_capitals_still_counts() {
    let exe = Path::new("/Applications/Other.APP/Contents/MacOS/bin");
    assert_eq!(
        bundle_of_executable(exe),
        Some(PathBuf::from("/Applications/Other.APP"))
    );
}

#[test]
fn an_executable_outside_a_bundle_names_none() {
    for exe in [
        "/usr/local/bin/jotra",
        "/Applications/Jotra.app/Contents/Resources/jotra",
        "/Applications/Jotra.app/Other/MacOS/jotra",
        "/Applications/Jotra/Contents/MacOS/jotra",
        "MacOS/jotra",
        "jotra",
    ] {
        assert_eq!(bundle_of_executable(Path::new(exe)), None, "{exe}");
    }
}

#[test]
fn a_bundles_executables_and_info_plist_sit_under_contents() {
    let bundle = current_in(ADMIN_FOLDER);
    assert_eq!(
        executable_in_bundle(&bundle, "jotra"),
        PathBuf::from("/Applications/Jotra.app/Contents/MacOS/jotra")
    );
    assert_eq!(
        info_plist_path(&bundle),
        PathBuf::from("/Applications/Jotra.app/Contents/Info.plist")
    );
}

#[test]
fn each_name_has_the_other_beside_it() {
    assert_eq!(
        sibling_path(&legacy_in(ADMIN_FOLDER)),
        Some(current_in(ADMIN_FOLDER))
    );
    assert_eq!(
        sibling_path(&current_in(ADMIN_FOLDER)),
        Some(legacy_in(ADMIN_FOLDER))
    );
}

#[test]
fn a_bundle_under_another_name_has_no_sibling() {
    assert_eq!(sibling_path(Path::new("/Applications/Other.app")), None);
    assert_eq!(sibling_path(Path::new("/")), None);
}

#[test]
fn a_path_through_a_caskroom_is_in_one() {
    assert!(is_in_caskroom(Path::new(
        "/opt/homebrew/Caskroom/jotra/1.1.0/Jotra.app"
    )));
    assert!(!is_in_caskroom(&current_in(ADMIN_FOLDER)));
    assert!(!is_in_caskroom(Path::new("/Users/u/MyCaskroom/Jotra.app")));
}

#[test]
fn a_blank_skip_variable_counts_as_unset() {
    assert!(!is_rename_skipped(None));
    assert!(!is_rename_skipped(Some(OsStr::new(""))));
    assert!(!is_rename_skipped(Some(OsStr::new("  "))));
    assert!(is_rename_skipped(Some(OsStr::new("1"))));
    assert!(is_rename_skipped(Some(OsStr::new("no"))));
}

#[test]
fn the_skip_variable_has_the_new_prefix() {
    assert_eq!(SKIP_VAR, "JOTRA_SKIP_BUNDLE_RENAME");
}

#[test]
fn a_relaunch_keeps_the_arguments_without_the_process_serial_number() {
    let args = ["-psn_0_12345", "--open", "/tmp/a b.md"].map(OsString::from);
    assert_eq!(
        relaunch_arguments(args),
        vec![OsString::from("--open"), OsString::from("/tmp/a b.md")]
    );
    assert!(relaunch_arguments(Vec::<OsString>::new()).is_empty());
}

#[test]
fn a_relaunch_carries_the_variables_under_either_prefix_and_nothing_else() {
    let vars = [
        ("PATH", "/usr/bin:/bin"),
        ("JOTRA_DATA_DIR", "/Users/u/My Data"),
        ("HOME", "/Users/u"),
        ("WRIT_NOTES_DIR", "/Users/u/a=b c"),
        ("JOTRAX", "1"),
        ("jotra_data_dir", "/lower"),
        ("XWRIT_DATA_DIR", "/other"),
        ("WRIT_", ""),
    ]
    .map(|(name, value)| (OsString::from(name), OsString::from(value)));
    assert_eq!(
        relaunch_environment(vars),
        vec![
            (
                OsString::from("JOTRA_DATA_DIR"),
                OsString::from("/Users/u/My Data")
            ),
            (
                OsString::from("WRIT_NOTES_DIR"),
                OsString::from("/Users/u/a=b c")
            ),
            (OsString::from("WRIT_"), OsString::new()),
        ]
    );
    assert!(relaunch_environment(Vec::<(OsString, OsString)>::new()).is_empty());
}

#[test]
fn every_variable_the_app_reads_starts_with_a_carried_prefix() {
    for variable in [
        UserVariable::DataDir,
        UserVariable::NotesDir,
        UserVariable::GuiBin,
    ] {
        assert!(
            variable
                .current_name()
                .starts_with(user_variables::VARIABLE_PREFIX),
            "{variable:?}"
        );
        assert!(
            variable
                .legacy_name()
                .starts_with(legacy_names::VARIABLE_PREFIX),
            "{variable:?}"
        );
    }
    assert!(SKIP_VAR.starts_with(user_variables::VARIABLE_PREFIX));
    assert!(legacy_names::COMMAND_VAR.starts_with(legacy_names::VARIABLE_PREFIX));
}

#[test]
fn an_executable_reached_through_a_link_opens_the_bundle_it_resolves_to() {
    let mut facts = from_current(SiblingState::Absent);
    facts.exe_through_link = true;
    facts.bundle = Some(legacy_in(USER_APPS));
    facts.skip_requested = true;
    facts.record = record_with(StepOutcome::Done, VERSION);
    assert_eq!(
        plan_bundle_rename(&facts),
        Ok(BundlePlan::OpenCanonical(current_in(USER_APPS)))
    );
}

#[test]
fn a_link_to_no_known_bundle_stays() {
    let mut facts = from_current(SiblingState::Absent);
    facts.exe_through_link = true;
    facts.canonical_bundle = None;
    assert_eq!(
        plan_bundle_rename(&facts),
        Err(StayReason::CanonicalBundleUnknown)
    );
}

#[test]
fn an_executable_outside_a_bundle_stays() {
    let mut facts = from_legacy(SiblingState::Absent);
    facts.bundle = None;
    assert_eq!(plan_bundle_rename(&facts), Err(StayReason::NotInBundle));
}

#[test]
fn the_skip_variable_keeps_the_bundle() {
    let mut facts = from_legacy(SiblingState::Absent);
    facts.skip_requested = true;
    assert_eq!(plan_bundle_rename(&facts), Err(StayReason::SkipRequested));
}

#[test]
fn a_translocated_bundle_stays() {
    let bundle = PathBuf::from("/private/var/folders/x/AppTranslocation/ABC/d")
        .join(legacy_names::BUNDLE_FOLDER);
    let facts = facts_for(bundle, SiblingState::Absent);
    assert_eq!(plan_bundle_rename(&facts), Err(StayReason::Translocated));
}

#[test]
fn a_bundle_on_a_read_only_volume_stays() {
    let mut facts = from_legacy(SiblingState::Absent);
    facts.read_only_volume = true;
    assert_eq!(plan_bundle_rename(&facts), Err(StayReason::ReadOnlyVolume));
}

#[test]
fn a_bundle_in_a_caskroom_stays() {
    let facts = facts_for(
        legacy_in("/opt/homebrew/Caskroom/writ/1.0.0"),
        SiblingState::Absent,
    );
    assert_eq!(plan_bundle_rename(&facts), Err(StayReason::InCaskroom));
}

#[test]
fn the_guards_answer_in_their_order() {
    let mut facts = facts_for(
        legacy_in("/opt/homebrew/Caskroom/AppTranslocation"),
        SiblingState::Absent,
    );
    facts.skip_requested = true;
    facts.read_only_volume = true;
    facts.running = None;
    assert_eq!(plan_bundle_rename(&facts), Err(StayReason::SkipRequested));
    facts.skip_requested = false;
    assert_eq!(plan_bundle_rename(&facts), Err(StayReason::Translocated));
    facts.bundle = Some(legacy_in("/opt/homebrew/Caskroom/x"));
    assert_eq!(plan_bundle_rename(&facts), Err(StayReason::ReadOnlyVolume));
    facts.read_only_volume = false;
    assert_eq!(plan_bundle_rename(&facts), Err(StayReason::InCaskroom));
    facts.bundle = Some(legacy_in(USER_APPS));
    assert_eq!(
        plan_bundle_rename(&facts),
        Err(StayReason::RunningInfoUnreadable)
    );
}

#[test]
fn a_bundle_whose_info_plist_does_not_read_stays() {
    let mut facts = from_legacy(SiblingState::Absent);
    facts.running = None;
    assert_eq!(
        plan_bundle_rename(&facts),
        Err(StayReason::RunningInfoUnreadable)
    );
}

#[test]
fn a_done_rename_never_runs_again() {
    let mut facts = from_legacy(SiblingState::Absent);
    facts.record = record_with(StepOutcome::Done, "1.0.0");
    assert_eq!(plan_bundle_rename(&facts), Err(StayReason::AlreadyRenamed));
}

#[test]
fn a_rename_declined_or_failed_in_this_version_does_not_run_again() {
    for outcome in [StepOutcome::Declined, StepOutcome::Failed] {
        let mut facts = from_legacy(SiblingState::Absent);
        facts.record = record_with(outcome, VERSION);
        assert_eq!(
            plan_bundle_rename(&facts),
            Err(StayReason::SettledForVersion {
                outcome,
                version: VERSION.to_string(),
            })
        );
    }
}

#[test]
fn a_rename_declined_or_failed_in_another_version_runs_again() {
    for outcome in [StepOutcome::Declined, StepOutcome::Failed] {
        let mut facts = from_legacy(SiblingState::Absent);
        facts.record = record_with(outcome, "1.0.0");
        assert_eq!(plan_bundle_rename(&facts), Ok(user_rename(false)));
    }
}

#[test]
fn a_bundle_with_another_identifier_stays() {
    let mut facts = from_legacy(SiblingState::Absent);
    facts.running = Some(info("com.example.other", VERSION));
    assert_eq!(
        plan_bundle_rename(&facts),
        Err(StayReason::UnknownIdentifier(
            "com.example.other".to_string()
        ))
    );
}

#[test]
fn a_bundle_under_another_name_stays() {
    let facts = facts_for(
        PathBuf::from("/Applications/Editor.app"),
        SiblingState::Absent,
    );
    assert_eq!(
        plan_bundle_rename(&facts),
        Err(StayReason::UnrecognisedName)
    );
}

#[test]
fn the_old_name_alone_in_a_writable_folder_is_renamed_by_the_user() {
    let facts = from_legacy(SiblingState::Absent);
    assert_eq!(plan_bundle_rename(&facts), Ok(user_rename(false)));
}

#[test]
fn the_old_name_alone_in_a_locked_applications_folder_asks_an_administrator() {
    let mut facts = facts_for(legacy_in(ADMIN_FOLDER), SiblingState::Absent);
    facts.folder_writable = false;
    assert_eq!(
        plan_bundle_rename(&facts),
        Ok(BundlePlan::Rename(RenamePlan {
            from: legacy_in(ADMIN_FOLDER),
            to: current_in(ADMIN_FOLDER),
            trash_older: false,
            privileges: Privileges::Administrator,
        }))
    );
}

#[test]
fn the_old_name_in_another_locked_folder_stays() {
    let mut facts = from_legacy(SiblingState::Absent);
    facts.folder_writable = false;
    assert_eq!(
        plan_bundle_rename(&facts),
        Err(StayReason::FolderNotWritable)
    );
}

#[test]
fn a_link_at_the_new_name_keeps_the_old_bundle() {
    let facts = from_legacy(SiblingState::Link);
    assert_eq!(plan_bundle_rename(&facts), Err(StayReason::SiblingIsLink));
}

#[test]
fn an_unreadable_new_name_keeps_the_old_bundle() {
    let facts = from_legacy(SiblingState::Unreadable);
    assert_eq!(
        plan_bundle_rename(&facts),
        Err(StayReason::SiblingUnreadable)
    );
}

#[test]
fn another_apps_bundle_at_the_new_name_keeps_the_old_bundle() {
    let facts = from_legacy(SiblingState::Bundle(info("com.example.jotra", "9.0.0")));
    assert_eq!(
        plan_bundle_rename(&facts),
        Err(StayReason::SiblingIdentifierDiffers(
            "com.example.jotra".to_string()
        ))
    );
}

#[test]
fn the_old_name_opens_a_new_name_bundle_as_new_or_newer() {
    for version in [VERSION, "1.1.1", "2.0.0"] {
        let facts = from_legacy(SiblingState::Bundle(kept(version)));
        assert_eq!(
            plan_bundle_rename(&facts),
            Ok(BundlePlan::OpenSibling(current_in(USER_APPS))),
            "{version}"
        );
    }
}

#[test]
fn the_old_name_opens_a_new_name_bundle_whatever_the_record_holds() {
    for record in [
        record_with(StepOutcome::Declined, VERSION),
        record_with(StepOutcome::Failed, VERSION),
        record_with(StepOutcome::Done, "1.0.0"),
    ] {
        for version in [VERSION, "1.2.0"] {
            let mut facts = from_legacy(SiblingState::Bundle(kept(version)));
            facts.record = record.clone();
            assert_eq!(
                plan_bundle_rename(&facts),
                Ok(BundlePlan::OpenSibling(current_in(USER_APPS))),
                "{record:?} {version}"
            );
        }
    }
}

#[test]
fn the_record_still_keeps_the_rename_beside_an_older_new_name_bundle() {
    let mut facts = from_legacy(SiblingState::Bundle(kept("1.0.0")));
    facts.record = record_with(StepOutcome::Declined, VERSION);
    assert_eq!(
        plan_bundle_rename(&facts),
        Err(StayReason::SettledForVersion {
            outcome: StepOutcome::Declined,
            version: VERSION.to_string(),
        })
    );
}

#[test]
fn the_old_name_opens_a_new_name_bundle_even_in_a_locked_folder() {
    let mut facts = from_legacy(SiblingState::Bundle(kept(VERSION)));
    facts.folder_writable = false;
    assert_eq!(
        plan_bundle_rename(&facts),
        Ok(BundlePlan::OpenSibling(current_in(USER_APPS)))
    );
}

#[test]
fn an_older_new_name_bundle_is_trashed_before_the_rename() {
    for version in ["1.0.9", "1.1.0-rc.1"] {
        let facts = from_legacy(SiblingState::Bundle(kept(version)));
        assert_eq!(
            plan_bundle_rename(&facts),
            Ok(user_rename(true)),
            "{version}"
        );
    }
}

#[test]
fn an_older_new_name_bundle_in_a_locked_folder_stays() {
    let mut facts = facts_for(legacy_in(ADMIN_FOLDER), SiblingState::Bundle(kept("1.0.0")));
    facts.folder_writable = false;
    assert_eq!(
        plan_bundle_rename(&facts),
        Err(StayReason::FolderNotWritable)
    );
}

#[test]
fn a_version_that_is_not_semantic_stays() {
    let facts = from_legacy(SiblingState::Bundle(kept("1.1")));
    assert_eq!(
        plan_bundle_rename(&facts),
        Err(StayReason::VersionUnparsable("1.1".to_string()))
    );
    let mut facts = from_current(SiblingState::Bundle(kept(VERSION)));
    facts.running = Some(kept("latest"));
    assert_eq!(
        plan_bundle_rename(&facts),
        Err(StayReason::VersionUnparsable("latest".to_string()))
    );
}

#[test]
fn the_new_name_with_nothing_or_a_link_beside_it_has_nothing_to_do() {
    for sibling in [SiblingState::Absent, SiblingState::Link] {
        let facts = from_current(sibling);
        assert_eq!(plan_bundle_rename(&facts), Err(StayReason::NothingToDo));
    }
}

#[test]
fn a_link_that_did_not_land_is_left_again_under_another_version() {
    for outcome in [StepOutcome::Failed, StepOutcome::Declined] {
        let mut facts = from_current(SiblingState::Absent);
        facts.record = renamed_with_link(outcome, "1.0.0");
        assert_eq!(
            plan_bundle_rename(&facts),
            Ok(BundlePlan::RestoreLink {
                link: legacy_in(USER_APPS),
                bundle: current_in(USER_APPS),
            }),
            "{outcome:?}"
        );
    }
}

#[test]
fn a_link_settled_for_this_version_or_never_tried_is_not_left_again() {
    for record in [
        renamed_with_link(StepOutcome::Failed, VERSION),
        renamed_with_link(StepOutcome::Declined, VERSION),
        renamed_with_link(StepOutcome::Done, "1.0.0"),
        record_with(StepOutcome::Done, "1.0.0"),
        NameMigrationRecord::default(),
    ] {
        let mut facts = from_current(SiblingState::Absent);
        facts.record = record.clone();
        assert_eq!(
            plan_bundle_rename(&facts),
            Err(StayReason::NothingToDo),
            "{record:?}"
        );
    }
}

#[test]
fn a_link_that_did_not_land_in_a_locked_folder_stays_missing() {
    let mut facts = from_current(SiblingState::Absent);
    facts.record = renamed_with_link(StepOutcome::Failed, "1.0.0");
    facts.folder_writable = false;
    assert_eq!(
        plan_bundle_rename(&facts),
        Err(StayReason::FolderNotWritable)
    );
}

#[test]
fn a_link_is_left_again_only_where_the_old_name_is_free() {
    let mut facts = from_current(SiblingState::Link);
    facts.record = renamed_with_link(StepOutcome::Failed, "1.0.0");
    assert_eq!(plan_bundle_rename(&facts), Err(StayReason::AlreadyRenamed));
    let mut facts = from_current(SiblingState::Unreadable);
    facts.record = renamed_with_link(StepOutcome::Failed, "1.0.0");
    assert_eq!(plan_bundle_rename(&facts), Err(StayReason::AlreadyRenamed));
}

#[test]
fn the_new_name_leaves_an_unreadable_old_name_alone() {
    let facts = from_current(SiblingState::Unreadable);
    assert_eq!(
        plan_bundle_rename(&facts),
        Err(StayReason::SiblingUnreadable)
    );
}

#[test]
fn the_new_name_leaves_another_apps_old_name_bundle_alone() {
    let facts = from_current(SiblingState::Bundle(info("com.example.writ", "0.1.0")));
    assert_eq!(
        plan_bundle_rename(&facts),
        Err(StayReason::SiblingIdentifierDiffers(
            "com.example.writ".to_string()
        ))
    );
}

#[test]
fn the_new_name_leaves_a_newer_old_name_bundle_alone() {
    let facts = from_current(SiblingState::Bundle(kept("1.2.0")));
    assert_eq!(plan_bundle_rename(&facts), Err(StayReason::SiblingNewer));
}

#[test]
fn the_new_name_replaces_an_old_name_bundle_as_old_or_older() {
    for version in [VERSION, "1.0.0", "0.7.0"] {
        let facts = from_current(SiblingState::Bundle(kept(version)));
        assert_eq!(
            plan_bundle_rename(&facts),
            Ok(BundlePlan::ReplaceLegacy {
                legacy: legacy_in(USER_APPS),
                bundle: current_in(USER_APPS),
            }),
            "{version}"
        );
    }
}

#[test]
fn the_new_name_in_a_locked_folder_leaves_the_old_bundle() {
    let mut facts = facts_for(
        current_in(ADMIN_FOLDER),
        SiblingState::Bundle(kept("1.0.0")),
    );
    facts.folder_writable = false;
    assert_eq!(
        plan_bundle_rename(&facts),
        Err(StayReason::FolderNotWritable)
    );
}

#[test]
fn every_reason_to_stay_reads_as_a_sentence() {
    let reasons = [
        StayReason::CanonicalBundleUnknown,
        StayReason::NotInBundle,
        StayReason::SkipRequested,
        StayReason::Translocated,
        StayReason::ReadOnlyVolume,
        StayReason::InCaskroom,
        StayReason::RunningInfoUnreadable,
        StayReason::AlreadyRenamed,
        StayReason::SettledForVersion {
            outcome: StepOutcome::Declined,
            version: VERSION.to_string(),
        },
        StayReason::UnknownIdentifier("x".to_string()),
        StayReason::UnrecognisedName,
        StayReason::SiblingIdentifierDiffers("x".to_string()),
        StayReason::SiblingIsLink,
        StayReason::SiblingUnreadable,
        StayReason::VersionUnparsable("x".to_string()),
        StayReason::SiblingNewer,
        StayReason::NothingToDo,
        StayReason::FolderNotWritable,
    ];
    for reason in reasons {
        assert!(!reason.to_string().is_empty(), "{reason:?}");
    }
    assert!(StayReason::SkipRequested.to_string().contains(SKIP_VAR));
    assert!(StayReason::SettledForVersion {
        outcome: StepOutcome::Failed,
        version: VERSION.to_string(),
    }
    .to_string()
    .contains(VERSION));
}

#[test]
fn the_per_user_steps_run_from_the_new_name() {
    let mut facts = from_current(SiblingState::Absent);
    assert!(per_user_steps_apply(&facts));
    facts.skip_requested = true;
    assert!(per_user_steps_apply(&facts));
}

#[test]
fn the_per_user_steps_wait_outside_the_new_name() {
    let base = from_current(SiblingState::Absent);
    let mut cases = Vec::new();

    let mut facts = base.clone();
    facts.exe_through_link = true;
    cases.push(("through a link", facts));

    cases.push(("old name", from_legacy(SiblingState::Absent)));

    let mut facts = base.clone();
    facts.bundle = None;
    cases.push(("outside a bundle", facts));

    let mut facts = base.clone();
    facts.running = Some(info("com.example.other", VERSION));
    cases.push(("other identifier", facts));

    let mut facts = base.clone();
    facts.running = None;
    cases.push(("unreadable info", facts));

    let mut facts = base.clone();
    facts.read_only_volume = true;
    cases.push(("read-only volume", facts));

    cases.push((
        "translocated",
        facts_for(
            current_in("/private/var/folders/x/AppTranslocation/ABC/d"),
            SiblingState::Absent,
        ),
    ));
    cases.push((
        "caskroom",
        facts_for(
            current_in("/opt/homebrew/Caskroom/jotra/1.1.0"),
            SiblingState::Absent,
        ),
    ));

    for (case, facts) in cases {
        assert!(!per_user_steps_apply(&facts), "{case}");
    }
}

#[test]
fn the_login_item_step_removes_the_old_agent_and_registers_when_asked() {
    for (agent, launch_at_login, plan) in [
        (
            LegacyLoginAgent::Own,
            true,
            LoginItemPlan {
                remove_legacy_agent: true,
                register: true,
                keep_foreign_agent: false,
            },
        ),
        (
            LegacyLoginAgent::Own,
            false,
            LoginItemPlan {
                remove_legacy_agent: true,
                register: false,
                keep_foreign_agent: false,
            },
        ),
        (
            LegacyLoginAgent::Absent,
            true,
            LoginItemPlan {
                remove_legacy_agent: false,
                register: true,
                keep_foreign_agent: false,
            },
        ),
        (
            LegacyLoginAgent::Absent,
            false,
            LoginItemPlan {
                remove_legacy_agent: false,
                register: false,
                keep_foreign_agent: false,
            },
        ),
    ] {
        assert_eq!(
            plan_login_item(agent, launch_at_login),
            plan,
            "{agent:?} {launch_at_login}"
        );
    }
}

#[test]
fn the_login_item_step_keeps_an_agent_that_starts_something_else() {
    for launch_at_login in [true, false] {
        assert_eq!(
            plan_login_item(LegacyLoginAgent::Foreign, launch_at_login),
            LoginItemPlan {
                remove_legacy_agent: false,
                register: launch_at_login,
                keep_foreign_agent: true,
            }
        );
    }
}

#[test]
fn an_agent_is_the_apps_own_when_it_starts_the_app_binary_in_either_bundle() {
    let current = OsStr::new("jotra-tauri");
    for program in [
        executable_in_bundle(&legacy_in(ADMIN_FOLDER), legacy_names::GUI_BINARY_STEM),
        executable_in_bundle(&legacy_in(USER_APPS), "jotra-tauri"),
        executable_in_bundle(&current_in(ADMIN_FOLDER), "jotra-tauri"),
        executable_in_bundle(&current_in(USER_APPS), legacy_names::GUI_BINARY_STEM),
    ] {
        assert!(
            is_own_login_agent_program(&program, Some(current)),
            "{program:?}"
        );
    }
    assert!(is_own_login_agent_program(
        &executable_in_bundle(&legacy_in(ADMIN_FOLDER), legacy_names::GUI_BINARY_STEM),
        None
    ));
}

#[test]
fn an_agent_that_starts_anything_else_is_not_the_apps_own() {
    let current = OsStr::new("jotra-tauri");
    for program in [
        executable_in_bundle(Path::new("/Applications/Writer.app"), "jotra-tauri"),
        executable_in_bundle(&legacy_in(ADMIN_FOLDER), "backup-agent"),
        executable_in_bundle(&legacy_in(ADMIN_FOLDER), "jotra"),
        PathBuf::from("/usr/local/bin/writ-tauri"),
        PathBuf::from("/Applications/Writ.app/Contents/Resources/writ-tauri"),
        PathBuf::from("/"),
    ] {
        assert!(
            !is_own_login_agent_program(&program, Some(current)),
            "{program:?}"
        );
    }
    assert!(!is_own_login_agent_program(
        &executable_in_bundle(&legacy_in(ADMIN_FOLDER), "jotra-tauri"),
        None
    ));
}

#[test]
fn the_login_agents_live_under_the_home_library() {
    assert_eq!(LAUNCH_AGENTS_FOLDER, "Library/LaunchAgents");
    assert_eq!(
        Path::new("/Users/u")
            .join(LAUNCH_AGENTS_FOLDER)
            .join(legacy_names::LOGIN_AGENT_FILE),
        PathBuf::from("/Users/u/Library/LaunchAgents/Writ.plist")
    );
}

fn legacy_bundle_link() -> InstalledEntry {
    InstalledEntry::Link {
        target: executable_in_bundle(&legacy_in(ADMIN_FOLDER), legacy_names::COMMAND_NAME),
        beside_app: false,
    }
}

fn link_facts(legacy: InstalledEntry, current: InstalledEntry, writable: bool) -> CommandLinkFacts {
    CommandLinkFacts {
        legacy,
        current,
        folder_writable: writable,
        bundle: current_in(ADMIN_FOLDER),
        bundle_read_only: false,
    }
}

#[test]
fn a_legacy_command_that_is_not_jotras_is_untouched() {
    for legacy in [
        InstalledEntry::Absent,
        InstalledEntry::File {
            is_jotra_command: false,
        },
        InstalledEntry::Other,
        InstalledEntry::Link {
            target: PathBuf::from("/opt/homebrew/Cellar/writ/2.0/bin/writ"),
            beside_app: false,
        },
        InstalledEntry::Link {
            target: executable_in_bundle(&legacy_in(ADMIN_FOLDER), "backup"),
            beside_app: false,
        },
    ] {
        for writable in [true, false] {
            assert_eq!(
                plan_command_links(&link_facts(
                    legacy.clone(),
                    InstalledEntry::Absent,
                    writable
                )),
                CommandLinkPlan::Untouched,
                "{legacy:?} {writable}"
            );
        }
    }
}

#[test]
fn a_locked_command_folder_waits_for_the_install_in_settings() {
    assert_eq!(
        plan_command_links(&link_facts(
            legacy_bundle_link(),
            InstalledEntry::Absent,
            false
        )),
        CommandLinkPlan::NeedsPermission
    );
}

#[test]
fn a_legacy_link_into_a_bundle_that_is_gone_is_relinked() {
    let dangling = InstalledEntry::Link {
        target: executable_in_bundle(&legacy_in(ADMIN_FOLDER), legacy_names::COMMAND_NAME),
        beside_app: false,
    };
    assert_eq!(
        plan_command_links(&link_facts(dangling, InstalledEntry::Absent, true)),
        CommandLinkPlan::Relink { add_current: true }
    );
    let copied = InstalledEntry::File {
        is_jotra_command: true,
    };
    assert_eq!(
        plan_command_links(&link_facts(copied, InstalledEntry::Absent, true)),
        CommandLinkPlan::Relink { add_current: true }
    );
}

#[test]
fn no_command_is_pointed_into_a_bundle_the_rename_stays_out_of() {
    let guarded = [
        (
            PathBuf::from("/private/var/folders/x/AppTranslocation/ABC/d/Jotra.app"),
            false,
        ),
        (PathBuf::from("/Volumes/Jotra/Jotra.app"), true),
        (
            PathBuf::from("/opt/homebrew/Caskroom/jotra/1.1.0/Jotra.app"),
            false,
        ),
    ];
    for (bundle, read_only) in guarded {
        for writable in [true, false] {
            let facts = CommandLinkFacts {
                bundle: bundle.clone(),
                bundle_read_only: read_only,
                ..link_facts(legacy_bundle_link(), InstalledEntry::Absent, writable)
            };
            assert_eq!(
                plan_command_links(&facts),
                CommandLinkPlan::BundleGuarded,
                "{bundle:?} {writable}"
            );
        }
    }
}

#[test]
fn the_legacy_command_is_relinked_and_the_new_name_added_where_allowed() {
    let bundle_link = InstalledEntry::Link {
        target: executable_in_bundle(&legacy_in(ADMIN_FOLDER), "jotra"),
        beside_app: false,
    };
    for (current, add_current) in [
        (InstalledEntry::Absent, true),
        (bundle_link, true),
        (
            InstalledEntry::File {
                is_jotra_command: true,
            },
            true,
        ),
        (
            InstalledEntry::File {
                is_jotra_command: false,
            },
            false,
        ),
        (InstalledEntry::Other, false),
    ] {
        assert_eq!(
            plan_command_links(&link_facts(legacy_bundle_link(), current.clone(), true)),
            CommandLinkPlan::Relink { add_current },
            "{current:?}"
        );
    }
}
