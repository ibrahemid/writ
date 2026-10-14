//! Edits in the form CodeMirror describes them (`ChangeSet.toJSON()`), applied
//! to the text they were made against.
//!
//! The editor's large tiers save by sending what changed rather than the whole
//! document (ADR-022). Positions and lengths are UTF-16 code units, as
//! CodeMirror counts them; the text here is UTF-8, so every section is walked
//! by character and a section that would end inside a surrogate pair is
//! refused rather than guessed at.

use serde::{Deserialize, Deserializer};
use serde_json::Value;

/// One stretch of a change set, in document order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangeSection {
    /// This many code units of the text kept as they are.
    Keep(usize),
    /// `deleted` code units removed and `inserted` put in their place.
    Replace {
        /// Code units of the text the section removes.
        deleted: usize,
        /// The text the section puts in their place, empty for a deletion.
        inserted: String,
    },
}

/// Why a change set could not be read or applied.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ChangeSetError {
    /// The value is not a change set in CodeMirror's JSON form.
    #[error("the changes are not in CodeMirror's form: {0}")]
    Malformed(String),
    /// The change set was made against a text of another length.
    #[error("the changes cover {expected} code units and the text holds {actual}")]
    LengthMismatch {
        /// The length the change set walks.
        expected: usize,
        /// The length of the text it was applied to.
        actual: usize,
    },
    /// A section ends between the two halves of a surrogate pair.
    #[error("the changes cut a character in two at code unit {0}")]
    SplitsCharacter(usize),
}

/// A change set as CodeMirror serialises it.
///
/// Each element of the JSON array is a section: a number keeps that many code
/// units, and an array `[deleted, ...lines]` removes `deleted` code units and
/// inserts the lines joined with `\n` (no lines is a deletion).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChangeSet {
    sections: Vec<ChangeSection>,
}

impl ChangeSet {
    /// A change set from its sections, in document order.
    #[must_use]
    pub fn new(sections: Vec<ChangeSection>) -> Self {
        Self { sections }
    }

    /// The sections, in document order.
    #[must_use]
    pub fn sections(&self) -> &[ChangeSection] {
        &self.sections
    }

    /// Reads a change set from CodeMirror's JSON form.
    ///
    /// # Errors
    ///
    /// [`ChangeSetError::Malformed`] for anything that is not that form.
    pub fn from_json(value: &Value) -> Result<Self, ChangeSetError> {
        let parts = value
            .as_array()
            .ok_or_else(|| ChangeSetError::Malformed("not an array".to_string()))?;
        let mut sections = Vec::with_capacity(parts.len());
        for (index, part) in parts.iter().enumerate() {
            let section = match part {
                Value::Number(_) => ChangeSection::Keep(length_of(part, index)?),
                Value::Array(items) => {
                    let Some((first, lines)) = items.split_first() else {
                        return Err(ChangeSetError::Malformed(format!(
                            "section {index} is an empty array"
                        )));
                    };
                    let deleted = length_of(first, index)?;
                    let mut inserted = String::new();
                    for (n, line) in lines.iter().enumerate() {
                        let line = line.as_str().ok_or_else(|| {
                            ChangeSetError::Malformed(format!(
                                "section {index} inserts something other than text"
                            ))
                        })?;
                        if n > 0 {
                            inserted.push('\n');
                        }
                        inserted.push_str(line);
                    }
                    ChangeSection::Replace { deleted, inserted }
                }
                _ => {
                    return Err(ChangeSetError::Malformed(format!(
                        "section {index} is neither a length nor a replacement"
                    )))
                }
            };
            sections.push(section);
        }
        Ok(Self { sections })
    }

    /// The length, in code units, of the text the change set was made against.
    #[must_use]
    pub fn base_length(&self) -> usize {
        self.sections
            .iter()
            .map(|section| match section {
                ChangeSection::Keep(n) => *n,
                ChangeSection::Replace { deleted, .. } => *deleted,
            })
            .sum()
    }

    /// The length, in code units, of the text the change set produces.
    #[must_use]
    pub fn result_length(&self) -> usize {
        self.sections
            .iter()
            .map(|section| match section {
                ChangeSection::Keep(n) => *n,
                ChangeSection::Replace { inserted, .. } => inserted.encode_utf16().count(),
            })
            .sum()
    }

    /// The text `base` becomes under these changes.
    ///
    /// # Errors
    ///
    /// [`ChangeSetError::LengthMismatch`] when the change set walks a text of
    /// another length, and [`ChangeSetError::SplitsCharacter`] when a section
    /// ends inside a surrogate pair.
    pub fn apply(&self, base: &str) -> Result<String, ChangeSetError> {
        let inserted: usize = self
            .sections
            .iter()
            .map(|section| match section {
                ChangeSection::Keep(_) => 0,
                ChangeSection::Replace { inserted, .. } => inserted.len(),
            })
            .sum();
        let mut out = String::with_capacity(base.len().saturating_add(inserted));
        let mut cursor = Utf16Cursor::new(base);
        for section in &self.sections {
            match section {
                ChangeSection::Keep(n) => {
                    let start = cursor.byte;
                    cursor.advance(*n, self)?;
                    out.push_str(&base[start..cursor.byte]);
                }
                ChangeSection::Replace { deleted, inserted } => {
                    cursor.advance(*deleted, self)?;
                    out.push_str(inserted);
                }
            }
        }
        if cursor.byte != base.len() {
            return Err(cursor.mismatch(self));
        }
        Ok(out)
    }
}

impl<'de> Deserialize<'de> for ChangeSet {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        Self::from_json(&value).map_err(serde::de::Error::custom)
    }
}

fn length_of(value: &Value, index: usize) -> Result<usize, ChangeSetError> {
    value
        .as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(|| ChangeSetError::Malformed(format!("section {index} has no whole length")))
}

/// A position in a UTF-8 text, counted both in bytes and in UTF-16 code units.
struct Utf16Cursor<'a> {
    text: &'a str,
    byte: usize,
    unit: usize,
}

impl<'a> Utf16Cursor<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            text,
            byte: 0,
            unit: 0,
        }
    }

    /// Moves `units` code units on.
    fn advance(&mut self, units: usize, changes: &ChangeSet) -> Result<(), ChangeSetError> {
        let bytes = self.text.as_bytes();
        let mut left = units;
        while left > 0 {
            let Some(&lead) = bytes.get(self.byte) else {
                return Err(self.mismatch(changes));
            };
            if lead < 0x80 {
                self.byte += 1;
                self.unit += 1;
                left -= 1;
                continue;
            }
            let ch = self.text[self.byte..]
                .chars()
                .next()
                .ok_or_else(|| self.mismatch(changes))?;
            let width = ch.len_utf16();
            if width > left {
                return Err(ChangeSetError::SplitsCharacter(self.unit + left));
            }
            self.byte += ch.len_utf8();
            self.unit += width;
            left -= width;
        }
        Ok(())
    }

    fn mismatch(&self, changes: &ChangeSet) -> ChangeSetError {
        ChangeSetError::LengthMismatch {
            expected: changes.base_length(),
            actual: self.text.encode_utf16().count(),
        }
    }
}
