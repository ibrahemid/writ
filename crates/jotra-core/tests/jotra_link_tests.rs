//! The `jotra://` grammar (ADR-052): what each verb takes, and every way out
//! of it that is refused.

use jotra_core::jotra_link::{open_url, parse, JotraLink, LinkRefusal, MAX_LINK_BYTES};

fn open(path: &str, line: Option<u32>) -> Result<JotraLink, LinkRefusal> {
    Ok(JotraLink::Open {
        path: path.to_string(),
        line,
    })
}

fn new(name: Option<&str>) -> Result<JotraLink, LinkRefusal> {
    Ok(JotraLink::New {
        name: name.map(str::to_string),
    })
}

#[test]
fn the_two_verbs_read_as_what_they_ask_for() {
    let table: &[(&str, Result<JotraLink, LinkRefusal>)] = &[
        ("jotra://open?path=Plan.md", open("Plan.md", None)),
        (
            "jotra://open?path=Plan.md&line=12",
            open("Plan.md", Some(12)),
        ),
        (
            "jotra://open?line=12&path=Plan.md",
            open("Plan.md", Some(12)),
        ),
        ("jotra://open/?path=Plan.md", open("Plan.md", None)),
        ("JOTRA://open?path=Plan.md", open("Plan.md", None)),
        (
            "jotra://open?path=Projects%2FLaunch%20Plan.md",
            open("Projects/Launch Plan.md", None),
        ),
        (
            "jotra://open?path=Projects/Launch%20Plan.md",
            open("Projects/Launch Plan.md", None),
        ),
        (
            "jotra://open?path=C++%20notes.md",
            open("C++ notes.md", None),
        ),
        ("jotra://open?path=Caf%C3%A9.md", open("Café.md", None)),
        ("jotra://open?path=Café.md", open("Café.md", None)),
        ("jotra://open?path=..x/x..", open("..x/x..", None)),
        (
            "jotra://open?path=a.md&line=4294967295",
            open("a.md", Some(u32::MAX)),
        ),
        ("jotra://new", new(None)),
        ("jotra://new/", new(None)),
        ("jotra://new?", new(None)),
        ("jotra://new?name=Plan", new(Some("Plan"))),
        (
            "jotra://new?name=Launch%20plan.md",
            new(Some("Launch plan.md")),
        ),
        ("jotra://new?name=%20%20Plan%20", new(Some("Plan"))),
    ];
    for (input, expected) in table {
        assert_eq!(&parse(input), expected, "{input}");
    }
}

