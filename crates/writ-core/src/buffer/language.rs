//! The language chosen for a buffer.
//!
//! The editor owns the list of languages, so this module cannot say whether a
//! name is one it knows; an id it does not know is shown as if nothing was
//! chosen. What is checked here is that a choice is a short, printable name,
//! so a row never stores text no picker could have sent.

use crate::errors::{WritError, WritResult};

/// Longest language id a buffer records, in characters. The longest name the
/// editor offers is 25 characters.
pub const LANGUAGE_CHOICE_MAX_CHARS: usize = 64;

/// Checks a language choice before a buffer records it.
///
/// `None` clears the choice and always passes. An id passes when it is not
/// empty, has no surrounding whitespace, holds no control character and is at
/// most [`LANGUAGE_CHOICE_MAX_CHARS`] characters long.
///
/// # Errors
///
/// [`WritError::InvalidLanguage`] naming the rule the id breaks.
pub fn validate_language_choice(choice: Option<&str>) -> WritResult<()> {
    let Some(id) = choice else {
        return Ok(());
    };
    let reason = if id.is_empty() {
        Some("the language id is empty".to_string())
    } else if id.trim() != id {
        Some("the language id starts or ends with whitespace".to_string())
    } else if id.chars().any(char::is_control) {
        Some("the language id holds a control character".to_string())
    } else if id.chars().count() > LANGUAGE_CHOICE_MAX_CHARS {
        Some(format!(
            "the language id is longer than {LANGUAGE_CHOICE_MAX_CHARS} characters"
        ))
    } else {
        None
    };
    match reason {
        Some(reason) => Err(WritError::InvalidLanguage { reason }),
        None => Ok(()),
    }
}
