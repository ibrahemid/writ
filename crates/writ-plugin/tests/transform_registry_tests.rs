use writ_plugin::transform::{
    RegistryError, TextTransform, TransformCategory, TransformDescriptor, TransformError,
    TransformMetadata, TransformRegistry,
};

struct EchoTransform {
    metadata: TransformMetadata,
}

impl EchoTransform {
    fn new() -> Self {
        Self {
            metadata: TransformMetadata {
                label: "Echo".to_string(),
                description: "Returns input unchanged.".to_string(),
                category: TransformCategory::Other,
            },
        }
    }
}

impl TextTransform for EchoTransform {
    fn id(&self) -> &str {
        "echo"
    }
    fn metadata(&self) -> &TransformMetadata {
        &self.metadata
    }
    fn apply(&self, input: &str) -> Result<String, TransformError> {
        Ok(input.to_string())
    }
}

struct AlphaTransform {
    metadata: TransformMetadata,
}

impl AlphaTransform {
    fn new() -> Self {
        Self {
            metadata: TransformMetadata {
                label: "Alpha".to_string(),
                description: "Alpha.".to_string(),
                category: TransformCategory::Other,
            },
        }
    }
}

impl TextTransform for AlphaTransform {
    fn id(&self) -> &str {
        "a_xform"
    }
    fn metadata(&self) -> &TransformMetadata {
        &self.metadata
    }
    fn apply(&self, input: &str) -> Result<String, TransformError> {
        Ok(input.to_string())
    }
}

#[test]
fn metadata_serializes_to_json_with_lowercase_category() {
    let meta = TransformMetadata {
        label: "Trim Leading Whitespace".to_string(),
        description: "Remove leading whitespace from each line.".to_string(),
        category: TransformCategory::Whitespace,
    };
    let json = serde_json::to_string(&meta).expect("serialize");
    assert!(json.contains("\"category\":\"whitespace\""));
    assert!(json.contains("\"label\":\"Trim Leading Whitespace\""));
}

#[test]
fn descriptor_carries_id_and_metadata() {
    let descriptor = TransformDescriptor {
        id: "trim_leading_whitespace".to_string(),
        metadata: TransformMetadata {
            label: "Trim".to_string(),
            description: "Trim.".to_string(),
            category: TransformCategory::Whitespace,
        },
    };
    let json = serde_json::to_string(&descriptor).expect("serialize");
    assert!(json.contains("\"id\":\"trim_leading_whitespace\""));
    assert!(json.contains("\"category\":\"whitespace\""));
}

#[test]
fn transform_error_displays_reason() {
    let err = TransformError::InvalidInput {
        reason: "input not utf-8".to_string(),
    };
    assert_eq!(err.to_string(), "invalid input: input not utf-8");
}

#[test]
fn registry_error_displays_duplicate_id() {
    let err = RegistryError::DuplicateId {
        id: "trim".to_string(),
    };
    assert_eq!(err.to_string(), "duplicate transform id: trim");
}

#[test]
fn registry_registers_and_looks_up_transform() {
    let mut registry = TransformRegistry::new();
    registry
        .register(Box::new(EchoTransform::new()))
        .expect("register");
    let t = registry.get("echo").expect("found");
    assert_eq!(t.id(), "echo");
    assert_eq!(t.apply("hello").unwrap(), "hello");
}

#[test]
fn registry_rejects_duplicate_id() {
    let mut registry = TransformRegistry::new();
    registry.register(Box::new(EchoTransform::new())).unwrap();
    let result = registry.register(Box::new(EchoTransform::new()));
    assert_eq!(
        result,
        Err(RegistryError::DuplicateId {
            id: "echo".to_string()
        })
    );
}

#[test]
fn registry_get_returns_none_for_unknown_id() {
    let registry = TransformRegistry::new();
    assert!(registry.get("nope").is_none());
}

#[test]
fn registry_list_returns_all_descriptors() {
    let mut registry = TransformRegistry::new();
    registry.register(Box::new(EchoTransform::new())).unwrap();
    let descriptors = registry.list();
    assert_eq!(descriptors.len(), 1);
    assert_eq!(descriptors[0].id, "echo");
    assert_eq!(descriptors[0].metadata.label, "Echo");
}

#[test]
fn registry_list_is_sorted_by_id() {
    let mut registry = TransformRegistry::new();
    registry.register(Box::new(EchoTransform::new())).unwrap();
    registry.register(Box::new(AlphaTransform::new())).unwrap();
    let ids: Vec<String> = registry.list().into_iter().map(|d| d.id).collect();
    assert_eq!(ids, vec!["a_xform".to_string(), "echo".to_string()]);
}
