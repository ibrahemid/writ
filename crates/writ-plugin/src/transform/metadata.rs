use serde::{Deserialize, Serialize};

/// Coarse grouping of transforms used to organize the palette listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransformCategory {
    /// Whitespace-related operations (trim, normalize).
    Whitespace,
    /// Punctuation rewrites (smart→straight quotes, dashes, ellipses).
    Punctuation,
    /// Indentation operations (dedent, retab).
    Indentation,
    /// Anything that does not fit the other categories.
    Other,
}

/// Human-facing metadata for a registered transform.
///
/// `metadata` is what the palette displays; the `id` returned by
/// [`TextTransform::id`](super::TextTransform::id) is what the host
/// invokes the transform by.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransformMetadata {
    /// Title shown in the command palette.
    pub label: String,
    /// One-line tooltip / subtext.
    pub description: String,
    /// Coarse grouping category.
    pub category: TransformCategory,
}

/// Pair of transform id and its metadata, used for IPC listing.
///
/// The trait itself is not `Serialize`, so the registry exposes this
/// flat struct for cross-process display.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransformDescriptor {
    /// Stable id used to invoke the transform (lowercase snake_case).
    pub id: String,
    /// Display metadata.
    pub metadata: TransformMetadata,
}
