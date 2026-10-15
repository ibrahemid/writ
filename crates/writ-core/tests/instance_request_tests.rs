use writ_core::instance::{
    is_wait_token, split_line_target, ContextAsk, OpenRequest, ReviewAsk, CONTEXT_ASK_FRESH_MS,
    MAX_WAIT_TOKEN_LEN, OPEN_REQUEST_FRESH_MS,
};
use writ_core::write_review::REVIEW_WAIT;

fn sent_at(sent_at_ms: i64) -> OpenRequest {
    OpenRequest {
        paths: vec!["/notes/a.md".to_string()],
        sent_at_ms,
        ..OpenRequest::default()
    }
}

fn nothing_exists(_: &str) -> bool {
    false
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

#[test]
fn a_request_without_lines_or_a_wait_reads_as_before() {
    let parsed: OpenRequest =
        serde_json::from_str(r#"{"paths":["/a.md"],"sent_at_ms":5}"#).expect("parse");
    assert!(parsed.lines.is_empty());
    assert_eq!(parsed.wait, None);
    assert!(parsed.links.is_empty());
    let written = serde_json::to_string(&parsed).expect("write");
    assert!(
        !written.contains("lines") && !written.contains("wait") && !written.contains("links"),
        "{written}"
    );
}

#[test]
fn a_request_carries_its_links_across_the_wire() {
    let request = OpenRequest {
        links: vec!["writ://open?path=Plan.md&line=12".to_string()],
        ..sent_at(5)
    };
    let written = serde_json::to_string(&request).expect("write");
    let parsed: OpenRequest = serde_json::from_str(&written).expect("parse");
    assert_eq!(parsed, request);
}

#[test]
fn a_context_ask_crosses_the_wire_and_asks_the_window_for_nothing() {
    let request = OpenRequest {
        context: Some(ContextAsk {
            token: "abc-1".to_string(),
            program: "Claude Code".to_string(),
        }),
        sent_at_ms: 5,
        ..OpenRequest::default()
    };
    let parsed: OpenRequest =
        serde_json::from_str(&serde_json::to_string(&request).expect("write")).expect("parse");
    assert_eq!(parsed, request);
    assert!(!parsed.shows_the_window());
    assert_eq!(
        parsed.context_ask(5).map(|ask| ask.program.as_str()),
        Some("Claude Code")
    );

    assert!(
        OpenRequest::default().shows_the_window(),
        "a bare request comes forward"
    );
    let with_paths = OpenRequest {
        context: request.context.clone(),
        ..sent_at(5)
    };
    assert!(with_paths.shows_the_window());
}

#[test]
fn a_review_ask_crosses_the_wire_opens_nothing_and_is_answered_while_the_write_waits() {
    let ask = |token: &str| OpenRequest {
        review: Some(ReviewAsk {
            token: token.to_string(),
            program: "claude-code".to_string(),
        }),
        ..OpenRequest::default()
    };
    let request = ask("rv-1");
    let parsed: OpenRequest =
        serde_json::from_str(&serde_json::to_string(&request).expect("write")).expect("parse");
    assert_eq!(parsed, request);
    assert!(!parsed.shows_the_window());

    let waited = i64::try_from(REVIEW_WAIT.as_millis()).expect("fits");
    assert!(ask("rv-1").review_ask(waited).is_some());
    assert!(ask("rv-1").review_ask(waited + 1).is_none());
    assert!(ask("../config.toml").review_ask(0).is_none());
    assert!(ask("").review_ask(0).is_none());
}

#[test]
fn a_stale_context_ask_or_one_whose_token_names_another_file_is_not_answered() {
    let ask = |token: &str| OpenRequest {
        context: Some(ContextAsk {
            token: token.to_string(),
            program: "Codex".to_string(),
        }),
        ..OpenRequest::default()
    };
    assert!(ask("ok-1").context_ask(CONTEXT_ASK_FRESH_MS).is_some());
    assert!(ask("ok-1").context_ask(CONTEXT_ASK_FRESH_MS + 1).is_none());
    assert!(ask("../config.toml").context_ask(0).is_none());
    assert!(ask("").context_ask(0).is_none());
}

#[test]
fn a_request_carries_its_lines_and_wait_token_across_the_wire() {
    let mut request = sent_at(5);
    request.lines.insert("/notes/a.md".to_string(), 12);
    request.wait = Some("abc-123".to_string());
    let parsed: OpenRequest =
        serde_json::from_str(&serde_json::to_string(&request).expect("write")).expect("parse");
    assert_eq!(parsed, request);
    assert_eq!(parsed.wait_token(), Some("abc-123"));
}

#[test]
fn a_wait_token_that_could_name_another_file_is_not_acted_on() {
    for token in ["", "../x", "a/b", "a\\b", "a.b", "a b", "ä"] {
        assert!(!is_wait_token(token), "{token:?}");
    }
    assert!(!is_wait_token(&"a".repeat(MAX_WAIT_TOKEN_LEN + 1)));
    assert!(is_wait_token(&"a".repeat(MAX_WAIT_TOKEN_LEN)));
    let request = OpenRequest {
        wait: Some("../../config.toml".to_string()),
        ..sent_at(5)
    };
    assert_eq!(request.wait_token(), None);
}

#[test]
fn a_trailing_line_is_split_off_a_path() {
    assert_eq!(
        split_line_target("Plan.md:12", nothing_exists),
        ("Plan.md".to_string(), Some(12))
    );
    assert_eq!(
        split_line_target("/notes/Plan.md:12:4", nothing_exists),
        ("/notes/Plan.md".to_string(), Some(12))
    );
    assert_eq!(
        split_line_target("Plan.md:0", nothing_exists),
        ("Plan.md".to_string(), Some(1))
    );
}

#[test]
fn a_path_with_no_line_is_left_whole() {
    for arg in ["Plan.md", "Plan.md:", "Plan.md:x", ":12", "Plan.md:12x"] {
        assert_eq!(
            split_line_target(arg, nothing_exists),
            (arg.to_string(), None),
            "{arg}"
        );
    }
}

#[test]
fn a_file_whose_name_ends_in_a_number_opens_as_written() {
    let exists = |arg: &str| arg == "log:12";
    assert_eq!(
        split_line_target("log:12", exists),
        ("log:12".to_string(), None)
    );
}

#[test]
fn a_windows_drive_is_not_a_line() {
    assert_eq!(
        split_line_target("C:\\notes\\Plan.md", nothing_exists),
        ("C:\\notes\\Plan.md".to_string(), None)
    );
    assert_eq!(
        split_line_target("C:12", nothing_exists),
        ("C:12".to_string(), None)
    );
    assert_eq!(
        split_line_target("C:\\notes\\Plan.md:7", nothing_exists),
        ("C:\\notes\\Plan.md".to_string(), Some(7))
    );
}

#[test]
fn a_wait_is_answered_when_the_last_of_its_files_closes() {
    let mut waits = writ_core::instance::WaitSet::default();
    assert!(!waits.register("t1", ["/a.md".to_string(), "/b.md".to_string()]));
    assert!(!waits.register("t2", ["/b.md".to_string()]));
    assert!(waits.closed(["/a.md"]).is_empty());
    assert_eq!(
        waits.closed(["/b.md"]),
        vec!["t1".to_string(), "t2".to_string()]
    );
    assert!(waits.is_empty());
}

#[test]
fn a_wait_with_nothing_to_open_is_answered_at_once() {
    let mut waits = writ_core::instance::WaitSet::default();
    assert!(waits.register("t1", Vec::<String>::new()));
    assert!(waits.is_empty());
}

#[test]
fn closing_a_file_nobody_waits_on_answers_nobody() {
    let mut waits = writ_core::instance::WaitSet::default();
    waits.register("t1", ["/a.md".to_string()]);
    assert!(waits.closed(["/other.md"]).is_empty());
    assert!(!waits.is_empty());
}
