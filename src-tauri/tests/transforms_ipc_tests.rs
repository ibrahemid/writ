use writ_plugin::transform::builtins::register_builtins;
use writ_plugin::transform::{TransformError, TransformRegistry};

fn build_registry() -> TransformRegistry {
    let mut registry = TransformRegistry::new();
    register_builtins(&mut registry).expect("builtins register");
    registry
}

fn apply(registry: &TransformRegistry, id: &str, input: &str) -> Result<String, String> {
    let transform = registry
        .get(id)
        .ok_or_else(|| format!("unknown transform: {id}"))?;
    transform.apply(input).map_err(|e| e.to_string())
}

#[test]
fn list_returns_every_registered_builtin() {
    let registry = build_registry();
    let descriptors = registry.list();
    let ids: Vec<String> = descriptors.into_iter().map(|d| d.id).collect();
    assert!(ids.contains(&"trim_leading_whitespace".to_string()));
    assert!(ids.contains(&"normalize_whitespace".to_string()));
    assert!(ids.contains(&"smart_to_straight_quotes".to_string()));
    assert!(ids.contains(&"dedent".to_string()));
    assert_eq!(ids.len(), 4);
}

#[test]
fn apply_round_trips_trim_leading_whitespace() {
    let registry = build_registry();
    let out = apply(&registry, "trim_leading_whitespace", "   hello\n\t world").unwrap();
    assert_eq!(out, "hello\nworld");
}

#[test]
fn apply_round_trips_smart_quotes() {
    let registry = build_registry();
    let out = apply(
        &registry,
        "smart_to_straight_quotes",
        "\u{201C}hi\u{201D} \u{2019}",
    )
    .unwrap();
    assert_eq!(out, "\"hi\" '");
}

#[test]
fn apply_returns_error_for_unknown_id() {
    let registry = build_registry();
    let err = apply(&registry, "nonexistent_id", "anything").unwrap_err();
    assert!(err.contains("unknown transform"));
    assert!(err.contains("nonexistent_id"));
}

#[test]
fn transform_error_renders_to_string_for_ipc() {
    let err = TransformError::Internal {
        reason: "boom".to_string(),
    };
    assert_eq!(err.to_string(), "transform failed: boom");
}