#[test]
fn everything_outside_the_grammar_is_refused() {
    use LinkRefusal::*;
    let table: &[(&str, LinkRefusal)] = &[
        ("https://open?path=Plan.md", NotJotra),
        ("file:///etc/passwd", NotJotra),
        ("jotra-preview://open?path=a", NotJotra),
        ("open?path=a", NotJotra),
        ("jotra:open?path=a", Malformed),
        ("jotra://open?path=a#frag", Malformed),
        ("jotra://open?path", Malformed),
        ("jotra://open?path=a&", Malformed),
        ("jotra://open?path=a b", Unencoded),
        ("jotra://open?path=a\nb", Unencoded),
        ("jotra://open?path=a\u{0}b", Unencoded),
        (" jotra://open?path=a", Unencoded),
        ("jotra://save?path=a", UnknownVerb),
        ("jotra://write?path=a", UnknownVerb),
        ("jotra://delete?path=a", UnknownVerb),
        ("jotra://OPEN?path=a", UnknownVerb),
        ("jotra://open/x?path=a", UnknownVerb),
        ("jotra://user@open?path=a", UnknownVerb),
        ("jotra://open:80?path=a", UnknownVerb),
        ("jotra://?path=a", UnknownVerb),
        ("jotra://open?path=a&text=hello", UnknownParameter),
        ("jotra://open?Path=a", UnknownParameter),
        ("jotra://open?name=a", UnknownParameter),
        ("jotra://new?path=a", UnknownParameter),
        ("jotra://new?name=a&content=b", UnknownParameter),
        ("jotra://open?path=a&path=b", RepeatedParameter),
        ("jotra://open?path=a&line=1&line=2", RepeatedParameter),
        ("jotra://new?name=a&name=b", RepeatedParameter),
        ("jotra://open?path=%zz", BadEncoding),
        ("jotra://open?path=a%", BadEncoding),
        ("jotra://open?path=a%2", BadEncoding),
        ("jotra://open?path=%FF", BadEncoding),
        ("jotra://open", MissingPath),
        ("jotra://open?line=3", MissingPath),
        ("jotra://open?path=", MissingPath),
        ("jotra://open?path=/etc/passwd", NotRelative),
        ("jotra://open?path=%2Fetc%2Fpasswd", NotRelative),
        ("jotra://open?path=%5C%5Cserver%5Cshare", NotRelative),
        ("jotra://open?path=a%5Cb", NotRelative),
        ("jotra://open?path=C:%5CWindows", NotRelative),
        ("jotra://open?path=C:/Windows", NotRelative),
        ("jotra://open?path=Plan.md:stream", NotRelative),
        ("jotra://open?path=..", BadSegment),
        ("jotra://open?path=../secret", BadSegment),
        ("jotra://open?path=%2E%2E/secret", BadSegment),
        ("jotra://open?path=a/../../secret", BadSegment),
        ("jotra://open?path=a/./b", BadSegment),
        ("jotra://open?path=a//b", BadSegment),
        ("jotra://open?path=a/", BadSegment),
        ("jotra://open?path=a%00b", BadSegment),
        ("jotra://open?path=a%0Ab", BadSegment),
        ("jotra://open?path=a&line=0", BadLine),
        ("jotra://open?path=a&line=", BadLine),
        ("jotra://open?path=a&line=-1", BadLine),
        ("jotra://open?path=a&line=%2B1", BadLine),
        ("jotra://open?path=a&line=1.5", BadLine),
        ("jotra://open?path=a&line=4294967296", BadLine),
        ("jotra://open?path=a&line=00000000001", BadLine),
        ("jotra://new?name=", BadName),
        ("jotra://new?name=%20", BadName),
        ("jotra://new?name=a%2Fb", BadName),
        ("jotra://new?name=a%5Cb", BadName),
        ("jotra://new?name=..", BadName),
        ("jotra://new?name=a:b", BadName),
        ("jotra://new?name=a%00", BadName),
    ];
    for (input, refusal) in table {
        assert_eq!(parse(input), Err(*refusal), "{input}");
    }
}

#[test]
fn a_link_over_the_ceiling_is_refused_before_it_is_read() {
    let long = format!("jotra://open?path={}", "a".repeat(MAX_LINK_BYTES));
    assert_eq!(parse(&long), Err(LinkRefusal::TooLong));

    let name = format!("jotra://new?name={}", "a".repeat(256));
    assert_eq!(parse(&name), Err(LinkRefusal::BadName));
}

#[test]
fn a_refusal_never_repeats_the_link() {
    let input = "jotra://open?path=../Secret%20Plans.md";
    let refusal = parse(input).expect_err("refused");
    let message = refusal.to_string();
    assert!(!message.contains("Secret"), "{message}");
    assert!(!message.contains(input), "{message}");
}

#[test]
fn a_built_link_escapes_what_the_query_would_read_as_its_own() {
    assert_eq!(
        open_url("Projects/Launch Plan.md", Some(12)).as_deref(),
        Some("jotra://open?path=Projects/Launch%20Plan.md&line=12")
    );
    assert_eq!(
        open_url("a&line=9.md", None).as_deref(),
        Some("jotra://open?path=a%26line%3D9.md")
    );
    assert_eq!(
        open_url("C++ #1 100%.md", None).as_deref(),
        Some("jotra://open?path=C%2B%2B%20%231%20100%25.md")
    );
    assert_eq!(
        open_url("Plan.md", Some(0)).as_deref(),
        Some("jotra://open?path=Plan.md"),
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
fn only_arguments_spelled_as_jotra_links_are_taken_from_a_launch() {
    use std::ffi::OsString;
    let args = [
        "--hidden",
        "/notes/Plan.md",
        "jotra://open?path=Plan.md",
        "JOTRA://new",
        "jotra-preview://x",
        "writing.md",
    ]
    .map(OsString::from);
    assert_eq!(
        jotra_core::jotra_link::link_args(args),
        vec![
            "jotra://open?path=Plan.md".to_string(),
            "JOTRA://new".to_string()
        ]
    );
    assert!(!jotra_core::jotra_link::is_jotra_link("wri"));
}
