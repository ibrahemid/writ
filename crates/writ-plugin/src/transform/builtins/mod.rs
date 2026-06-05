//! Built-in transforms shipped with the host binary.
//!
//! Each atomic transform is a unit struct implementing
//! [`TextTransform`](super::TextTransform); [`tidy_whitespace`] builds a
//! [`CompositeTransform`](super::CompositeTransform) from several of them.
//! The [`register_builtins`] helper registers all of them at host startup.

use crate::transform::{RegistryError, TransformRegistry};

mod dedent;
mod ensure_final_newline;
mod fix_punctuation_spacing;
mod normalize_whitespace;
mod smart_to_straight_quotes;
mod tidy_whitespace;
mod trim_leading_whitespace;
mod trim_trailing_whitespace;

pub use dedent::Dedent;
pub use ensure_final_newline::EnsureFinalNewline;
pub use fix_punctuation_spacing::FixPunctuationSpacing;
pub use normalize_whitespace::NormalizeWhitespace;
pub use smart_to_straight_quotes::SmartToStraightQuotes;
pub use tidy_whitespace::tidy_whitespace;
pub use trim_leading_whitespace::TrimLeadingWhitespace;
pub use trim_trailing_whitespace::TrimTrailingWhitespace;

/// Registers every built-in transform with `registry`.
///
/// Returns the first [`RegistryError`] if any registration fails (only
/// possible when the registry is not empty and an id collides).
pub fn register_builtins(registry: &mut TransformRegistry) -> Result<(), RegistryError> {
    registry.register(Box::new(TrimLeadingWhitespace))?;
    registry.register(Box::new(TrimTrailingWhitespace))?;
    registry.register(Box::new(NormalizeWhitespace))?;
    registry.register(Box::new(SmartToStraightQuotes))?;
    registry.register(Box::new(Dedent))?;
    registry.register(Box::new(EnsureFinalNewline))?;
    registry.register(Box::new(FixPunctuationSpacing))?;
    registry.register(Box::new(tidy_whitespace()))?;
    Ok(())
}
