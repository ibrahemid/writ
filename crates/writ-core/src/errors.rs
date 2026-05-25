use thiserror::Error;

/// Crate-wide error type for `writ-core` operations.
///
/// Every fallible operation in this crate returns [`WritResult`], whose
/// error arm is this enum. Variants are intentionally narrow so callers
/// can pattern-match on specific failure modes (for example,
/// [`WritError::BufferNotFound`]) without inspecting error messages.
#[derive(Error, Debug)]
pub enum WritError {
    /// The requested buffer id is not known to the manager.
    #[error("buffer not found: {id}")]
    BufferNotFound {
        /// Identifier that was looked up.
        id: String,
    },

    /// A buffer with the same id already exists.
    #[error("buffer already exists: {id}")]
    BufferAlreadyExists {
        /// Identifier that collided with an existing buffer.
        id: String,
    },

    /// Configuration failed validation.
    #[error("invalid config: {message}")]
    InvalidConfig {
        /// Human-readable description of the validation failure.
        message: String,
    },

    /// A buffer title failed sanitization.
    ///
    /// Raised when a title submitted to [`crate::buffer::manager::BufferManager::create_buffer`]
    /// contains path separators, parent / root / prefix components, or is
    /// otherwise unsafe to surface to filesystem-adjacent code.
    #[error("invalid buffer title: {reason}")]
    InvalidTitle {
        /// Human-readable description of why the title was rejected.
        reason: String,
    },

    /// An underlying I/O error propagated from `std::io`.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// A serialization or deserialization failure.
    #[error("serialization error: {0}")]
    Serialization(String),
}

/// Shorthand for a result whose error arm is [`WritError`].
pub type WritResult<T> = Result<T, WritError>;
