use writ_plugin::transform::{
    CompositeTransform, TextTransform, TransformCategory, TransformError, TransformMetadata,
};

struct AppendChar(char);

impl TextTransform for AppendChar {
    fn id(&self) -> &str {
        "append_char"
    }
    fn metadata(&self) -> &TransformMetadata {
        unimplemented!("not needed for these tests")
    }
    fn apply(&self, input: &str) -> Result<String, TransformError> {
        Ok(format!("{input}{}", self.0))
    }
}

struct AlwaysFails;

impl TextTransform for AlwaysFails {
    fn id(&self) -> &str {
        "always_fails"
    }
    fn metadata(&self) -> &TransformMetadata {
        unimplemented!("not needed for these tests")
    }
    fn apply(&self, _input: &str) -> Result<String, TransformError> {
        Err(TransformError::Internal {
            reason: "boom".to_string(),
        })
    }
}

fn meta() -> TransformMetadata {
    TransformMetadata {
        label: "Test Composite".to_string(),
        description: "Composite under test.".to_string(),
        category: TransformCategory::Other,
    }
}

#[test]
fn composite_applies_steps_in_order() {
    let c = CompositeTransform::new(
        "abc".to_string(),
        meta(),
        vec![
            Box::new(AppendChar('a')),
            Box::new(AppendChar('b')),
            Box::new(AppendChar('c')),
        ],
    );
    assert_eq!(c.apply("x").unwrap(), "xabc");
}

#[test]
fn composite_exposes_id_and_metadata() {
    let c = CompositeTransform::new("my_id".to_string(), meta(), vec![]);
    assert_eq!(c.id(), "my_id");
    assert_eq!(c.metadata().label, "Test Composite");
    assert_eq!(c.metadata().category, TransformCategory::Other);
}

#[test]
fn composite_with_no_steps_returns_input_unchanged() {
    let c = CompositeTransform::new("noop".to_string(), meta(), vec![]);
    assert_eq!(c.apply("unchanged").unwrap(), "unchanged");
}

#[test]
fn composite_short_circuits_on_first_error() {
    let c = CompositeTransform::new(
        "fails".to_string(),
        meta(),
        vec![
            Box::new(AppendChar('a')),
            Box::new(AlwaysFails),
            Box::new(AppendChar('z')),
        ],
    );
    let err = c.apply("x").unwrap_err();
    assert_eq!(
        err,
        TransformError::Internal {
            reason: "boom".to_string()
        }
    );
}

#[test]
fn composite_nests_within_a_composite() {
    let inner = CompositeTransform::new(
        "inner".to_string(),
        meta(),
        vec![Box::new(AppendChar('a')), Box::new(AppendChar('b'))],
    );
    let outer = CompositeTransform::new(
        "outer".to_string(),
        meta(),
        vec![Box::new(inner), Box::new(AppendChar('c'))],
    );
    assert_eq!(outer.apply("x").unwrap(), "xabc");
}
