use std::sync::OnceLock;

use crate::transform::{TextTransform, TransformCategory, TransformError, TransformMetadata};

/// Strips leading whitespace from every line, preserving line endings
/// (including CRLF) and trailing whitespace.
#[derive(Debug, Default)]
pub struct TrimLeadingWhitespace;

impl TextTransform for TrimLeadingWhitespace {
    fn id(&self) -> &str {
        "trim_leading_whitespace"
    }

    fn metadata(&self) -> &TransformMetadata {
        static META: OnceLock<TransformMetadata> = OnceLock::new();
        META.get_or_init(|| TransformMetadata {
            label: "Trim Leading Whitespace".to_string(),
            description: "Remove leading spaces and tabs from each line.".to_string(),
            category: TransformCategory::Whitespace,
        })
    }

    fn apply(&self, input: &str) -> Result<String, TransformError> {
        let mut out = String::with_capacity(input.len());
        for line in input.split_inclusive('\n') {
            let (content, ending) = split_line_ending(line);
            let trimmed = content.trim_start_matches([' ', '\t']);
            out.push_str(trimmed);
            out.push_str(ending);
        }
        Ok(out)
    }
}

fn split_line_ending(line: &str) -> (&str, &str) {
    if let Some(stripped) = line.strip_suffix("\r\n") {
        (stripped, "\r\n")
    } else if let Some(stripped) = line.strip_suffix('\n') {
        (stripped, "\n")
    } else {
        (line, "")
    }
}
