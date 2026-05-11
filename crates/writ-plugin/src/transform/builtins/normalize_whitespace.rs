use std::sync::OnceLock;

use crate::transform::{
    TextTransform, TransformCategory, TransformError, TransformMetadata,
};

/// Collapses runs of two or more spaces or tabs inside each line down
/// to a single space. Leading whitespace, blank lines, and line endings
/// are preserved.
#[derive(Debug, Default)]
pub struct NormalizeWhitespace;

impl TextTransform for NormalizeWhitespace {
    fn id(&self) -> &str {
        "normalize_whitespace"
    }

    fn metadata(&self) -> &TransformMetadata {
        static META: OnceLock<TransformMetadata> = OnceLock::new();
        META.get_or_init(|| TransformMetadata {
            label: "Normalize Whitespace".to_string(),
            description: "Collapse multiple spaces/tabs inside each line to one space."
                .to_string(),
            category: TransformCategory::Whitespace,
        })
    }

    fn apply(&self, input: &str) -> Result<String, TransformError> {
        let mut out = String::with_capacity(input.len());
        for line in input.split_inclusive('\n') {
            let (content, ending) = split_line_ending(line);
            normalize_line_into(content, &mut out);
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

fn normalize_line_into(content: &str, out: &mut String) {
    let leading_len = content
        .bytes()
        .take_while(|b| *b == b' ' || *b == b'\t')
        .count();
    out.push_str(&content[..leading_len]);
    let rest = &content[leading_len..];

    let mut prev_was_ws = false;
    for ch in rest.chars() {
        if ch == ' ' || ch == '\t' {
            if !prev_was_ws {
                out.push(' ');
                prev_was_ws = true;
            }
        } else {
            out.push(ch);
            prev_was_ws = false;
        }
    }
}
