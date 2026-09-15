//! The line diff a proposal card is drawn from.

use writ_core::diff::{line_diff, DiffError, Hunk, LineKind, MAX_DIFF_BYTES};

/// Ten numbered lines, as a document whose middle is far from both edges.
fn ten_lines() -> String {
    (1..=10)
        .map(|n| format!("line {n}\n"))
        .collect::<Vec<_>>()
        .join("")
}

/// The text `hunks` turn `before` into, rebuilt line by line.
///
/// A hunk names where it starts on each side, so replaying one means copying
/// the untouched lines up to that point and then following its own lines. If
/// the numbers are wrong, the rebuilt text is wrong.
fn replay(before: &str, hunks: &[Hunk]) -> String {
    let source: Vec<&str> = before.lines().collect();
    let mut out: Vec<String> = Vec::new();
    let mut cursor = 0usize;
    for hunk in hunks {
        let start = hunk.before_start.saturating_sub(1);
        while cursor < start {
            out.push(source[cursor].to_string());
            cursor += 1;
        }
        for line in &hunk.lines {
            match line.kind {
                LineKind::Context => {
                    out.push(line.text.clone());
                    cursor += 1;
                }
                LineKind::Removed => cursor += 1,
                LineKind::Added => out.push(line.text.clone()),
            }
        }
    }
    while cursor < source.len() {
        out.push(source[cursor].to_string());
        cursor += 1;
    }
    out.join("\n")
}

/// The lines of one kind a hunk carries, in order.
fn of_kind(hunk: &Hunk, kind: LineKind) -> Vec<&str> {
    hunk.lines
        .iter()
        .filter(|line| line.kind == kind)
        .map(|line| line.text.as_str())
        .collect()
}

#[test]
fn two_empty_texts_differ_nowhere() {
    assert_eq!(line_diff("", ""), Ok(Vec::new()));
}

#[test]
fn identical_texts_differ_nowhere() {
    let text = ten_lines();
    assert_eq!(line_diff(&text, &text), Ok(Vec::new()));
}

#[test]
fn an_empty_before_makes_every_line_added() {
    let hunks = line_diff("", "one\ntwo\n").expect("under the limit");
    assert_eq!(hunks.len(), 1);
    assert_eq!(hunks[0].before_start, 0);
    assert_eq!(hunks[0].after_start, 1);
    assert_eq!(of_kind(&hunks[0], LineKind::Added), vec!["one", "two"]);
    assert!(of_kind(&hunks[0], LineKind::Removed).is_empty());
    assert!(of_kind(&hunks[0], LineKind::Context).is_empty());
}

#[test]
fn an_empty_after_makes_every_line_removed() {
    let hunks = line_diff("one\ntwo\n", "").expect("under the limit");
    assert_eq!(hunks.len(), 1);
    assert_eq!(hunks[0].before_start, 1);
    assert_eq!(hunks[0].after_start, 0);
    assert_eq!(of_kind(&hunks[0], LineKind::Removed), vec!["one", "two"]);
    assert!(of_kind(&hunks[0], LineKind::Added).is_empty());
}

#[test]
fn a_trailing_newline_makes_no_empty_last_line() {
    assert_eq!(line_diff("one\ntwo\n", "one\ntwo"), Ok(Vec::new()));
}

#[test]
fn one_changed_line_in_ten_is_one_hunk_with_three_lines_of_context() {
    let before = ten_lines();
    let after = before.replace("line 5\n", "line five\n");
    let hunks = line_diff(&before, &after).expect("under the limit");
    assert_eq!(hunks.len(), 1);
    let hunk = &hunks[0];
    assert_eq!(hunk.before_start, 2);
    assert_eq!(hunk.after_start, 2);
    assert_eq!(of_kind(hunk, LineKind::Removed), vec!["line 5"]);
    assert_eq!(of_kind(hunk, LineKind::Added), vec!["line five"]);
    assert_eq!(
        of_kind(hunk, LineKind::Context),
        vec!["line 2", "line 3", "line 4", "line 6", "line 7", "line 8"]
    );
    assert_eq!(replay(&before, &hunks), after.trim_end_matches('\n'));
}

