//! Frame navigations a previewed document would start without a script are
//! disarmed before it is served.
//!
//! The oracle is html5ever's spec tokenizer started at every `<` in the output,
//! a superset of the tags any engine could build from the text in any context
//! (see `preview_resource_hints_tests.rs`). A frame is live when an `iframe` or
//! `frame` tag keeps a `src` or `srcdoc`, and a pragma is live when a `meta`
//! tag keeps an `http-equiv` whose value is not one of the inert pragmas. The
//! tokenizer keeps the first of two same-named attributes, and the rewrite
//! renames every copy it does not keep, so a copy left behind is the first one
//! the oracle sees.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};

use dom_query::Document;
use html5ever::tendril::StrTendril;
use html5ever::tokenizer::{
    BufferQueue, Tag, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts,
};
use writ_core::preview::frames::{disarm_frame_navigations, DISARMED_PREFIX};

/// Captures the first token, if it is a tag, and pauses the tokenizer there.
struct FirstTag {
    seen: Cell<bool>,
    tag: RefCell<Option<Tag>>,
}

impl TokenSink for FirstTag {
    type Handle = ();

    fn process_token(&self, token: Token, _line: u64) -> TokenSinkResult<()> {
        if matches!(token, Token::ParseError(_)) {
            return TokenSinkResult::Continue;
        }
        let first = !self.seen.replace(true);
        match token {
            Token::TagToken(tag) => {
                if first {
                    *self.tag.borrow_mut() = Some(tag);
                }
                TokenSinkResult::Script(())
            }
            _ => TokenSinkResult::Continue,
        }
    }
}

/// The tag a spec tokenizer reads when it starts at byte `at`, if one starts there.
fn tag_at(html: &str, at: usize) -> Option<Tag> {
    let tokenizer = Tokenizer::new(
        FirstTag {
            seen: Cell::new(false),
            tag: RefCell::new(None),
        },
        TokenizerOpts::default(),
    );
    let input = BufferQueue::default();
    input.push_back(StrTendril::from_slice(&html[at..]));
    let _ = tokenizer.feed(&input);
    tokenizer.sink.tag.take()
}

/// Pragmas that set no navigation and open no connection.
const INERT_PRAGMAS: [&str; 5] = [
    "content-type",
    "content-language",
    "default-style",
    "x-ua-compatible",
    "content-security-policy",
];

/// Whether a decoded `http-equiv` value is, as a whole and ASCII
/// case-insensitively, one of the inert pragmas.
fn is_inert_pragma(value: &str) -> bool {
    INERT_PRAGMAS
        .iter()
        .any(|pragma| value.eq_ignore_ascii_case(pragma))
}

/// Every frame navigation a tokenizer could read anywhere in `html`.
fn live_navigations(html: &str) -> Vec<String> {
    navigations(html, true)
}

/// [`live_navigations`] without the attributes that hold nothing, which start
/// no navigation: an empty `src` loads `about:blank`, an empty `srcdoc` an
/// empty document, and an empty `http-equiv` names no pragma. An end tag the
/// rewrite leaves alone reads as one of these from inside a tag.
fn loading_navigations(html: &str) -> Vec<String> {
    navigations(html, false)
}

fn navigations(html: &str, count_empty: bool) -> Vec<String> {
    html.match_indices('<')
        .filter_map(|(at, _)| tag_at(html, at))
        .filter(|tag| tag.kind == TagKind::StartTag)
        .filter_map(|tag| {
            let attribute = |name: &str| {
                tag.attrs
                    .iter()
                    .find(|attr| &*attr.name.local == name)
                    .map(|attr| attr.value.to_string())
                    .filter(|value| count_empty || !value.is_empty())
            };
            match &*tag.name {
                "iframe" | "frame" => attribute("src")
                    .map(|src| format!("{} src={src:?}", tag.name))
                    .or_else(|| {
                        attribute("srcdoc").map(|doc| format!("{} srcdoc={doc:?}", tag.name))
                    }),
                "meta" => attribute("http-equiv")
                    .filter(|value| !is_inert_pragma(value))
                    .map(|value| format!("meta http-equiv={value:?}")),
                _ => None,
            }
        })
        .collect()
}

