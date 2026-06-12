//! Content-type renderer registry — ADR-009 §D1.
//!
//! Trait-object registry that holds at most one [`ContentRenderer`] per
//! [`ContentTypeId`]. Mirrors the loader-agnostic precedent set by ADR-006's
//! `TextTransform` registry: first-party renderers register at startup; a
//! future external renderer adapter implements the same trait.

use std::collections::HashMap;

use super::types::{ContentRenderer, ContentTypeId, RendererCapabilities};

/// Error returned when registration cannot proceed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegisterError {
    /// A renderer is already registered under the given id.
    Duplicate(ContentTypeId),
}

impl std::fmt::Display for RegisterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Duplicate(id) => write!(f, "renderer already registered for `{id}`"),
        }
    }
}

impl std::error::Error for RegisterError {}

/// Owns the set of registered [`ContentRenderer`] implementations.
///
/// The registry is intentionally not `Clone`: it owns trait objects and is
/// shared via `Arc<ContentRendererRegistry>` from `AppState` in `src-tauri`.
pub struct ContentRendererRegistry {
    renderers: HashMap<ContentTypeId, Box<dyn ContentRenderer>>,
}

impl ContentRendererRegistry {
    /// Construct an empty registry.
    pub fn new() -> Self {
        Self {
            renderers: HashMap::new(),
        }
    }

    /// Register a renderer. Errors if a renderer is already registered under
    /// the same content type id — there is no implicit replacement.
    pub fn register(
        &mut self,
        renderer: Box<dyn ContentRenderer>,
    ) -> Result<(), RegisterError> {
        let id = renderer.content_type();
        if self.renderers.contains_key(&id) {
            return Err(RegisterError::Duplicate(id));
        }
        self.renderers.insert(id, renderer);
        Ok(())
    }

    /// Borrow the renderer for a given id, if registered.
    pub fn get(&self, id: &ContentTypeId) -> Option<&dyn ContentRenderer> {
        self.renderers.get(id).map(|b| b.as_ref())
    }

    /// Whether a renderer is registered for the given id.
    pub fn has(&self, id: &ContentTypeId) -> bool {
        self.renderers.contains_key(id)
    }

    /// Number of registered renderers.
    pub fn len(&self) -> usize {
        self.renderers.len()
    }

    /// Whether the registry has no entries.
    pub fn is_empty(&self) -> bool {
        self.renderers.is_empty()
    }

    /// Snapshot of every registered renderer's `(id, capabilities)` pair.
    pub fn list(&self) -> Vec<(ContentTypeId, RendererCapabilities)> {
        self.renderers
            .iter()
            .map(|(id, r)| (id.clone(), r.capabilities()))
            .collect()
    }
}

impl Default for ContentRendererRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preview::types::{
        ContentRenderer, ContentTypeId, RenderError, RenderOutput, RenderRequest,
        RendererCapabilities,
    };

    struct StubRenderer {
        id: ContentTypeId,
        caps: RendererCapabilities,
    }

    impl ContentRenderer for StubRenderer {
        fn content_type(&self) -> ContentTypeId {
            self.id.clone()
        }
        fn capabilities(&self) -> RendererCapabilities {
            self.caps
        }
        fn render(&self, request: RenderRequest) -> Result<RenderOutput, RenderError> {
            Ok(RenderOutput {
                document_html: format!("<stub>{}</stub>", request.buffer_text.len()),
                used_fallback_stylesheet: false,
                parser_warnings: vec![],
            })
        }
    }

    fn stub(id: &str) -> Box<StubRenderer> {
        Box::new(StubRenderer {
            id: ContentTypeId::new(id),
            caps: RendererCapabilities {
                supports_live_render: true,
                supports_print: true,
                max_safe_document_bytes: 50 * 1024 * 1024,
            },
        })
    }

    #[test]
    fn empty_registry_reports_empty() {
        let reg = ContentRendererRegistry::new();
        assert!(reg.is_empty());
        assert_eq!(reg.len(), 0);
        assert!(reg.get(&ContentTypeId::new("html")).is_none());
        assert!(!reg.has(&ContentTypeId::new("html")));
        assert!(reg.list().is_empty());
    }

    #[test]
    fn register_and_get_round_trip() {
        let mut reg = ContentRendererRegistry::new();
        reg.register(stub("html")).unwrap();
        reg.register(stub("markdown")).unwrap();
        assert_eq!(reg.len(), 2);
        assert!(reg.has(&ContentTypeId::new("html")));
        assert!(reg.has(&ContentTypeId::new("markdown")));
        assert!(!reg.has(&ContentTypeId::new("pdf")));
    }

    #[test]
    fn duplicate_registration_is_rejected() {
        let mut reg = ContentRendererRegistry::new();
        reg.register(stub("html")).unwrap();
        let err = reg.register(stub("html")).unwrap_err();
        assert_eq!(err, RegisterError::Duplicate(ContentTypeId::new("html")));
        // The first registration survives.
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn list_returns_id_and_capabilities() {
        let mut reg = ContentRendererRegistry::new();
        reg.register(stub("html")).unwrap();
        reg.register(stub("svg")).unwrap();
        let mut listed: Vec<String> = reg
            .list()
            .into_iter()
            .map(|(id, _)| id.as_str().to_string())
            .collect();
        listed.sort();
        assert_eq!(listed, vec!["html".to_string(), "svg".to_string()]);
    }

    #[test]
    fn renderer_render_is_callable_via_trait_object() {
        let mut reg = ContentRendererRegistry::new();
        reg.register(stub("html")).unwrap();
        let renderer = reg.get(&ContentTypeId::new("html")).unwrap();
        let out = renderer
            .render(RenderRequest {
                content_type: ContentTypeId::new("html"),
                buffer_text: "hi".to_string(),
                theme: Default::default(),
            })
            .unwrap();
        assert!(out.document_html.contains("<stub>2</stub>"));
    }
}
