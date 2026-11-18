//! The per-user name migration record on disk (ADR-059 decision 12).

use std::cell::Cell;
use std::fs;

use chrono::Utc;
use jotra_core::name_migration::{MigrationStep, NameMigrationRecord, StepEntry, StepOutcome};
use jotra_storage::name_migration::{
    name_migration_record_path, read_name_migration_record, record_migration_step,
    run_migration_step, write_name_migration_record, StepCompletion, StepRun,
};

#[derive(Debug)]
struct StepFailure(&'static str);

impl std::fmt::Display for StepFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

fn done() -> Result<StepCompletion, StepFailure> {
    Ok(StepCompletion::Done)
}

#[test]
fn the_record_lives_in_the_data_folder() {
    let data = tempfile::tempdir().expect("data");
    assert_eq!(
        name_migration_record_path(data.path()),
        data.path().join("name-migration.json")
    );
}

#[test]
fn a_missing_record_is_empty() {
    let data = tempfile::tempdir().expect("data");
    assert_eq!(
        read_name_migration_record(data.path()),
        NameMigrationRecord::default()
    );
    assert!(!name_migration_record_path(data.path()).exists());
}

#[test]
fn a_corrupt_record_reads_as_empty_and_is_replaced() {
    let data = tempfile::tempdir().expect("data");
    fs::write(name_migration_record_path(data.path()), b"{ not json").expect("write");
    assert_eq!(
        read_name_migration_record(data.path()),
        NameMigrationRecord::default()
    );

    let run = run_migration_step(data.path(), MigrationStep::LoginItem, "1.1.0", done);
    assert_eq!(run, StepRun::Done);
    assert_eq!(
        read_name_migration_record(data.path())
            .entry(MigrationStep::LoginItem)
            .map(|entry| entry.outcome),
        Some(StepOutcome::Done)
    );
}

#[test]
fn a_written_record_reads_back() {
    let data = tempfile::tempdir().expect("data");
    let mut record = NameMigrationRecord::default();
    let entry = StepEntry {
        outcome: StepOutcome::Declined,
        version: "1.1.0".to_string(),
        at: Utc::now(),
        reason: None,
    };
    record
        .record(MigrationStep::BundleRename, &entry)
        .expect("record");
    write_name_migration_record(data.path(), &record).expect("write");
    assert_eq!(read_name_migration_record(data.path()), record);
}

#[test]
fn recording_a_step_keeps_every_other_entry() {
    let data = tempfile::tempdir().expect("data");
    fs::write(
        name_migration_record_path(data.path()),
        r#"{"steps":{"shell_profile":{"outcome":"done","version":"1.4.0"}}}"#,
    )
    .expect("write");
    let entry = StepEntry {
        outcome: StepOutcome::Done,
        version: "1.1.0".to_string(),
        at: Utc::now(),
        reason: None,
    };
    record_migration_step(data.path(), MigrationStep::CommandLinks, &entry).expect("record");
    record_migration_step(data.path(), MigrationStep::LinuxAutostart, &entry).expect("record");

    let record = read_name_migration_record(data.path());
    assert_eq!(
        record.entry(MigrationStep::CommandLinks),
        Some(entry.clone())
    );
    assert_eq!(record.entry(MigrationStep::LinuxAutostart), Some(entry));
    let text = fs::read_to_string(name_migration_record_path(data.path())).expect("read");
    assert!(text.contains("\"shell_profile\""), "{text}");
}

#[test]
fn a_done_step_does_not_run_again() {
    let data = tempfile::tempdir().expect("data");
    let calls = Cell::new(0);
    let run = |version: &str| {
        run_migration_step(data.path(), MigrationStep::BundleRename, version, || {
            calls.set(calls.get() + 1);
            done()
        })
    };

    assert_eq!(run("1.1.0"), StepRun::Done);
    assert_eq!(run("1.1.0"), StepRun::AlreadySettled);
    assert_eq!(run("1.2.0"), StepRun::AlreadySettled);
    assert_eq!(calls.get(), 1);
}

#[test]
fn a_declined_step_waits_for_the_next_version() {
    let data = tempfile::tempdir().expect("data");
    let calls = Cell::new(0);
    let decline = || {
        calls.set(calls.get() + 1);
        Ok::<_, StepFailure>(StepCompletion::Declined)
    };

    assert_eq!(
        run_migration_step(data.path(), MigrationStep::BundleRename, "1.1.0", decline),
        StepRun::Declined
    );
    assert_eq!(
        run_migration_step(data.path(), MigrationStep::BundleRename, "1.1.0", decline),
        StepRun::AlreadySettled
    );
    let entry = read_name_migration_record(data.path())
        .entry(MigrationStep::BundleRename)
        .expect("entry");
    assert_eq!(entry.outcome, StepOutcome::Declined);
    assert_eq!(entry.version, "1.1.0");

    assert_eq!(
        run_migration_step(data.path(), MigrationStep::BundleRename, "1.2.0", done),
        StepRun::Done
    );
    assert_eq!(calls.get(), 1);
}

#[test]
fn a_failed_step_records_its_reason_and_runs_in_the_next_version() {
    let data = tempfile::tempdir().expect("data");

    let run = run_migration_step(data.path(), MigrationStep::WindowsRunValue, "1.1.0", || {
        Err(StepFailure("access denied"))
    });
    assert_eq!(run, StepRun::Failed);
    let entry = read_name_migration_record(data.path())
        .entry(MigrationStep::WindowsRunValue)
        .expect("entry");
    assert_eq!(entry.outcome, StepOutcome::Failed);
    assert_eq!(entry.reason.as_deref(), Some("access denied"));

    assert_eq!(
        run_migration_step(data.path(), MigrationStep::WindowsRunValue, "1.1.0", done),
        StepRun::AlreadySettled
    );
    assert_eq!(
        run_migration_step(data.path(), MigrationStep::WindowsRunValue, "1.2.0", done),
        StepRun::Done
    );
}

#[test]
fn a_record_that_cannot_be_written_still_reports_the_step() {
    let root = tempfile::tempdir().expect("root");
    let not_a_folder = root.path().join("data");
    fs::write(&not_a_folder, b"").expect("file");

    let run = run_migration_step(&not_a_folder, MigrationStep::LoginItem, "1.1.0", done);
    assert_eq!(run, StepRun::Done);
    assert_eq!(
        read_name_migration_record(&not_a_folder),
        NameMigrationRecord::default()
    );
}