/// Markup around a navigation, chosen for how a parser treats what is inside it.
const CONTEXTS: &[(&str, &str)] = &[
    ("", ""),
    (
        "<!doctype html><html><head>",
        "</head><body><p>x</p></body></html>",
    ),
    ("<body><p>", "</p></body>"),
    ("<!-- ", " -->"),
    ("<style>", "</style>"),
    ("<style><!--</style>", "-->"),
    ("<script>", "</script>"),
    ("<script>var s = \"", "\";</script>"),
    ("<textarea>", "</textarea>"),
    ("<title>", "</title>"),
    ("<noscript>", "</noscript>"),
    ("<noscript><!--</noscript>", "-->"),
    ("<template>", "</template>"),
    ("<div><template shadowrootmode=open>", "</template></div>"),
    ("<svg>", "</svg>"),
    ("<svg><foreignObject>", "</foreignObject></svg>"),
    ("<math><mi>", "</mi></math>"),
    ("<math><mtext>", "</mtext></math>"),
    ("<table><tr>", "</tr></table>"),
    ("<select>", "</select>"),
    ("<a title=\"", "\">x</a>"),
    ("<a title='", "'>x</a>"),
    ("<a title=", ">x</a>"),
    ("<![CDATA[", "]]>"),
    ("<xmp>", "</xmp>"),
    ("<iframe>", "</iframe>"),
    ("<noembed>", "</noembed>"),
    ("<frameset>", "</frameset>"),
    ("<plaintext>", ""),
];

/// Tags that start a frame navigation as the document is parsed, and pragmas
/// that are not known to be inert.
const NAVIGATIONS: &[&str] = &[
    "<iframe src=http://leak.example/></iframe>",
    "<iframe src=\"http://leak.example/\"></iframe>",
    "<iframe src='http://leak.example/'></iframe>",
    "<IFRAME SRC=http://leak.example/></IFRAME>",
    "<iFrAmE sRc=\"http://leak.example/\"></iFrAmE>",
    "<iframe/src=http://leak.example/></iframe>",
    "<iframe\tsrc=http://leak.example/></iframe>",
    "<iframe\nsrc=http://leak.example/></iframe>",
    "<iframe\x0Csrc=http://leak.example/></iframe>",
    "<iframe\r\nsrc=http://leak.example/></iframe>",
    "<iframe name=x src=http://leak.example/></iframe>",
    "<iframe title=\"a>b\" src=http://leak.example/></iframe>",
    "<iframe title='a\"b' src=http://leak.example/></iframe>",
    "<iframe title=a\"b src=http://leak.example/></iframe>",
    "<iframe title=\"x\"src=http://leak.example/></iframe>",
    "<iframe =x src=http://leak.example/></iframe>",
    "<iframe src = http://leak.example/></iframe>",
    "<iframe src=http://leak.example/ src=about:blank></iframe>",
    "<iframe src=about:blank SRC=http://leak.example/></iframe>",
    "<iframe data-writ-blocked-src=x src=http://leak.example/></iframe>",
    "<iframe srcdoc=\"&lt;link rel=preconnect href=http://leak.example/&gt;\"></iframe>",
    "<iframe srcdoc=\"&lt;meta http-equiv=refresh content='0;url=http://leak.example/'&gt;\"></iframe>",
    "<iframe SRCDOC='&lt;iframe src=http://leak.example/&gt;'></iframe>",
    "<iframe srcdoc></iframe>",
    "<iframe src></iframe>",
    "<frame src=http://leak.example/>",
    "<FRAME SRC=\"http://leak.example/\">",
    "<frame/src=http://leak.example/>",
    "<meta http-equiv=refresh content=\"0;url=http://leak.example/\">",
    "<meta http-equiv=\"refresh\" content=\"0;url=http://leak.example/\">",
    "<meta http-equiv='Refresh' content='0;url=http://leak.example/'>",
    "<META HTTP-EQUIV=REFRESH CONTENT=\"0;url=http://leak.example/\">",
    "<meta content=\"0;url=http://leak.example/\" http-equiv=refresh>",
    "<meta http-equiv=\"&#x72;efresh\" content=\"0;url=http://leak.example/\">",
    "<meta http-equiv=\"&#82;EFRESH\" content=\"0;url=http://leak.example/\">",
    "<meta http-equiv=\"r&#101;fresh\" content=\"0;url=http://leak.example/\">",
    "<meta http-equiv=\"  refresh  \" content=\"0;url=http://leak.example/\">",
    "<meta http-equiv=\"\x0Brefresh\" content=\"0;url=http://leak.example/\">",
    "<meta http-equiv=\"refresh\"/content=\"0;url=http://leak.example/\">",
    "<meta http-equiv=refresh http-equiv=content-type content=\"0;url=http://leak.example/\">",
    "<meta http-equiv = refresh content=\"0;url=http://leak.example/\">",
    "<meta/http-equiv=refresh content=\"0;url=http://leak.example/\">",
    "<meta http-equiv=\"refresh\"content=\"0;url=http://leak.example/\">",
    "<meta http-equiv=\"Link\" content=\"<http://leak.example/>; rel=preconnect\">",
    "<meta http-equiv=x-dns-prefetch-control content=on>",
    "<meta http-equiv=origin-trial content=token>",
    "<meta http-equiv=accept-ch content=sec-ch-ua-model>",
    "<meta http-equiv=set-cookie content=a=b>",
    "<meta http-equiv=\"\" content=\"0;url=http://leak.example/\">",
    "<meta http-equiv content=\"0;url=http://leak.example/\">",
    "<meta http-equiv=\" content-type\" content=\"text/html\">",
    "<meta http-equiv=ref content=\"0;url=http://leak.example/\">",
];

