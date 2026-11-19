use std::sync::OnceLock;

use crate::transform::{TextTransform, TransformCategory, TransformError, TransformMetadata};

const PUNCTUATION: [char; 6] = [',', '.', ';', ':', '!', '?'];

/// Removes stray whitespace immediately before sentence punctuation
/// (`, . ; : ! ?`), but only when the mark is followed by a boundary
/// (whitespace, end of line, end of input, or another such mark). That
/// guard leaves decimals (`3.14`), URLs (`http://…`), and ellipses
/// intact, since their punctuation is followed by a non-boundary
/// character.
#[derive(Debug, Default)]
pub struct FixPunctuationSpacing;

impl TextTransform for FixPunctuationSpacing {
    fn id(&self) -> &str {
        "fix_punctuation_spacing"
    }

    fn metadata(&self) -> &TransformMetadata {
        static META: OnceLock<TransformMetadata> = OnceLock::new();
        META.get_or_init(|| TransformMetadata {
            label: "Fix spacing before punctuation".to_string(),
            description: "Remove stray spaces before commas, periods, and other punctuation."
                .to_string(),
            category: TransformCategory::Punctuation,
        })
    }

    fn apply(&self, input: &str) -> Result<String, TransformError> {
        let mut out = String::with_capacity(input.len());
        let mut chars = input.chars().peekable();
        while let Some(ch) = chars.next() {
            if PUNCTUATION.contains(&ch) && is_boundary(chars.peek().copied()) {
                while out.ends_with([' ', '\t']) {
                    out.pop();
                }
            }
            out.push(ch);
        }
        Ok(out)
    }
}

fn is_boundary(next: Option<char>) -> bool {
    match next {
        None => true,
        Some(c) => c == ' ' || c == '\t' || c == '\r' || c == '\n' || PUNCTUATION.contains(&c),
    }
}
