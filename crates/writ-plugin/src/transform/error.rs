use thiserror::Error;

/// Errors a [`TextTransform`](super::TextTransform) may return when
/// applied to user input.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum TransformError {
    /// Input did not satisfy the transform's preconditions.
    #[error("invalid input: {reason}")]
    InvalidInput {
        /// Human-readable reason; surfaced to the user.
        reason: String,
    },
    /// Transform implementation failed for an internal reason.
    #[error("transform failed: {reason}")]
    Internal {
        /// Human-readable reason; surfaced to the user.
        reason: String,
    },
}

/// Errors returned by [`TransformRegistry`](super::TransformRegistry)
/// during registration or lookup.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum RegistryError {
    /// Two transforms tried to register with the same id.
    #[error("duplicate transform id: {id}")]
    DuplicateId {
        /// The conflicting id.
        id: String,
    },
}
