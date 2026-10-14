//! A renderer that panics fails its request instead of the process (ADR-045).

use writ_core::preview::{render_contained, RenderError, RENDERER_PANICKED};

#[test]
fn a_panicking_render_becomes_an_internal_error_without_the_message() {
    let document = String::from("private note text");

    let result: Result<String, RenderError> =
        render_contained(|| panic!("renderer bug while reading `{document}`"));

    assert_eq!(
        result,
        Err(RenderError::Internal {
            reason: RENDERER_PANICKED.to_string(),
        })
    );
    assert!(!RENDERER_PANICKED.contains("private note text"));
}

#[test]
fn a_render_that_returns_passes_through_unchanged() {
    assert_eq!(render_contained(|| Ok::<_, RenderError>(7)), Ok(7));

    let refused = RenderError::InvalidInput {
        reason: "not a diagram".to_string(),
    };
    assert_eq!(
        render_contained(|| Err::<(), _>(refused.clone())),
        Err(refused)
    );
}