#[test]
fn two_changes_twenty_lines_apart_are_two_hunks() {
    let before: String = (1..=30).map(|n| format!("line {n}\n")).collect();
    let after = before
        .replace("line 3\n", "line three\n")
        .replace("line 23\n", "line twenty-three\n");
    let hunks = line_diff(&before, &after).expect("under the limit");
    assert_eq!(hunks.len(), 2);
    assert_eq!(of_kind(&hunks[0], LineKind::Added), vec!["line three"]);
    assert_eq!(
        of_kind(&hunks[1], LineKind::Added),
        vec!["line twenty-three"]
    );
    assert_eq!(hunks[1].before_start, 20);
    assert_eq!(replay(&before, &hunks), after.trim_end_matches('\n'));
}

#[test]
fn two_changes_two_lines_apart_are_one_hunk() {
    let before: String = (1..=30).map(|n| format!("line {n}\n")).collect();
    let after = before
        .replace("line 10\n", "line ten\n")
        .replace("line 13\n", "line thirteen\n");
    let hunks = line_diff(&before, &after).expect("under the limit");
    assert_eq!(hunks.len(), 1);
    assert_eq!(
        of_kind(&hunks[0], LineKind::Added),
        vec!["line ten", "line thirteen"]
    );
    assert_eq!(replay(&before, &hunks), after.trim_end_matches('\n'));
}

#[test]
fn a_side_over_two_megabytes_is_refused() {
    let big = "x".repeat(MAX_DIFF_BYTES + 1);
    assert_eq!(
        line_diff(&big, "small"),
        Err(DiffError::TooLarge {
            bytes: MAX_DIFF_BYTES + 1,
            limit: MAX_DIFF_BYTES,
        })
    );
    assert_eq!(
        line_diff("small", &big),
        Err(DiffError::TooLarge {
            bytes: MAX_DIFF_BYTES + 1,
            limit: MAX_DIFF_BYTES,
        })
    );
}

#[test]
fn a_side_of_exactly_two_megabytes_is_diffed() {
    let big = "x".repeat(MAX_DIFF_BYTES);
    assert!(line_diff(&big, &big).is_ok());
}

#[test]
fn the_hunks_rebuild_the_second_text() {
    let pairs = [
        ("a\nb\nc\n", "a\nB\nc\n"),
        ("a\nb\nc\n", "c\nb\na\n"),
        ("", "one\n"),
        ("one\n", ""),
        ("a\n", "a\nb\nc\nd\ne\nf\ng\nh\n"),
        ("a\nb\nc\nd\ne\nf\ng\nh\n", "a\n"),
        (
            "intro\n\nbody one\nbody two\n\nend\n",
            "intro\n\nbody one\nbody two changed\nbody three\n\nend\n",
        ),
        (
            "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n",
            "1\n2\n3\n4x\n5\n6\n7\n8x\n9\n10\n",
        ),
        ("same\n", "same\n"),
        ("\n\n\n", "\n\n"),
    ];
    for (before, after) in pairs {
        let hunks = line_diff(before, after).expect("under the limit");
        assert_eq!(
            replay(before, &hunks),
            after.lines().collect::<Vec<_>>().join("\n"),
            "rebuilding {after:?} from {before:?}"
        );
    }
}

#[test]
fn a_change_larger_than_the_search_budget_lists_every_line() {
    let before: String = (0..4000).map(|n| format!("before {n}\n")).collect();
    let after: String = (0..4000).map(|n| format!("after {n}\n")).collect();
    let hunks = line_diff(&before, &after).expect("under the limit");
    assert_eq!(hunks.len(), 1);
    assert_eq!(of_kind(&hunks[0], LineKind::Removed).len(), 4000);
    assert_eq!(of_kind(&hunks[0], LineKind::Added).len(), 4000);
    assert_eq!(hunks[0].before_start, 1);
    assert_eq!(hunks[0].after_start, 1);
    assert_eq!(replay(&before, &hunks), after.trim_end_matches('\n'));
}

#[test]
fn a_hunk_serialises_with_lowercase_kinds() {
    let hunks = line_diff("a\n", "b\n").expect("under the limit");
    let json = serde_json::to_string(&hunks).expect("hunks serialise");
    assert!(json.contains("\"kind\":\"removed\""), "{json}");
    assert!(json.contains("\"kind\":\"added\""), "{json}");
    let back: Vec<Hunk> = serde_json::from_str(&json).expect("hunks round-trip");
    assert_eq!(back, hunks);
}
