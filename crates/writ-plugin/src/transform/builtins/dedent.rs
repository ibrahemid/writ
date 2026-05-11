use std::sync::OnceLock;

use crate::transform::{
    TextTransform, TransformCategory, TransformError, TransformMetadata,
};

/// Removes the longest common leading-whitespace prefix shared by all
/// non-blank lines.
#[derive(Debug, Default)]
pub struct Dedent;

impl TextTransform for Dedent {
    fn id(&self) -> &str {
        "dedent"
    }

    fn metadata(&self) -> &TransformMetadata {
        static META: OnceLock<TransformMetadata> = OnceLock::new();
        META.get_or_init(|| TransformMetadata {
            label: "Dedent".to_string(),
            description: "Remove shared leading indentation across non-blank lines.".to_string(),
            category: TransformCategory::Indentation,
        })
    }

    fn apply(&self, input: &str) -> Result<String, TransformError> {
        if input.is_empty() {
            return Ok(String::new());
        }

        let lines: Vec<(&str, &str)> = input.split_inclusive('\n').map(split_line_ending).collect();

        let first_non_blank = match lines.iter().find(|(content, _)| !is_blank(content)) {
            Some((content, _)) => *content,
            None => return Ok(input.to_string()),
        };
        let first_ws_len = leading_ws_len(first_non_blank);
        if first_ws_len == 0 {
            return Ok(input.to_string());
        }

        let mut prefix_len = first_ws_len;
        for (content, _) in lines.iter().filter(|(c, _)| !is_blank(c)) {
            let candidate = &first_non_blank[..prefix_len];
            while prefix_len > 0 && !content.starts_with(candidate) {
                prefix_len -= 1;
            }
            if prefix_len == 0 {
                return Ok(input.to_string());
            }
        }

        let common_prefix = &first_non_blank[..prefix_len];

        let mut out = String::with_capacity(input.len());
        for (content, ending) in lines {
            if is_blank(content) {
                out.push_str(content);
            } else if let Some(stripped) = content.strip_prefix(common_prefix) {
                out.push_str(stripped);
            } else {
                out.push_str(content);
            }
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

fn is_blank(line: &str) -> bool {
    line.bytes().all(|b| b == b' ' || b == b'\t')
}

fn leading_ws_len(line: &str) -> usize {
    line.bytes()
        .take_while(|b| *b == b' ' || *b == b'\t')
        .count()
}
