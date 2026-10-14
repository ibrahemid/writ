//! How often each palette command ran and when it last did.
//!
//! The palette ranks by these. They are derived from what the person does, so
//! they live in the database rather than in `config.toml` (ADR-048), which a
//! person edits and syncs and which a command used to rewrite on every run.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// What is known about one command's use.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CommandUsage {
    /// How many times the command has run.
    #[serde(default)]
    pub count: u32,
    /// Unix epoch milliseconds of the last run, `0` for never.
    #[serde(default)]
    pub last_used_ms: u64,
}

/// Runs of one command to add to what is stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandUse {
    /// The Writ command id.
    pub id: String,
    /// How many runs this adds.
    pub count: u32,
    /// Unix epoch milliseconds of the latest of them.
    pub last_used_ms: u64,
}

/// The `[commands.usage]` table a `config.toml` written before ADR-048
/// carries, for the one-time move into the database.
///
/// Tolerant like [`crate::config::FilesConfig::from_config_text`]: a file
/// that does not parse, a missing table and an entry of the wrong shape give
/// nothing for that part, so a broken config never stops the move of what
/// does read.
pub fn usage_from_config_text(text: &str) -> HashMap<String, CommandUsage> {
    let Ok(document) = text.parse::<toml::Table>() else {
        return HashMap::new();
    };
    let Some(toml::Value::Table(usage)) = document
        .get("commands")
        .and_then(|commands| commands.get("usage"))
    else {
        return HashMap::new();
    };
    usage
        .iter()
        .filter_map(|(id, entry)| {
            let entry: CommandUsage = entry.clone().try_into().ok()?;
            Some((id.clone(), entry))
        })
        .collect()
}
