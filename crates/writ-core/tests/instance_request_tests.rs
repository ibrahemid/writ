use writ_core::instance::{OpenRequest, OPEN_REQUEST_FRESH_MS};

fn sent_at(sent_at_ms: i64) -> OpenRequest {
    OpenRequest {
        paths: vec!["/notes/a.md".to_string()],
        sent_at_ms,
    }
}

#[test]
fn a_request_inside_the_window_is_fresh() {
    let now = 10_000_000;
    assert!(sent_at(now).is_fresh(now));
    assert!(sent_at(now - OPEN_REQUEST_FRESH_MS).is_fresh(now));
}

#[test]
fn a_request_older_than_the_window_is_stale() {
    let now = 10_000_000;
    assert!(!sent_at(now - OPEN_REQUEST_FRESH_MS - 1).is_fresh(now));
}

#[test]
fn a_request_stamped_ahead_of_the_clock_is_fresh() {
    assert!(sent_at(20_000).is_fresh(10_000));
}

#[test]
fn a_request_with_no_paths_parses() {
    let parsed: OpenRequest = serde_json::from_str(r#"{"sent_at_ms":5}"#).expect("parse");
    assert!(parsed.paths.is_empty());
}
