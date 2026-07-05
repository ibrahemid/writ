use std::collections::BTreeMap;

use crate::transform::error::{RegistryError, TransformError};
use crate::transform::metadata::{TransformDescriptor, TransformMetadata};

/// Synchronous, in-process text transform.
///
/// Implementations take a `&str` and return a transformed `String`.
/// Transforms must be `Send + Sync` so the host can hold the registry
/// behind a `RwLock` shared across IPC calls.
///
/// See ADR-006 for the loader-agnostic design rationale: a future
/// WASM/JS/dynlib loader implements this trait once on a host adapter.
pub trait TextTransform: Send + Sync {
    /// Stable id used by callers to invoke this transform.
    ///
    /// Must be lowercase snake_case and unique across the registry.
    fn id(&self) -> &str;

    /// Human-facing metadata for palette display.
    fn metadata(&self) -> &TransformMetadata;

    /// Applies the transform to `input` and returns the result.
    ///
    /// Implementations must be deterministic and side-effect free.
    fn apply(&self, input: &str) -> Result<String, TransformError>;
}

/// In-process registry of [`TextTransform`] trait objects.
///
/// `BTreeMap` keeps `list()` output deterministic without extra sorting.
pub struct TransformRegistry {
    transforms: BTreeMap<String, Box<dyn TextTransform>>,
}

impl TransformRegistry {
    /// Creates an empty registry.
    pub fn new() -> Self {
        Self {
            transforms: BTreeMap::new(),
        }
    }

    /// Registers a transform. Returns [`RegistryError::DuplicateId`] if
    /// a transform with the same id is already registered.
    pub fn register(&mut self, transform: Box<dyn TextTransform>) -> Result<(), RegistryError> {
        let id = transform.id().to_string();
        if self.transforms.contains_key(&id) {
            return Err(RegistryError::DuplicateId { id });
        }
        self.transforms.insert(id, transform);
        Ok(())
    }

    /// Returns a reference to the transform registered under `id`.
    pub fn get(&self, id: &str) -> Option<&dyn TextTransform> {
        self.transforms.get(id).map(|t| t.as_ref())
    }

    /// Returns one [`TransformDescriptor`] per registered transform,
    /// ordered by id.
    pub fn list(&self) -> Vec<TransformDescriptor> {
        self.transforms
            .values()
            .map(|t| TransformDescriptor {
                id: t.id().to_string(),
                metadata: t.metadata().clone(),
            })
            .collect()
    }

    /// Number of registered transforms.
    pub fn len(&self) -> usize {
        self.transforms.len()
    }

    /// Whether the registry holds zero transforms.
    pub fn is_empty(&self) -> bool {
        self.transforms.is_empty()
    }
}

impl Default for TransformRegistry {
    fn default() -> Self {
        Self::new()
    }
}
