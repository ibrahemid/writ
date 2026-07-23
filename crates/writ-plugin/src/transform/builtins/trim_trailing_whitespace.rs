use std::sync::OnceLock;

use crate::transform::{TextTransform, TransformCategory, TransformError, TransformMetadata};

/// Strips trailing spaces and tabs from every line, preserving line
/// endings (including CRLF) and leading whitespace.
#[derive(Debug, Default)]
pub struct TrimTrailingWhitespace;

impl TextTransform for TrimTrailingWhitespace {
    fn id(&self) -> &str {
        "trim_trailing_whitespace"
    }

    fn metadata(&self) -> &TransformMetadata {
        static META: OnceLock<TransformMetadata> = OnceLock::new();
        META.get_or_init(|| TransformMetadata {
            label: "Trim trailing spaces".to_string(),
            description: "Remove spaces and tabs from the end of each line.".to_string(),
            category: TransformCategory::Whitespace,
        })
    }

    fn apply(&self, input: &str) -> Result<String, TransformError> {
        let mut out = String::with_capacity(input.len());
        for line in input.split_inclusive('\n') {
            let (content, ending) = split_line_ending(line);
            out.push_str(content.trim_end_matches([' ', '\t']));
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
