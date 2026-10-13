//! Resource hints in a previewed document are made inert before it is served.
//!
//! The oracle is html5ever's spec tokenizer started at every `<` in the output.
//! A browser recognises a start tag only where a `<` opens one, and tokenizes
//! the tag from there the same way whatever came before it; the contexts that
//! differ (comments, raw text, script, attribute values) only stop a `<` from
//! opening a tag. Tokenizing from every `<` is therefore a superset of the
//! `<link>` elements any engine could build from the text, in any context.
//!
//! A link the oracle finds is live when its `rel` names anything but
//! `stylesheet` or `alternate`, or when it repeats an attribute name. A spec
//! tokenizer keeps the first of two same-named attributes and drops the other,
//! but Chromium's speculative preload scanner reads the raw token and acts on a
//! later `rel`, so a repeated attribute can carry a hint the first one hides.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};

use dom_query::Document;
use html5ever::tendril::StrTendril;
use html5ever::tokenizer::{
    BufferQueue, Tag, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts,
};
use writ_core::preview::hints::neutralize_resource_hints;

/// Captures the first token, if it is a tag, and pauses the tokenizer there.
/// Parse errors reach the sink as tokens too; they are not part of the text, so
/// they never count as the first token.
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

/// Whether every rel keyword leaves a `<link>` able to reach nothing by itself:
/// a stylesheet (the CSP governs its fetch) or an alternate.
fn is_inert_rel(rel: &str) -> bool {
    rel.split(['\t', '\n', '\x0C', '\r', ' '])
        .filter(|token| !token.is_empty())
        .all(|token| {
            let token = token.to_ascii_lowercase();
            token == "stylesheet" || token == "alternate"
        })
}

/// Every `<link>` start tag a tokenizer could read anywhere in `html` that an
/// engine could act on: one whose `rel` carries a keyword other than
/// stylesheet/alternate, or one that repeats an attribute name.
fn live_hints(html: &str) -> Vec<String> {
    html.match_indices('<')
        .filter_map(|(at, _)| tag_at(html, at))
        .filter(|tag| tag.kind == TagKind::StartTag && &*tag.name == "link")
        .filter_map(|tag| {
            let rel = tag
                .attrs
                .iter()
                .find(|attr| &*attr.name.local == "rel")
                .map(|attr| attr.value.to_string());
            if tag.had_duplicate_attributes {
                return Some(format!("a repeated attribute, first rel {rel:?}"));
            }
            rel.filter(|rel| !is_inert_rel(rel))
        })
        .collect()
}

/// Markup around a link, chosen for how a parser treats what is inside it.
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
    ("<svg>", "</svg>"),
    ("<svg><foreignObject>", "</foreignObject></svg>"),
    ("<math><mi>", "</mi></math>"),
    ("<table><tr>", "</tr></table>"),
    ("<select>", "</select>"),
    ("<a title=\"", "\">x</a>"),
    ("<a title='", "'>x</a>"),
    ("<a title=", ">x</a>"),
    ("<![CDATA[", "]]>"),
    ("<xmp>", "</xmp>"),
    ("<iframe>", "</iframe>"),
    ("<noembed>", "</noembed>"),
    ("<plaintext>", ""),
];