/// Markup the rewrite must leave byte for byte.
const KEPT: &[&str] = &[
    "<iframe></iframe>",
    "<iframe name=x title=\"src=http://leak.example/\"></iframe>",
    "<iframe data-src=http://leak.example/ srcx=y xsrc=z></iframe>",
    "<iframe sandbox allow=fullscreen width=10 height=10></iframe>",
    "<frame name=x>",
    "<frameset cols=\"50%,50%\"></frameset>",
    "<meta charset=\"utf-8\">",
    "<meta name=viewport content=\"width=device-width\">",
    "<meta http-equiv=\"Content-Type\" content=\"text/html; charset=utf-8\">",
    "<meta http-equiv=content-language content=ja>",
    "<meta http-equiv=\"Default-Style\" content=\"main\">",
    "<meta http-equiv=\"X-UA-Compatible\" content=\"IE=edge\">",
    "<meta http-equiv=\"Content-Security-Policy\" content=\"script-src 'none'\">",
    "<meta name=refresh content=\"0;url=http://leak.example/\">",
    "<meta content=refresh>",
    "<img src=http://leak.example/i.png>",
    "<script src=http://leak.example/s.js></script>",
    "<a href=http://leak.example/>x</a>",
    "<iframes src=http://leak.example/>",
    "<iframe-x src=http://leak.example/>",
    "<metadata http-equiv=refresh>",
    "<p>&lt;iframe src=http://leak.example/&gt;</p>",
    "< iframe src=http://leak.example/>",
];

/// Markup around a navigation for the tree comparison: the parser contexts
/// above, plus insertion modes with a rule of their own for these tags.
const TREE_CONTEXTS: &[(&str, &str)] = &[
    ("<head><noscript>", "</noscript></head>"),
    ("<html><head></head>", "<body>x</body></html>"),
    ("<table>", "</table>"),
    ("<table><colgroup>", "</colgroup></table>"),
    ("<table><caption>", "</caption></table>"),
    ("<body><frameset>", "</frameset></body>"),
    ("<template><tr>", "</tr></template>"),
    ("<p><b>", "</b></p>"),
];

