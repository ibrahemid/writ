//! The per-user name migration record's rules (ADR-059 decision 12).

use std::collections::HashSet;

use chrono::{TimeZone, Utc};
use jotra_core::name_migration::{
    MigrationStep, NameMigrationError, NameMigrationRecord, StepEntry, StepOutcome, RECORD_FILE,
};

fn entry(outcome: StepOutcome, version: &str) -> StepEntry {
    StepEntry {
        outcome,
        version: version.to_string(),
        at: Utc.with_ymd_and_hms(2026, 11, 18, 9, 0, 0).unwrap(),
        reason: None,
    }
}

#[test]
fn the_record_has_its_file_name() {
    assert_eq!(RECORD_FILE, "name-migration.json");
}

#[test]
fn every_step_has_its_own_key() {
    let keys: HashSet<&str> = MigrationStep::ALL.iter().map(|step| step.key()).collect();
    assert_eq!(keys.len(), MigrationStep::ALL.len());
    assert_eq!(
        MigrationStep::ALL.map(MigrationStep::key),
        [
            "bundle_rename",
            "bundle_link",
            "login_item",
            "command_links",
            "linux_autostart",
            "windows_run_value"
        ]
    );
}

#[test]
fn a_done_step_is_settled_in_every_version() {
    let done = entry(StepOutcome::Done, "1.1.0");
    assert!(done.settles("1.1.0"));
    assert!(done.settles("1.2.0"));
}

#[test]
fn a_declined_or_failed_step_is_settled_only_in_its_version() {
    for outcome in [StepOutcome::Declined, StepOutcome::Failed] {
        let ended = entry(outcome, "1.1.0");
        assert!(ended.settles("1.1.0"), "{outcome:?}");
        assert!(!ended.settles("1.2.0"), "{outcome:?}");
    }
}

#[test]
fn an_empty_record_runs_every_step() {
    let record = NameMigrationRecord::default();
    for step in MigrationStep::ALL {
        assert!(record.should_run(step, "1.1.0"), "{step:?}");
        assert_eq!(record.entry(step), None, "{step:?}");
    }
}

#[test]
fn a_recorded_step_reads_back_and_leaves_the_others() {
    let mut record = NameMigrationRecord::default();
    let declined = StepEntry {
        reason: Some("the person said no".to_string()),
        ..entry(StepOutcome::Declined, "1.1.0")
    };
    record
        .record(MigrationStep::BundleRename, &declined)
        .expect("record");

    assert_eq!(record.entry(MigrationStep::BundleRename), Some(declined));
    assert!(!record.should_run(MigrationStep::BundleRename, "1.1.0"));
    assert!(record.should_run(MigrationStep::BundleRename, "1.2.0"));
    assert!(record.should_run(MigrationStep::LoginItem, "1.1.0"));

    let done = entry(StepOutcome::Done, "1.2.0");
    record
        .record(MigrationStep::BundleRename, &done)
        .expect("record");
    assert_eq!(record.entry(MigrationStep::BundleRename), Some(done));
    assert!(!record.should_run(MigrationStep::BundleRename, "1.3.0"));
}

#[test]
fn the_record_survives_its_own_text() {
    let mut record = NameMigrationRecord::default();
    for (step, outcome) in [
        (MigrationStep::LoginItem, StepOutcome::Done),
        (MigrationStep::CommandLinks, StepOutcome::Failed),
        (MigrationStep::WindowsRunValue, StepOutcome::Declined),
    ] {
        record
            .record(step, &entry(outcome, "1.1.0"))
            .expect("record");
    }
    let text = record.to_json().expect("text");
    assert!(text.contains("\"login_item\""), "{text}");
    assert!(text.contains("\"failed\""), "{text}");
    assert_eq!(NameMigrationRecord::parse(&text).expect("parse"), record);
}

#[test]
fn an_entry_this_build_does_not_know_is_kept() {
    let text = r#"{"steps":{
        "shell_profile":{"outcome":"done","version":"1.4.0","at":"2026-11-18T09:00:00Z"},
        "login_item":{"outcome":"retired","version":"1.4.0"}
    }}"#;
    let mut record = NameMigrationRecord::parse(text).expect("parse");
    assert_eq!(record.entry(MigrationStep::LoginItem), None);
    assert!(record.should_run(MigrationStep::LoginItem, "1.1.0"));

    record
        .record(
            MigrationStep::LinuxAutostart,
            &entry(StepOutcome::Done, "1.1.0"),
        )
        .expect("record");
    let written = record.to_json().expect("text");
    assert!(written.contains("\"shell_profile\""), "{written}");
    assert!(written.contains("\"retired\""), "{written}");
}

#[test]
fn an_empty_object_is_an_empty_record() {
    assert_eq!(
        NameMigrationRecord::parse("{}").expect("parse"),
        NameMigrationRecord::default()
    );
}

#[test]
fn text_that_is_not_the_record_is_malformed() {
    for text in ["", "not json", "[1, 2]", r#"{"steps":3}"#] {
        assert!(
            matches!(
                NameMigrationRecord::parse(text),
                Err(NameMigrationError::Malformed(_))
            ),
            "{text}"
        );
    }
}
