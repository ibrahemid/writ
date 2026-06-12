use std::sync::OnceLock;

use crate::transform::{TextTransform, TransformCategory, TransformError, TransformMetadata};

/// Produces the paste-ready form of a prompt document: strips leading YAML
/// frontmatter and HTML comments outside fenced code blocks, trims trailing
/// whitespace, and ends with exactly one final newline. Delegates to
/// [`writ_core::prompt::strip_for_prompt`] (ADR-015).
#[derive(Debug, Default)]
pub struct PreparePrompt;

impl TextTransform for PreparePrompt {
    fn id(&self) -> &str {
        "prepare_prompt"
    }

    fn metadata(&self) -> &TransformMetadata {
        static META: OnceLock<TransformMetadata> = OnceLock::new();
        META.get_or_init(|| TransformMetadata {
            label: "Prepare as Prompt".to_string(),
            description: "Strip frontmatter and HTML comments outside code fences; tidy trailing whitespace.".to_string(),
            category: TransformCategory::Other,
        })
    }

    fn apply(&self, input: &str) -> Result<String, TransformError> {
        Ok(writ_core::prompt::strip_for_prompt(input))
    }
}