/// Links whose rel asks the engine to connect, resolve or fetch, and links that
/// repeat an attribute, whose later copy an engine may read.
const HINTS: &[&str] = &[
    "<link rel=preconnect href=http://198.51.100.7/>",
    "<link rel=dns-prefetch href=//leak.example>",
    "<link rel=prefetch href=http://leak.example/p>",
    "<link rel=preload as=image href=http://leak.example/i>",
    "<link rel=modulepreload href=http://leak.example/m.js>",
    "<link rel=prerender href=http://leak.example/>",
    "<link rel=icon href=http://leak.example/i.ico>",
    "<link rel=manifest href=http://leak.example/m.json>",
    "<link rel=next href=http://leak.example/2>",
    "<LINK REL=PRECONNECT HREF=http://leak.example>",
    "<LiNk ReL=\"PreConnect\" href=\"http://leak.example\">",
    "<link rel=\"preconnect\" href=\"http://leak.example\">",
    "<link rel='preconnect' href='http://leak.example'>",
    "<link/rel=preconnect/href=http://leak.example>",
    "<link\trel=preconnect\thref=http://leak.example>",
    "<link\nrel=preconnect\nhref=http://leak.example>",
    "<link\x0Crel=preconnect\x0Chref=http://leak.example>",
    "<link\r\nrel=preconnect\r\nhref=http://leak.example>",
    "<link href=http://leak.example rel=preconnect>",
    "<link title=\"a>b\" rel=preconnect href=http://leak.example>",
    "<link title='a\"b' rel=preconnect href=http://leak.example>",
    "<link title=a\"b rel=preconnect href=http://leak.example>",
    "<link rel=\"&#112;reconnect\" href=http://leak.example>",
    "<link rel=\"pre&#x63;onnect\" href=http://leak.example>",
    "<link rel=\"&#x70;&#x72;&#x65;connect\" href=http://leak.example>",
    "<link rel=\"stylesheet preconnect\" href=http://leak.example>",
    "<link rel=\"  preconnect  \" href=http://leak.example>",
    "<link rel=preconnect rel=stylesheet href=http://leak.example>",
    "<link =x rel=preconnect href=http://leak.example>",
    "<link rel=preconnect href=http://leak.example/>",
    "<link rel = preconnect href=http://leak.example>",
    "<link rel=preconnect\0 href=http://leak.example>",
    "<link rel=\"preconnect\"/href=http://leak.example>",
    "<link rel=pre\nconnect href=http://leak.example>",
    "<link rel=stylesheet rel=preconnect href=http://leak.example>",
    "<link rel=alternate rel=preconnect href=http://leak.example>",
    "<link rel=stylesheet REL=\"PRE&#x43;ONNECT\" href=http://leak.example>",
    "<link rel=\"stylesheet\"rel=preconnect href=http://leak.example>",
    "<link rel=stylesheet/rel=preconnect href=http://leak.example>",
    "<link rel=stylesheet title=t rel=dns-prefetch href=//leak.example>",
    "<link rel=stylesheet rel=stylesheet rel=preconnect href=http://leak.example>",
    "<link rel href=x rel=preconnect>",
    "<link rel=\"\" rel=preconnect href=x>",
    "<link rel=writ-blocked rel=preconnect href=http://leak.example>",
    "<LINK rel=writ-blocked  REL=\"PRE&#x43;ONNECT\" HREF=http://leak.example>",
    "<link rel=stylesheet href=a.css href=http://leak.example>",
    "<link rel=stylesheet a\0=1 a\u{FFFD}=2 href=a.css>",
];

/// Links the neutralizer must leave byte for byte: one `rel` naming nothing
/// besides stylesheet/alternate, or no `rel` at all, and no repeated attribute.
const KEPT: &[&str] = &[
    "<link rel=stylesheet href=\"/_assets/preview/preview-base.css\">",
    "<link rel=\"stylesheet\" href=\"/_assets/katex/katex.min.css\">",
    "<link rel=\"alternate stylesheet\" title=t href=http://leak.example/a.css>",
    "<link rel=alternate type=application/rss+xml href=http://leak.example/f>",
    "<link REL=StyleSheet HREF=a.css>",
    "<link rel=\"stylesheet\" href=\"a.css\" media=\"print\" crossorigin integrity=\"sha384-x\">",
    "<link rel=stylesheet href=a.css title=\"rel=preconnect\">",
    "<link rel=stylesheet data-rel=preconnect href=a.css>",
    "<link rel=stylesheet relx=preconnect xrel=preconnect href=a.css>",
    "<link href=http://leak.example>",
    "<link rel href=x>",
    "<link>",
    "<link/>",
    "<linkx rel=preconnect href=x>",
    "<link-x rel=preconnect href=x>",
    "<a rel=preconnect href=x>a</a>",
    "<p>&lt;link rel=preconnect href=x&gt;</p>",
    "< link rel=preconnect href=x>",
];

