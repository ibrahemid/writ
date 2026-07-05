use std::sync::OnceLock;

use crate::transform::{TextTransform, TransformCategory, TransformError, TransformMetadata};

/// Guarantees the text ends with exactly one trailing line ending,
/// collapsing any run of trailing newlines. Empty input (or input made up
/// only of newlines) stays empty. The line ending is CRLF if the input
/// contains any CRLF, otherwise LF.
#[derive(Debug, Default)]
pub struct EnsureFinalNewline;

impl TextTransform for EnsureFinalNewline {
    fn id(&self) -> &str {
        "ensure_final_newline"
    }

    fn metadata(&self) -> &TransformMetadata {
        static META: OnceLock<TransformMetadata> = OnceLock::new();
        META.get_or_init(|| TransformMetadata {
            label: "Ensure Final Newline".to_string(),
            description: "End the text with exactly one trailing newline.".to_string(),
            category: TransformCategory::Whitespace,
        })
    }

    fn apply(&self, input: &str) -> Result<String, TransformError> {
        let trimmed = input.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            return Ok(String::new());
        }
        let ending = if input.contains("\r\n") { "\r\n" } else { "\n" };
        let mut out = String::with_capacity(trimmed.len() + ending.len());
        out.push_str(trimmed);
        out.push_str(ending);
        Ok(out)
    }
}
