//! The startup read of `config.toml` (ADR-048).
//!
//! [`WritConfig::parse`] refuses a whole file over one bad value, which is
//! right for a reload (the running copy stays as it was) and wrong for a
//! launch, where there is no running copy to keep. [`WritConfig::parse_lenient`]
//! keeps every value that reads, gives each one that does not the value its
//! absence would give, and names it with the line it sits on.

use serde::Serialize;

use super::WritConfig;

/// One value in `config.toml` the startup read could not use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConfigDiagnostic {
    /// Dotted path of the value, such as `editor.font_size`. Empty when the
    /// file is not TOML at all, which has no key to name.
    pub key: String,
    /// One-based line the value, or the syntax error, sits on.
    pub line: u32,
    /// What was wrong with it, in the parser's words.
    pub message: String,
}

/// What the startup read made of a file.
#[derive(Debug, Clone, PartialEq)]
pub struct LenientConfig {
    /// The settings to run on: the file's own values, with the default in
    /// place of each one a diagnostic names.
    pub config: WritConfig,
    /// The values left out. Empty when the file read in full.
    pub diagnostics: Vec<ConfigDiagnostic>,
}

impl WritConfig {
    /// Reads `text` keeping every value that can be read.
    ///
    /// A file that reads in full is exactly [`WritConfig::parse`]. One that is
    /// not TOML runs on [`WritConfig::default`] with one diagnostic at the
    /// line the parser stopped on. Otherwise each value that fails is found by
    /// reading it on its own, removed, and named; what is left is read as a
    /// whole, so a removed value falls back to what a file without it reads
    /// as, and the tables around it keep their other values.
    pub fn parse_lenient(text: &str) -> LenientConfig {
        let strict_error = match Self::parse(text) {
            Ok(config) => {
                return LenientConfig {
                    config,
                    diagnostics: Vec::new(),
                }
            }
            Err(error) => error,
        };

        let mut table = match text.parse::<toml::Table>() {
            Ok(table) => table,
            Err(error) => return Self::unreadable(text, &error),
        };

        let mut bad_paths = Vec::new();
        for (key, value) in &table {
            find_unreadable(&[key.as_str()], value, &mut bad_paths);
        }
        let spans = toml::de::DeTable::parse(text).ok();
        let mut diagnostics: Vec<ConfigDiagnostic> = bad_paths
            .iter()
            .map(|(path, message)| ConfigDiagnostic {
                key: path.join("."),
                line: spans
                    .as_ref()
                    .map_or(1, |spans| line_of_path(text, spans.get_ref(), path)),
                message: message.clone(),
            })
            .collect();
        for (path, _) in &bad_paths {
            remove_path(&mut table, path);
        }

        match toml::Value::Table(table).try_into::<WritConfig>() {
            Ok(mut config) => {
                config.settle_apps();
                if diagnostics.is_empty() {
                    // The file fails as a whole while every value reads on its
                    // own, so there is no one value to leave out.
                    diagnostics.push(ConfigDiagnostic {
                        key: String::new(),
                        line: strict_error
                            .span()
                            .map_or(1, |span| line_at(text, span.start)),
                        message: strict_error.message().trim().to_string(),
                    });
                }
                LenientConfig {
                    config,
                    diagnostics,
                }
            }
            Err(error) => {
                diagnostics.push(ConfigDiagnostic {
                    key: String::new(),
                    line: 1,
                    message: error.message().trim().to_string(),
                });
                LenientConfig {
                    config: WritConfig::default(),
                    diagnostics,
                }
            }
        }
    }

    fn unreadable(text: &str, error: &toml::de::Error) -> LenientConfig {
        LenientConfig {
            config: WritConfig::default(),
            diagnostics: vec![ConfigDiagnostic {
                key: String::new(),
                line: error.span().map_or(1, |span| line_at(text, span.start)),
                message: error.message().trim().to_string(),
            }],
        }
    }
}

/// Collects the paths under `path` whose value cannot be read, each with the
/// reason. A table that fails is searched child by child, so the narrowest
/// failing value is the one named; a table whose children all read alone but
/// not together is named whole.
fn find_unreadable(path: &[&str], value: &toml::Value, out: &mut Vec<(Vec<String>, String)>) {
    let Err(error) = read_alone(path, value) else {
        return;
    };
    if let toml::Value::Table(children) = value {
        let before = out.len();
        for (key, child) in children {
            let mut child_path = path.to_vec();
            child_path.push(key.as_str());
            find_unreadable(&child_path, child, out);
        }
        if out.len() > before {
            return;
        }
    }
    out.push((
        path.iter().map(|segment| (*segment).to_string()).collect(),
        error.message().trim().to_string(),
    ));
}

/// Reads a config holding `value` at `path` and nothing else.
fn read_alone(path: &[&str], value: &toml::Value) -> Result<WritConfig, toml::de::Error> {
    let mut wrapped = value.clone();
    for segment in path.iter().rev() {
        let mut table = toml::Table::new();
        table.insert((*segment).to_string(), wrapped);
        wrapped = toml::Value::Table(table);
    }
    wrapped.try_into()
}

fn remove_path(table: &mut toml::Table, path: &[String]) {
    let Some((last, parents)) = path.split_last() else {
        return;
    };
    let mut current = table;
    for segment in parents {
        match current.get_mut(segment) {
            Some(toml::Value::Table(next)) => current = next,
            _ => return,
        }
    }
    current.remove(last);
}

/// The line of the deepest key on `path` the spanned document holds.
fn line_of_path(text: &str, root: &toml::de::DeTable<'_>, path: &[String]) -> u32 {
    let mut current = root;
    let mut line = 1;
    for segment in path {
        let Some((key, value)) = current
            .iter()
            .find(|(key, _)| key.get_ref() == segment.as_str())
        else {
            break;
        };
        line = line_at(text, key.span().start);
        match value.get_ref() {
            toml::de::DeValue::Table(next) => current = next,
            _ => break,
        }
    }
    line
}

fn line_at(text: &str, offset: usize) -> u32 {
    let end = offset.min(text.len());
    let newlines = text.as_bytes()[..end]
        .iter()
        .filter(|b| **b == b'\n')
        .count();
    u32::try_from(newlines + 1).unwrap_or(u32::MAX)
}