/// Markup around a link for the tree comparison: the parser contexts above,
/// plus the insertion modes that place a `link` by a rule of their own.
const TREE_CONTEXTS: &[(&str, &str)] = &[
    ("<head><noscript>", "</noscript></head>"),
    ("<html><head></head>", "<body>x</body></html>"),
    ("<table>", "</table>"),
    ("<table><colgroup>", "</colgroup></table>"),
    ("<table><caption>", "</caption></table>"),
    ("<frameset>", "</frameset>"),
    ("<body><frameset>", "</frameset></body>"),
    ("<template><tr>", "</tr></template>"),
];

#[test]
fn every_hint_the_bank_holds_is_live_before_neutralizing() {
    for hint in HINTS {
        // The NUL and split-keyword variants are not hints to an engine (the
        // keyword is not `preconnect`), but their rel still carries an unknown
        // keyword, so the oracle counts them, which is what the neutralizer
        // has to clear.
        assert!(
            !live_hints(hint).is_empty(),
            "the oracle sees no live hint in {hint:?}, so the case proves nothing"
        );
    }
}

#[test]
fn no_link_with_a_hint_survives_in_any_context() {
    for (before, after) in CONTEXTS {
        for hint in HINTS {
            let html = format!("{before}{hint}{after}");
            let out = neutralize_resource_hints(&html);
            let live = live_hints(&out);
            assert!(
                live.is_empty(),
                "{live:?} survive in {html:?}\n  neutralized: {out:?}"
            );
        }
    }
}

#[test]
fn links_without_a_hint_are_left_byte_for_byte() {
    for (before, after) in CONTEXTS {
        for kept in KEPT {
            let html = format!("{before}{kept}{after}");
            let out = neutralize_resource_hints(&html);
            assert!(
                matches!(out, Cow::Borrowed(_)),
                "{html:?} was rewritten to {out:?}"
            );
        }
    }
}

#[test]
fn a_disabled_link_is_renamed_and_keeps_its_attributes() {
    let out = neutralize_resource_hints(
        "<head><link rel=preconnect href=http://leak.example></head><body>text</body>",
    );
    assert_eq!(
        out,
        "<head><basefont rel=preconnect href=http://leak.example></head><body>text</body>"
    );
}

#[test]
fn a_later_rel_cannot_bring_a_hint_back() {
    // The tree builder reads the first `rel` of a tag and Chromium's preload
    // scanner the last, so a kept first `rel` does not make a link safe.
    for html in [
        "<link rel=stylesheet rel=preconnect href=http://leak.example>",
        "<link rel=alternate rel=preconnect href=http://leak.example>",
        "<link rel=writ-blocked rel=preconnect href=http://leak.example>",
        "<link rel=stylesheet REL=\"PRE&#x43;ONNECT\" href=http://leak.example>",
    ] {
        let out = neutralize_resource_hints(html);
        assert_eq!(out, html.replacen("<link", "<basefont", 1), "{html:?}");
    }
}

#[test]
fn a_disabled_link_builds_the_same_tree_as_the_link() {
    // The tree builder places a `basefont` start tag by every rule it uses for
    // a `link`, so renaming one changes the element's name and nothing else.
    let links = [
        "<link rel=preconnect href=http://leak.example>",
        "<link rel=stylesheet rel=preconnect href=http://leak.example/>",
        "<link rel=icon href=http://leak.example/i.ico>",
    ];
    for (before, after) in CONTEXTS.iter().chain(TREE_CONTEXTS) {
        for link in links {
            let html = format!(
                "{before}{link}<p>after</p><title>t</title>\
                 <link rel=stylesheet href=a.css>{after}<p>end</p>"
            );
            let out = neutralize_resource_hints(&html);
            assert!(matches!(out, Cow::Owned(_)), "{html:?} was not rewritten");
            let original = Document::from(html.as_str()).html().to_string();
            let renamed = Document::from(&*out)
                .html()
                .to_string()
                .replace("basefont", "link");
            assert_eq!(renamed, original, "{html:?}\n  rewritten: {out:?}");
        }
    }
}

#[test]
fn a_document_with_many_hints_has_every_one_neutralized() {
    let html = HINTS.concat();
    assert!(live_hints(&neutralize_resource_hints(&html)).is_empty());
}