#[test]
fn every_navigation_the_bank_holds_is_live_before_disarming() {
    for navigation in NAVIGATIONS {
        // The valueless `src` and `srcdoc` start no load, but they are the
        // attributes the rewrite clears, which is what the oracle counts.
        assert!(
            !live_navigations(navigation).is_empty(),
            "the oracle sees no navigation in {navigation:?}, so the case proves nothing"
        );
    }
}

#[test]
fn no_frame_navigation_survives_in_any_context() {
    for (before, after) in CONTEXTS {
        for navigation in NAVIGATIONS {
            let html = format!("{before}{navigation}{after}");
            let out = disarm_frame_navigations(&html);
            let live = live_navigations(&out);
            assert!(
                live.is_empty(),
                "{live:?} survive in {html:?}\n  disarmed: {out:?}"
            );
        }
    }
}

#[test]
fn markup_without_a_navigation_is_left_byte_for_byte() {
    for (before, after) in CONTEXTS {
        for kept in KEPT {
            let html = format!("{before}{kept}{after}");
            let out = disarm_frame_navigations(&html);
            assert!(
                matches!(out, Cow::Borrowed(_)),
                "{html:?} was rewritten to {out:?}"
            );
        }
    }
}

#[test]
fn a_disarmed_attribute_keeps_its_value_and_its_element() {
    for (html, expected) in [
        (
            "<iframe src=\"http://leak.example/\" title=t></iframe>",
            "<iframe data-writ-blocked-src=\"http://leak.example/\" title=t></iframe>",
        ),
        (
            "<IFRAME SRCDOC='<b>x</b>'></IFRAME>",
            "<IFRAME data-writ-blocked-SRCDOC='<b>x</b>'></IFRAME>",
        ),
        (
            "<frameset><frame src=a.html></frameset>",
            "<frameset><frame data-writ-blocked-src=a.html></frameset>",
        ),
        (
            "<meta http-equiv=refresh content=\"0;url=http://leak.example/\">",
            "<meta data-writ-blocked-http-equiv=refresh content=\"0;url=http://leak.example/\">",
        ),
    ] {
        assert_eq!(disarm_frame_navigations(html), expected, "{html:?}");
    }
}

#[test]
fn every_copy_of_a_repeated_attribute_is_disarmed() {
    // The tree builder keeps the first of two same-named attributes; another
    // reader could take the last, so neither may survive.
    for (html, expected) in [
        (
            "<iframe src=about:blank SRC=http://leak.example/></iframe>",
            "<iframe data-writ-blocked-src=about:blank data-writ-blocked-SRC=http://leak.example/></iframe>",
        ),
        (
            "<iframe srcdoc=a src=b srcdoc=c></iframe>",
            "<iframe data-writ-blocked-srcdoc=a data-writ-blocked-src=b data-writ-blocked-srcdoc=c></iframe>",
        ),
        (
            "<meta http-equiv=content-type http-equiv=refresh content=0>",
            "<meta http-equiv=content-type data-writ-blocked-http-equiv=refresh content=0>",
        ),
        (
            "<meta http-equiv=refresh http-equiv=content-type content=0>",
            "<meta data-writ-blocked-http-equiv=refresh http-equiv=content-type content=0>",
        ),
    ] {
        assert_eq!(disarm_frame_navigations(html), expected, "{html:?}");
    }
}

