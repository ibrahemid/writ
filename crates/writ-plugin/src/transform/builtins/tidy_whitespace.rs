use crate::transform::builtins::{
    Dedent, EnsureFinalNewline, NormalizeWhitespace, TrimTrailingWhitespace,
};
use crate::transform::{CompositeTransform, TransformCategory, TransformMetadata};

/// Builds the "Tidy Whitespace" composite: trim trailing whitespace,
/// dedent, collapse internal runs, then ensure a single final newline.
///
/// Punctuation spacing is intentionally excluded; see ADR-012.
pub fn tidy_whitespace() -> CompositeTransform {
    CompositeTransform::new(
        "tidy_whitespace".to_string(),
        TransformMetadata {
            label: "Tidy whitespace".to_string(),
            description:
                "Trim trailing spaces, remove shared indentation, collapse repeated spaces, and end with one newline."
                    .to_string(),
            category: TransformCategory::Whitespace,
        },
        vec![
            Box::new(TrimTrailingWhitespace),
            Box::new(Dedent),
            Box::new(NormalizeWhitespace),
            Box::new(EnsureFinalNewline),
        ],
    )
}