#[test]
fn hints_beside_kept_links_do_not_disturb_them() {
    let html = format!(
        "{}{}{}",
        KEPT[0], "<link rel=preconnect href=http://leak.example>", KEPT[1]
    );
    let out = neutralize_resource_hints(&html);
    assert!(out.starts_with(KEPT[0]), "{out}");
    assert!(out.ends_with(KEPT[1]), "{out}");
    assert!(live_hints(&out).is_empty(), "{out}");
}

#[test]
fn an_unterminated_attribute_cannot_hide_a_later_hint() {
    // A context-free reading that skipped from one candidate to the end of its
    // attributes would jump over the real link after the comment closes.
    let html = "<!-- <link title=\" --><link rel=preconnect href=http://leak.example>\">";
    let out = neutralize_resource_hints(html);
    assert!(live_hints(&out).is_empty(), "{out}");
}

#[test]
fn a_pathological_document_is_handled_in_linear_time() {
    // Every candidate opens an attribute that never closes, so each one would
    // read to the end of the text without a scan budget.
    let html = "<link title=\"".repeat(200_000);
    let started = std::time::Instant::now();
    let out = neutralize_resource_hints(&html);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "took {:?}",
        started.elapsed()
    );
    assert!(out.len() >= html.len());
}

#[test]
fn a_link_with_very_many_attributes_is_disabled_in_linear_time() {
    // Every attribute name is compared with the earlier ones to find a repeat;
    // names of one length make each comparison read the bytes.
    let names: String = (0..200_000).map(|i| format!(" a{i:06}=1")).collect();
    let html = format!("<link rel=stylesheet{names} href=a.css>").repeat(4);
    let started = std::time::Instant::now();
    let out = neutralize_resource_hints(&html);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "took {:?}",
        started.elapsed()
    );
    assert_eq!(out.matches("<basefont").count(), 4, "{}", &out[..80]);
}

#[test]
fn text_with_no_link_is_returned_without_a_copy() {
    for html in [
        "",
        "plain text",
        "<p>a < b</p>",
        "<li>nk</li>",
        "<lin",
        "<link",
    ] {
        assert!(
            matches!(neutralize_resource_hints(html), Cow::Borrowed(_)),
            "{html:?}"
        );
    }
}

#[test]
fn multibyte_text_around_a_hint_stays_intact() {
    let html = "مرحبا <link rel=preconnect href=http://leak.example> 日本語";
    let out = neutralize_resource_hints(html);
    assert!(out.starts_with("مرحبا "), "{out}");
    assert!(out.ends_with(" 日本語"), "{out}");
    assert!(live_hints(&out).is_empty(), "{out}");
}

#[test]
fn every_generated_link_tag_is_either_inert_or_neutralized() {
    // Attribute sequences built from the pieces whose tokenizer states differ:
    // case, value quoting, missing values, `=` as a name, separators that are
    // whitespace, `/`, or nothing at all, and closings that end, self-close or
    // never end the tag. Two attributes of one name are a repeat whichever
    // case or value each carries.
    let names = ["rel", "REL", "href", "=x"];
    let values = [
        "",
        "=preconnect",
        "=\"stylesheet\"",
        "='alternate'",
        "=stylesheet/x",
        "= preconnect",
        "=\"a>b\"",
        "=a\"b",
        "=\"&#112;reconnect\"",
    ];
    let separators = [" ", "/", "\x0C", "\r\n", ""];
    let closings = [">", "/>", ""];
    let attributes: Vec<String> = names
        .iter()
        .flat_map(|name| values.iter().map(move |value| format!("{name}{value}")))
        .collect();

    let mut cases = 0;
    for first in &attributes {
        for second in &attributes {
            for lead in &separators[..4] {
                for between in separators {
                    for closing in closings {
                        let html =
                            format!("<link{lead}{first}{between}{second}{closing}<p>after</p>");
                        let out = neutralize_resource_hints(&html);
                        if matches!(out, Cow::Borrowed(_)) {
                            assert!(
                                live_hints(&html).is_empty(),
                                "{html:?} was kept but a tokenizer reads a hint in it"
                            );
                        }
                        assert!(live_hints(&out).is_empty(), "{html:?} -> {out:?}");
                        cases += 1;
                    }
                }
            }
        }
    }
    assert!(cases > 10_000, "{cases} cases");
}