#[test]
fn every_pragma_but_the_inert_ones_is_disarmed() {
    // Kept only when the value is, as a whole and in plain text, an inert
    // pragma: no trimming, no character reference, nothing past the lookahead.
    let padding = " ".repeat(200);
    for value in [
        "refresh",
        "REFRESH",
        " refresh",
        "\trefresh\n",
        "\x0Brefresh",
        "refresh;",
        "&#x72;efresh",
        "r\u{0435}fresh",
        "\0refresh",
        "Link",
        "x-dns-prefetch-control",
        "origin-trial",
        "accept-ch",
        "delegate-ch",
        "set-cookie",
        "",
        " content-type",
        "content-type ",
        "&#x63;ontent-type",
        "content-typ\u{00E9}",
        "content-security-policy-report-only",
        padding.as_str(),
    ] {
        for html in [
            format!("<meta http-equiv=\"{value}\" content=\"0;url=x\">"),
            format!("<meta http-equiv='{value}' content=\"0;url=x\">"),
        ] {
            let out = disarm_frame_navigations(&html);
            assert!(
                out.contains("data-writ-blocked-http-equiv="),
                "{html:?} was kept: {out:?}"
            );
        }
    }
    for html in [
        "<meta http-equiv>",
        "<meta http-equiv/>",
        "<meta http-equiv content=x>",
        "<meta http-equiv=>",
        "<meta http-equiv=refresh>",
    ] {
        let out = disarm_frame_navigations(html);
        assert!(
            out.contains("data-writ-blocked-http-equiv"),
            "{html:?} was kept: {out:?}"
        );
    }
}

#[test]
fn the_inert_pragmas_are_kept() {
    for value in [
        "content-type",
        "Content-Type",
        "CONTENT-LANGUAGE",
        "default-style",
        "X-UA-Compatible",
        "content-security-policy",
    ] {
        for quoted in [
            format!("\"{value}\""),
            format!("'{value}'"),
            value.to_string(),
        ] {
            let html = format!("<meta http-equiv={quoted} content=\"text/html\">");
            assert!(
                matches!(disarm_frame_navigations(&html), Cow::Borrowed(_)),
                "{html:?}"
            );
        }
    }
}

#[test]
fn a_disarmed_document_builds_the_same_tree() {
    // Only attribute names change, so the tree with the prefix taken off the
    // rewritten names is the original tree.
    let navigations = [
        "<iframe src=http://leak.example/ title=t></iframe>",
        "<iframe srcdoc=\"&lt;b&gt;x&lt;/b&gt;\" src=a></iframe>",
        "<frame src=a.html>",
        "<meta http-equiv=refresh content=\"0;url=http://leak.example/\">",
        "<svg><meta http-equiv=refresh content=0></svg>",
    ];
    for (before, after) in CONTEXTS.iter().chain(TREE_CONTEXTS) {
        for navigation in navigations {
            let html = format!(
                "{before}{navigation}<p>after</p><title>t</title>\
                 <iframe name=kept></iframe>{after}<p>end</p>"
            );
            let out = disarm_frame_navigations(&html);
            assert!(matches!(out, Cow::Owned(_)), "{html:?} was not rewritten");
            let original = Document::from(html.as_str()).html().to_string();
            let disarmed = Document::from(&*out)
                .html()
                .to_string()
                .replace(DISARMED_PREFIX, "");
            assert_eq!(disarmed, original, "{html:?}\n  rewritten: {out:?}");
        }
    }
}

#[test]
fn an_unterminated_attribute_cannot_hide_a_later_navigation() {
    for html in [
        "<!-- <iframe title=\" --><iframe src=http://leak.example/>\">",
        "<!-- <meta title=' --><meta http-equiv=refresh content=0>'>",
        "<iframe title=\"<iframe src=http://leak.example/>\" src=http://leak.example/>",
        "<style><iframe title=\"</style><iframe src=http://leak.example/>\">",
    ] {
        let out = disarm_frame_navigations(html);
        assert!(live_navigations(&out).is_empty(), "{html:?} -> {out:?}");
    }
}

#[test]
fn overlapping_candidates_are_read_in_linear_time() {
    // Every candidate opens an attribute the next one sits inside, so reading
    // each tag on its own would read to the end of the text every time.
    let documents = [
        "<iframe title=\"".repeat(200_000),
        "<iframe title=\"<iframe title='".repeat(100_000) + "\" src=http://leak.example/>",
        "<meta http-equiv=".repeat(200_000) + "refresh>",
        "<frame/src=".repeat(200_000) + ">",
        "<iframe src=a ".repeat(200_000) + ">",
        "<iframe </src \n".repeat(200_000),
        format!("<iframe </src{}=x>", " ".repeat(1_000_000)),
    ];
    for html in &documents {
        let started = std::time::Instant::now();
        let out = disarm_frame_navigations(html);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "took {:?}",
            started.elapsed()
        );
        assert!(out.len() >= html.len());
    }
    let out = disarm_frame_navigations(&documents[1]);
    assert!(out.ends_with("\" data-writ-blocked-src=http://leak.example/>"));
}

