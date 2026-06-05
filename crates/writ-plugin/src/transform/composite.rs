use crate::transform::error::TransformError;
use crate::transform::metadata::TransformMetadata;
use crate::transform::registry::TextTransform;

/// A [`TextTransform`] that runs an ordered list of sub-transforms,
/// feeding each step's output into the next.
///
/// A composite is itself a `TextTransform`, so it registers, lists, and
/// applies through the same surface as any atomic transform (see ADR-012).
/// `apply` short-circuits on the first sub-transform error.
pub struct CompositeTransform {
    id: String,
    metadata: TransformMetadata,
    steps: Vec<Box<dyn TextTransform>>,
}

impl CompositeTransform {
    /// Builds a composite from a stable `id`, display `metadata`, and an
    /// ordered list of `steps`.
    pub fn new(
        id: String,
        metadata: TransformMetadata,
        steps: Vec<Box<dyn TextTransform>>,
    ) -> Self {
        Self {
            id,
            metadata,
            steps,
        }
    }
}

impl TextTransform for CompositeTransform {
    fn id(&self) -> &str {
        &self.id
    }

    fn metadata(&self) -> &TransformMetadata {
        &self.metadata
    }

    fn apply(&self, input: &str) -> Result<String, TransformError> {
        let mut current = input.to_string();
        for step in &self.steps {
            current = step.apply(&current)?;
        }
        Ok(current)
    }
}
