use std::sync::OnceLock;

use crate::transform::{TextTransform, TransformCategory, TransformError, TransformMetadata};

/// Replaces curly single and double quotes (and low / high-reversed
/// variants) with their ASCII straight equivalents.
#[derive(Debug, Default)]
pub struct SmartToStraightQuotes;

impl TextTransform for SmartToStraightQuotes {
    fn id(&self) -> &str {
        "smart_to_straight_quotes"
    }

    fn metadata(&self) -> &TransformMetadata {
        static META: OnceLock<TransformMetadata> = OnceLock::new();
        META.get_or_init(|| TransformMetadata {
            label: "Straighten quotes".to_string(),
            description: "Replace curly quotes with straight ones.".to_string(),
            category: TransformCategory::Punctuation,
        })
    }

    fn apply(&self, input: &str) -> Result<String, TransformError> {
        let mut out = String::with_capacity(input.len());
        for ch in input.chars() {
            match ch {
                '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' => out.push('\''),
                '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' => out.push('"'),
                other => out.push(other),
            }
        }
        Ok(out)
    }
}
