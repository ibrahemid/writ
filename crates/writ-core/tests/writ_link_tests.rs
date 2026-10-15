//! The `writ://` grammar (ADR-052): what each verb takes, and every way out
//! of it that is refused.

use writ_core::writ_link::{open_url, parse, LinkRefusal, WritLink, MAX_LINK_BYTES};

fn open(path: &str, line: Option<u32>) -> Result<WritLink, LinkRefusal> {
    Ok(WritLink::Open {
        path: path.to_string(),
        line,
    })
}

fn new(name: Option<&str>) -> Result<WritLink, LinkRefusal> {
    Ok(WritLink::New {
        name: name.map(str::to_string),
    })
}

#[test]
fn the_two_verbs_read_as_what_they_ask_for() {
    let table: &[(&str, Result<WritLink, LinkRefusal>)] = &[
        ("writ://open?path=Plan.md", open("Plan.md", None)),
        (
            "writ://open?path=Plan.md&line=12",
            open("Plan.md", Some(12)),
        ),
        (
            "writ://open?line=12&path=Plan.md",
            open("Plan.md", Some(12)),
        ),
        ("writ://open/?path=Plan.md", open("Plan.md", None)),
        ("WRIT://open?path=Plan.md", open("Plan.md", None)),
        (
            "writ://open?path=Projects%2FLaunch%20Plan.md",
            open("Projects/Launch Plan.md", None),
        ),
        (
            "writ://open?path=Projects/Launch%20Plan.md",
            open("Projects/Launch Plan.md", None),
        ),
        (
            "writ://open?path=C++%20notes.md",
            open("C++ notes.md", None),
        ),
        ("writ://open?path=Caf%C3%A9.md", open("Café.md", None)),
        ("writ://open?path=Café.md", open("Café.md", None)),
        ("writ://open?path=..x/x..", open("..x/x..", None)),
        (
            "writ://open?path=a.md&line=4294967295",
            open("a.md", Some(u32::MAX)),
        ),
        ("writ://new", new(None)),
        ("writ://new/", new(None)),
        ("writ://new?", new(None)),
        ("writ://new?name=Plan", new(Some("Plan"))),
        (
            "writ://new?name=Launch%20plan.md",
            new(Some("Launch plan.md")),
        ),
        ("writ://new?name=%20%20Plan%20", new(Some("Plan"))),
    ];
    for (input, expected) in table {
        assert_eq!(&parse(input), expected, "{input}");
    }
}

