//! The per-user name migration record on disk (ADR-059 decision 12).
//!
//! The record sits in the data folder as
//! [`jotra_core::name_migration::RECORD_FILE`]. Each platform step asks
//! [`run_migration_step`] to run it: a settled step is skipped, the outcome is
//! written back, and nothing here returns an error to the launch.

use std::fmt::Display;
use std::io;
use std::path::{Path, PathBuf};

use chrono::Utc;
use jotra_core::name_migration::{
    MigrationStep, NameMigrationError, NameMigrationRecord, StepEntry, StepOutcome, RECORD_FILE,
};
use tracing::{info, warn};

use crate::atomic::write_atomic;
use crate::errors::{StorageError, StorageResult};

/// How a step's own work ended, as the step reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepCompletion {
    /// The step did its work, or found it already done.
    Done,
    /// The person refused the prompt the step needed.
    Declined,
}

/// What [`run_migration_step`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepRun {
    /// The record already settles the step for this version; it did not run.
    AlreadySettled,
    /// The step ran and is done.
    Done,
    /// The step ran and the person declined it.
    Declined,
    /// The step ran and could not finish.
    Failed,
}

/// The record's path inside `data_dir`.
pub fn name_migration_record_path(data_dir: &Path) -> PathBuf {
    data_dir.join(RECORD_FILE)
}

/// Reads the record in `data_dir`.
///
/// A missing file is an empty record. A file that cannot be read or parsed is
/// logged and read as empty, so its steps run again rather than the launch
/// stopping on it.
pub fn read_name_migration_record(data_dir: &Path) -> NameMigrationRecord {
    let text = match std::fs::read_to_string(name_migration_record_path(data_dir)) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return NameMigrationRecord::default();
        }
        Err(error) => {
            warn!(%error, "name migration record unreadable; its steps run again");
            return NameMigrationRecord::default();
        }
    };
    NameMigrationRecord::parse(&text).unwrap_or_else(|error| {
        warn!(%error, "name migration record malformed; its steps run again");
        NameMigrationRecord::default()
    })
}

/// Writes `record` into `data_dir`, replacing the file in one step.
pub fn write_name_migration_record(
    data_dir: &Path,
    record: &NameMigrationRecord,
) -> StorageResult<()> {
    let text = record.to_json().map_err(into_storage_error)?;
    write_atomic(&name_migration_record_path(data_dir), text.as_bytes())?;
    Ok(())
}

/// Records how `step` ended in the record in `data_dir`, keeping every other
/// entry as it was.
pub fn record_migration_step(
    data_dir: &Path,
    step: MigrationStep,
    entry: &StepEntry,
) -> StorageResult<()> {
    let mut record = read_name_migration_record(data_dir);
    record.record(step, entry).map_err(into_storage_error)?;
    write_name_migration_record(data_dir, &record)
}

/// Runs `step` for app `version` unless the record in `data_dir` settles it,
/// and records the outcome.
///
/// `run` does the step's work. An error from it is a failed step, logged with
/// the step's name and recorded with its reason. A record that cannot be
/// written is logged; the step's outcome is still returned.
pub fn run_migration_step<E: Display>(
    data_dir: &Path,
    step: MigrationStep,
    version: &str,
    run: impl FnOnce() -> Result<StepCompletion, E>,
) -> StepRun {
    if !read_name_migration_record(data_dir).should_run(step, version) {
        return StepRun::AlreadySettled;
    }

    let (outcome, reason) = match run() {
        Ok(StepCompletion::Done) => (StepOutcome::Done, None),
        Ok(StepCompletion::Declined) => (StepOutcome::Declined, None),
        Err(error) => {
            let reason = error.to_string();
            warn!(step = step.key(), version, error = %reason, "name migration step failed");
            (StepOutcome::Failed, Some(reason))
        }
    };
    let entry = StepEntry {
        outcome,
        version: version.to_string(),
        at: Utc::now(),
        reason,
    };
    if let Err(error) = record_migration_step(data_dir, step, &entry) {
        warn!(step = step.key(), version, %error, "name migration record not written");
    }
    info!(
        step = step.key(),
        version,
        ?outcome,
        "name migration step ran"
    );

    match outcome {
        StepOutcome::Done => StepRun::Done,
        StepOutcome::Declined => StepRun::Declined,
        StepOutcome::Failed => StepRun::Failed,
    }
}

fn into_storage_error(error: NameMigrationError) -> StorageError {
    match error {
        NameMigrationError::Malformed(error) => StorageError::Json(error),
    }
}
