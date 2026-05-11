//! Text-transform registry surface.
//!
//! `transform` defines the [`TextTransform`] trait, a registry that
//! holds trait objects, and the metadata/error types the host exposes
//! to the frontend over IPC. See ADR-006 for the design rationale.

pub mod builtins;
mod error;
mod metadata;
mod registry;

pub use error::{RegistryError, TransformError};
pub use metadata::{TransformCategory, TransformDescriptor, TransformMetadata};
pub use registry::{TextTransform, TransformRegistry};