#[test]
fn everything_outside_the_grammar_is_refused() {
    use LinkRefusal::*;
    let table: &[(&str, LinkRefusal)] = &[
        ("https://open?path=Plan.md", NotWrit),
        ("file:///etc/passwd", NotWrit),
        ("writ-preview://open?path=a", NotWrit),
        ("open?path=a", NotWrit),
        ("writ:open?path=a", Malformed),
        ("writ://open?path=a#frag", Malformed),
        ("writ://open?path", Malformed),
        ("writ://open?path=a&", Malformed),
        ("writ://open?path=a b", Unencoded),
        ("writ://open?path=a\nb", Unencoded),
        ("writ://open?path=a\u{0}b", Unencoded),
        (" writ://open?path=a", Unencoded),
        ("writ://save?path=a", UnknownVerb),
        ("writ://write?path=a", UnknownVerb),
        ("writ://delete?path=a", UnknownVerb),
        ("writ://OPEN?path=a", UnknownVerb),
        ("writ://open/x?path=a", UnknownVerb),
        ("writ://user@open?path=a", UnknownVerb),
        ("writ://open:80?path=a", UnknownVerb),
        ("writ://?path=a", UnknownVerb),
        ("writ://open?path=a&text=hello", UnknownParameter),
        ("writ://open?Path=a", UnknownParameter),
        ("writ://open?name=a", UnknownParameter),
        ("writ://new?path=a", UnknownParameter),
        ("writ://new?name=a&content=b", UnknownParameter),
        ("writ://open?path=a&path=b", RepeatedParameter),
        ("writ://open?path=a&line=1&line=2", RepeatedParameter),
        ("writ://new?name=a&name=b", RepeatedParameter),
        ("writ://open?path=%zz", BadEncoding),
        ("writ://open?path=a%", BadEncoding),
        ("writ://open?path=a%2", BadEncoding),
        ("writ://open?path=%FF", BadEncoding),
        ("writ://open", MissingPath),
        ("writ://open?line=3", MissingPath),
        ("writ://open?path=", MissingPath),
        ("writ://open?path=/etc/passwd", NotRelative),
        ("writ://open?path=%2Fetc%2Fpasswd", NotRelative),
        ("writ://open?path=%5C%5Cserver%5Cshare", NotRelative),
        ("writ://open?path=a%5Cb", NotRelative),
        ("writ://open?path=C:%5CWindows", NotRelative),
        ("writ://open?path=C:/Windows", NotRelative),
        ("writ://open?path=Plan.md:stream", NotRelative),
        ("writ://open?path=..", BadSegment),
        ("writ://open?path=../secret", BadSegment),
        ("writ://open?path=%2E%2E/secret", BadSegment),
        ("writ://open?path=a/../../secret", BadSegment),
        ("writ://open?path=a/./b", BadSegment),
        ("writ://open?path=a//b", BadSegment),
        ("writ://open?path=a/", BadSegment),
        ("writ://open?path=a%00b", BadSegment),
        ("writ://open?path=a%0Ab", BadSegment),
        ("writ://open?path=a&line=0", BadLine),
        ("writ://open?path=a&line=", BadLine),
        ("writ://open?path=a&line=-1", BadLine),
        ("writ://open?path=a&line=%2B1", BadLine),
        ("writ://open?path=a&line=1.5", BadLine),
        ("writ://open?path=a&line=4294967296", BadLine),
        ("writ://open?path=a&line=00000000001", BadLine),
        ("writ://new?name=", BadName),
        ("writ://new?name=%20", BadName),
        ("writ://new?name=a%2Fb", BadName),
        ("writ://new?name=a%5Cb", BadName),
        ("writ://new?name=..", BadName),
        ("writ://new?name=a:b", BadName),
        ("writ://new?name=a%00", BadName),
    ];
    for (input, refusal) in table {
        assert_eq!(parse(input), Err(*refusal), "{input}");
    }
}

#[test]
fn a_link_over_the_ceiling_is_refused_before_it_is_read() {
    let long = format!("writ://open?path={}", "a".repeat(MAX_LINK_BYTES));
    assert_eq!(parse(&long), Err(LinkRefusal::TooLong));

    let name = format!("writ://new?name={}", "a".repeat(256));
    assert_eq!(parse(&name), Err(LinkRefusal::BadName));
}

#[test]
fn a_refusal_never_repeats_the_link() {
    let input = "writ://open?path=../Secret%20Plans.md";
    let refusal = parse(input).expect_err("refused");
    let message = refusal.to_string();
    assert!(!message.contains("Secret"), "{message}");
    assert!(!message.contains(input), "{message}");
}

#[test]
fn a_built_link_escapes_what_the_query_would_read_as_its_own() {
    assert_eq!(
        open_url("Projects/Launch Plan.md", Some(12)).as_deref(),
        Some("writ://open?path=Projects/Launch%20Plan.md&line=12")
    );
    assert_eq!(
        open_url("a&line=9.md", None).as_deref(),
        Some("writ://open?path=a%26line%3D9.md")
    );
    assert_eq!(
        open_url("C++ #1 100%.md", None).as_deref(),
        Some("writ://open?path=C%2B%2B%20%231%20100%25.md")
    );
    assert_eq!(
        open_url("Plan.md", Some(0)).as_deref(),
        Some("writ://open?path=Plan.md"),
        "a line of 0 is no line"
    );
}

#[test]
fn no_link_is_built_for_a_path_the_grammar_refuses() {
    for path in [
        "",
        "/etc/passwd",
        "../secret",
        "a/../b",
        "a\\b",
        "C:/x",
        "a//b",
        "a/",
    ] {
        assert_eq!(open_url(path, None), None, "{path:?}");
    }
}

#[test]
fn only_arguments_spelled_as_writ_links_are_taken_from_a_launch() {
    use std::ffi::OsString;
    let args = [
        "--hidden",
        "/notes/Plan.md",
        "writ://open?path=Plan.md",
        "WRIT://new",
        "writ-preview://x",
        "writing.md",
    ]
    .map(OsString::from);
    assert_eq!(
        writ_core::writ_link::link_args(args),
        vec![
            "writ://open?path=Plan.md".to_string(),
            "WRIT://new".to_string()
        ]
    );
    assert!(!writ_core::writ_link::is_writ_link("wri"));
}