#[test]
fn text_with_no_candidate_is_returned_without_a_copy() {
    for html in ["", "plain text", "<p>a < b</p>", "<ifr", "<iframe", "<meta"] {
        assert!(
            matches!(disarm_frame_navigations(html), Cow::Borrowed(_)),
            "{html:?}"
        );
    }
}

#[test]
fn multibyte_text_around_a_navigation_stays_intact() {
    let html = "مرحبا <iframe src=http://leak.example/></iframe> 日本語";
    let out = disarm_frame_navigations(html);
    assert!(out.starts_with("مرحبا "), "{out}");
    assert!(out.ends_with(" 日本語"), "{out}");
    assert!(live_navigations(&out).is_empty(), "{out}");
}

#[test]
fn every_generated_tag_is_either_inert_or_disarmed() {
    // Attribute sequences built from the pieces whose tokenizer states differ:
    // case, value quoting, missing values, `=` as a name, separators that are
    // whitespace, `/`, or nothing at all, and closings that end, self-close or
    // never end the tag.
    let names = ["src", "SRCDOC", "http-equiv", "title", "=x"];
    let values = [
        "",
        "=refresh",
        "=content-type",
        "=\"http://leak.example/\"",
        "='Refresh'",
        "=a/b",
        "= refresh",
        "=\"a>b\"",
        "=a\"b",
        "=\"&#x72;efresh\"",
    ];
    let separators = [" ", "/", "\x0C", "\r\n", ""];
    let closings = [">", "/>", ""];
    let attributes: Vec<String> = names
        .iter()
        .flat_map(|name| values.iter().map(move |value| format!("{name}{value}")))
        .collect();

    let mut cases = 0;
    for tag in ["iframe", "FRAME", "meta"] {
        for first in &attributes {
            for second in &attributes {
                for lead in &separators[..4] {
                    for between in separators {
                        for closing in closings {
                            let html = format!(
                                "<{tag}{lead}{first}{between}{second}{closing}<p>after</p>"
                            );
                            let out = disarm_frame_navigations(&html);
                            if matches!(out, Cow::Borrowed(_)) {
                                assert!(
                                    live_navigations(&html).is_empty(),
                                    "{html:?} was kept but a tokenizer reads a navigation in it"
                                );
                            }
                            assert!(live_navigations(&out).is_empty(), "{html:?} -> {out:?}");
                            cases += 1;
                        }
                    }
                }
            }
        }
    }
    assert!(cases > 100_000, "{cases} cases");
}

/// Markup that leaves an `iframe`, `frame` or `meta` open where the parser
/// reads text, so the walk from that tag reads what follows as attributes.
const STALE_WALKS: &[&str] = &[
    "<textarea><iframe x=\"</textarea>\" ",
    "<textarea><frame x=\"</textarea>\" ",
    "<textarea><meta x=\"</textarea>\" ",
    "<title><iframe x='</title>' ",
    "<!-- <iframe x=\"-->\" ",
    "<style><iframe x='</style>' ",
];

/// The tree `html` builds, serialized, with every inserted prefix taken out.
fn tree_without_prefix(html: &str) -> String {
    Document::from(html)
        .html()
        .to_string()
        .replace(DISARMED_PREFIX, "")
}

