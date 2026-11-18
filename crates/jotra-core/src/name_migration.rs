//! The per-user record of the one-time steps the rename takes on each machine
//! (ADR-059 decision 12).
//!
//! The record lives in the data folder as [`RECORD_FILE`]. A step that is done
//! never runs again. A step that was declined or failed runs again only under
//! a different app version, so a refused prompt or a folder that will not take
//! a link is not asked about on every launch.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// The record's file name inside the data folder.
pub const RECORD_FILE: &str = "name-migration.json";

/// A per-user step the rename takes once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MigrationStep {
    /// The macOS bundle renamed to the new name.
    BundleRename,
    /// The hidden link the renamed macOS bundle leaves at the old name.
    BundleLink,
    /// The macOS login item registered under the new name.
    LoginItem,
    /// The command links pointed at the renamed bundle.
    CommandLinks,
    /// The Linux autostart entry renamed.
    LinuxAutostart,
    /// The Windows `Run` value moved to the new name.
    WindowsRunValue,
}

impl MigrationStep {
    /// Every step, in the order the record lists them.
    pub const ALL: [Self; 6] = [
        Self::BundleRename,
        Self::BundleLink,
        Self::LoginItem,
        Self::CommandLinks,
        Self::LinuxAutostart,
        Self::WindowsRunValue,
    ];

    /// The step's key in the record, and its name in the log.
    pub fn key(self) -> &'static str {
        match self {
            Self::BundleRename => "bundle_rename",
            Self::BundleLink => "bundle_link",
            Self::LoginItem => "login_item",
            Self::CommandLinks => "command_links",
            Self::LinuxAutostart => "linux_autostart",
            Self::WindowsRunValue => "windows_run_value",
        }
    }
}

/// How a step ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepOutcome {
    /// The step did what it does, or found it already done.
    Done,
    /// The person refused the prompt the step needed.
    Declined,
    /// The step could not finish.
    Failed,
}

/// One step's line in the record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepEntry {
    /// How the step ended.
    pub outcome: StepOutcome,
    /// The app version that ran it.
    pub version: String,
    /// When it ran.
    pub at: DateTime<Utc>,
    /// What went wrong, for a failed step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl StepEntry {
    /// Whether this entry means the step does not run in `version`: it is
    /// done, or it was declined or failed in this same version.
    pub fn settles(&self, version: &str) -> bool {
        match self.outcome {
            StepOutcome::Done => true,
            StepOutcome::Declined | StepOutcome::Failed => self.version == version,
        }
    }
}

/// Why the record's text could not be read or written.
#[derive(Debug, thiserror::Error)]
pub enum NameMigrationError {
    /// The text is not the record's JSON.
    #[error("the name migration record is not valid JSON: {0}")]
    Malformed(#[from] serde_json::Error),
}

/// What the per-user steps have done on this machine.
///
/// Entries are kept as written, so a step a newer build added survives being
/// read and written back by an older one, and an entry this build cannot read
/// counts as absent rather than spoiling the rest.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct NameMigrationRecord {
    #[serde(default)]
    steps: BTreeMap<String, serde_json::Value>,
}

impl NameMigrationRecord {
    /// Reads a record from the file's text.
    pub fn parse(text: &str) -> Result<Self, NameMigrationError> {
        Ok(serde_json::from_str(text)?)
    }

    /// The file's text for this record.
    pub fn to_json(&self) -> Result<String, NameMigrationError> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// The entry for `step`, if one is recorded and readable.
    pub fn entry(&self, step: MigrationStep) -> Option<StepEntry> {
        let value = self.steps.get(step.key())?;
        StepEntry::deserialize(value).ok()
    }

    /// Whether `step` runs in `version`.
    pub fn should_run(&self, step: MigrationStep, version: &str) -> bool {
        !self.entry(step).is_some_and(|entry| entry.settles(version))
    }

    /// Records how `step` ended, replacing any earlier entry for it.
    pub fn record(
        &mut self,
        step: MigrationStep,
        entry: &StepEntry,
    ) -> Result<(), NameMigrationError> {
        self.steps
            .insert(step.key().to_string(), serde_json::to_value(entry)?);
        Ok(())
    }
}
