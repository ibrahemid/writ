//! Prompt-document helpers: token estimation, paste-ready stripping, and
//! `{{placeholder}}` templating.
//!
//! A prompt document is an ordinary buffer with three recognized
//! conventions: optional leading YAML frontmatter, HTML comments as author
//! notes, and `{{identifier}}` placeholders. Everything here is pure and
//! offline; see ADR-015 for the design rationale and the estimator's
//! documented accuracy band.

mod estimate;
mod placeholder;
mod strip;

pub use estimate::estimate_tokens;
pub use placeholder::{fill_placeholders, scan_placeholders};
pub use strip::strip_for_prompt;