#[test]
fn an_end_tag_keeps_its_name() {
    // Read from the stale tag, `</src>` is an attribute `src` with no value,
    // which loads nothing. Read by the parser it closes `<src>`, and renaming
    // it would move everything after it inside that element.
    let repro = "<src><textarea><iframe x=\"</textarea>\" </src>after";
    assert_eq!(disarm_frame_navigations(repro), repro);
    let rewritten_elsewhere = format!("{repro}<iframe src=http://leak.example/></iframe>");
    let out = disarm_frame_navigations(&rewritten_elsewhere);
    assert!(matches!(out, Cow::Owned(_)), "{out:?}");
    assert_eq!(
        tree_without_prefix(&out),
        tree_without_prefix(&rewritten_elsewhere)
    );

    let end_tags = [
        ("src", "</src>"),
        ("src", "</SRC>"),
        ("src", "</src >"),
        ("src", "</src/>"),
        ("src", "</src\n>"),
        ("src", "</src x=y>"),
        ("srcdoc", "</srcdoc>"),
        ("http-equiv", "</http-equiv>"),
        ("http-equiv", "</HTTP-EQUIV >"),
    ];
    for stale in STALE_WALKS {
        for (element, end_tag) in end_tags {
            let html = format!("<{element}>{stale}{end_tag}after<p>end</p>");
            let out = disarm_frame_navigations(&html);
            assert!(
                matches!(out, Cow::Borrowed(_)),
                "{html:?} was rewritten to {out:?}"
            );
            assert!(loading_navigations(&out).is_empty(), "{html:?}");
        }
    }
}

#[test]
fn an_end_tag_name_an_equals_sign_follows_is_renamed() {
    // Read from the stale tag, the `=` gives the attribute a value, so the
    // name is renamed even though it also closes `<src>`. No end tag a document
    // writes carries an `=`.
    for (html, expected) in [
        (
            "<src><textarea><iframe x=\"</textarea>\" </src =x>after",
            "<src><textarea><iframe x=\"</textarea>\" </data-writ-blocked-src =x>after",
        ),
        (
            "<srcdoc><textarea><iframe x=\"</textarea>\" </srcdoc\n= \"&lt;b&gt;\">after",
            "<srcdoc><textarea><iframe x=\"</textarea>\" </data-writ-blocked-srcdoc\n= \"&lt;b&gt;\">after",
        ),
        (
            "<http-equiv><textarea><meta x=\"</textarea>\" </http-equiv=refresh>after",
            "<http-equiv><textarea><meta x=\"</textarea>\" </data-writ-blocked-http-equiv=refresh>after",
        ),
    ] {
        let out = disarm_frame_navigations(html);
        assert_eq!(out, expected, "{html:?}");
        assert!(live_navigations(&out).is_empty(), "{html:?} -> {out:?}");
    }
}

#[test]
fn a_name_after_an_end_tag_opener_that_carries_a_value_is_disarmed() {
    let navigations = [
        "<iframe </src=http://leak.example/></iframe>",
        "<iframe </src =http://leak.example/></iframe>",
        "<iframe </SRC\n= \"http://leak.example/\"></iframe>",
        "<frame </src = 'http://leak.example/'>",
        "<iframe </srcdoc =\"&lt;link rel=preconnect href=http://leak.example/&gt;\"></iframe>",
        "<meta </http-equiv =refresh content=\"0;url=http://leak.example/\">",
        "<meta </http-equiv=refresh content=\"0;url=http://leak.example/\">",
    ];
    for navigation in navigations {
        assert!(
            !loading_navigations(navigation).is_empty(),
            "the oracle sees no navigation in {navigation:?}, so the case proves nothing"
        );
    }
    for (before, after) in CONTEXTS {
        for navigation in navigations {
            let html = format!("{before}{navigation}{after}");
            let out = disarm_frame_navigations(&html);
            let live = loading_navigations(&out);
            assert!(
                live.is_empty(),
                "{live:?} survive in {html:?}\n  disarmed: {out:?}"
            );
        }
    }
    for (html, expected) in [
        (
            "<iframe </src =http://leak.example/>",
            "<iframe </data-writ-blocked-src =http://leak.example/>",
        ),
        (
            "<iframe </src=http://leak.example/>",
            "<iframe </data-writ-blocked-src=http://leak.example/>",
        ),
        ("<iframe </src></iframe>", "<iframe </src></iframe>"),
    ] {
        assert_eq!(disarm_frame_navigations(html), expected, "{html:?}");
    }
}
