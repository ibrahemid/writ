//! Built-in transforms shipped with the host binary.
//!
//! Each transform is a unit struct implementing
//! [`TextTransform`](super::TextTransform). The [`register_builtins`]
//! helper registers all of them at host startup.

use crate::transform::{RegistryError, TransformRegistry};

mod dedent;
mod normalize_whitespace;
mod smart_to_straight_quotes;
mod trim_leading_whitespace;

pub use dedent::Dedent;
pub use normalize_whitespace::NormalizeWhitespace;
pub use smart_to_straight_quotes::SmartToStraightQuotes;
pub use trim_leading_whitespace::TrimLeadingWhitespace;

/// Registers every v1 built-in transform with `registry`.
///
/// Returns the first [`RegistryError`] if any registration fails (only
/// possible when the registry is not empty and an id collides).
pub fn register_builtins(registry: &mut TransformRegistry) -> Result<(), RegistryError> {
    registry.register(Box::new(TrimLeadingWhitespace))?;
    registry.register(Box::new(NormalizeWhitespace))?;
    registry.register(Box::new(SmartToStraightQuotes))?;
    registry.register(Box::new(Dedent))?;
    Ok(())
}
