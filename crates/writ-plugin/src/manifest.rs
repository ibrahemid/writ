use serde::{Deserialize, Serialize};

/// Metadata declared by a Writ plugin on disk.
///
/// The manifest is serialized as TOML or JSON alongside the plugin entry
/// point and is loaded before any plugin code runs. It is the contract
/// Writ uses to discover, display, and load a plugin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    /// Unique, human-readable plugin identifier (for example `gist-export`).
    pub name: String,
    /// Semantic version string for this manifest.
    pub version: String,
    /// One-line description shown in plugin listings.
    pub description: String,
    /// Plugin author or maintainer display name.
    pub author: String,
    /// Relative path to the plugin's entry file, resolved from the
    /// manifest's directory.
    pub entry: String,
}
