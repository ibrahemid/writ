//! The fragment variant for text Writ did not author.

use writ_render::{render_markdown_fragment, render_markdown_fragment_untrusted};

#[test]
fn a_script_tag_does_not_survive() {
    let fragment =
        render_markdown_fragment_untrusted("Before\n\n<script>alert(1)</script>\n\nAfter");
    assert!(!fragment.html.contains("<script"), "{}", fragment.html);
    assert!(!fragment.html.contains("alert(1)"), "{}", fragment.html);
    assert!(fragment.html.contains("Before"));
    assert!(fragment.html.contains("After"));
}

#[test]
fn an_event_handler_attribute_does_not_survive() {
    let fragment =
        render_markdown_fragment_untrusted("<img src=x onerror=\"alert(1)\">\n\nstill here");
    assert!(!fragment.html.contains("onerror"), "{}", fragment.html);
    assert!(!fragment.html.contains("<img"), "{}", fragment.html);
    assert!(fragment.html.contains("still here"));
}

#[test]
fn an_inline_tag_goes_and_its_text_stays() {
    let fragment = render_markdown_fragment_untrusted("a <b>bold</b> word");
    assert!(!fragment.html.contains("<b>"), "{}", fragment.html);
    assert!(!fragment.html.contains("</b>"), "{}", fragment.html);
    assert!(fragment.html.contains("bold"), "{}", fragment.html);
    assert!(fragment.html.contains("a "), "{}", fragment.html);
    assert!(fragment.html.contains(" word"), "{}", fragment.html);
}

#[test]
fn an_iframe_and_an_object_do_not_survive() {
    let fragment = render_markdown_fragment_untrusted(
        "<iframe src=\"writ-preview://x\"></iframe>\n\n<object data=\"x\"></object>\n\ntext",
    );
    assert!(!fragment.html.contains("<iframe"), "{}", fragment.html);
    assert!(!fragment.html.contains("<object"), "{}", fragment.html);
    assert!(fragment.html.contains("text"));
}

#[test]
fn a_callout_title_cannot_smuggle_a_tag() {
    let fragment =
        render_markdown_fragment_untrusted("> [!note] <img src=x onerror=\"go()\">\n> body");
    assert!(!fragment.html.contains("onerror"), "{}", fragment.html);
    assert!(!fragment.html.contains("<img"), "{}", fragment.html);
    assert!(fragment.html.contains("body"));
}

#[test]
fn a_code_block_keeps_its_language_and_escapes_its_content() {
    let fragment = render_markdown_fragment_untrusted("```rust\nfn main() { \"<b>\" }\n```\n");
    assert!(
        fragment
            .html
            .contains("<pre><code class=\"language-rust\">"),
        "{}",
        fragment.html
    );
    assert!(fragment.html.contains("&lt;b&gt;"), "{}", fragment.html);
    assert!(!fragment.html.contains("<b>"), "{}", fragment.html);
}

#[test]
fn a_table_renders_as_a_table() {
    let fragment = render_markdown_fragment_untrusted("| a | b |\n| --- | --- |\n| 1 | 2 |\n");
    assert!(fragment.html.contains("<table>"), "{}", fragment.html);
    assert!(fragment.html.contains("<th>a</th>"), "{}", fragment.html);
    assert!(fragment.html.contains("<td>2</td>"), "{}", fragment.html);
}

#[test]
fn a_task_list_renders_its_checkboxes() {
    let fragment = render_markdown_fragment_untrusted("- [x] done\n- [ ] not done\n");
    assert!(fragment.html.contains("<input"), "{}", fragment.html);
    assert!(
        fragment.html.contains("type=\"checkbox\""),
        "{}",
        fragment.html
    );
    assert!(fragment.html.contains("disabled"), "{}", fragment.html);
}

#[test]
fn a_mermaid_fence_stays_a_code_block() {
    let fragment = render_markdown_fragment_untrusted("```mermaid\ngraph TD;\nA-->B;\n```\n");
    assert!(!fragment.has_mermaid);
    assert!(
        !fragment.html.contains("<pre class=\"mermaid\">"),
        "{}",
        fragment.html
    );
    assert!(
        fragment
            .html
            .contains("<pre><code class=\"language-mermaid\">"),
        "{}",
        fragment.html
    );
    assert!(fragment.html.contains("graph TD;"), "{}", fragment.html);
}

#[test]
fn math_stays_the_text_the_model_wrote() {
    let fragment = render_markdown_fragment_untrusted("inline $x^2$ and\n\n$$\ny = 1\n$$\n");
    assert!(!fragment.has_math);
    assert!(!fragment.html.contains("class=\"math"), "{}", fragment.html);
    assert!(fragment.html.contains("$x^2$"), "{}", fragment.html);
    assert!(fragment.html.contains("y = 1"), "{}", fragment.html);
}

#[test]
fn an_ordinary_reply_renders_as_markdown() {
    let fragment = render_markdown_fragment_untrusted(
        "# Heading\n\nA paragraph with **bold**, `code` and a [link](https://example.com).\n\n- one\n- two\n",
    );
    assert!(
        fragment.html.contains("<h1>Heading</h1>"),
        "{}",
        fragment.html
    );
    assert!(fragment.html.contains("<strong>bold</strong>"));
    assert!(fragment.html.contains("<code>code</code>"));
    assert!(fragment.html.contains("href=\"https://example.com\""));
    assert!(fragment.html.contains("<li>one</li>"));
}

/// A document exercising raw HTML, mermaid, math, a table and a callout, which
/// the note entry point must still render exactly as it did before the variant
/// was added.
const NOTE: &str = "# Title\n\n\
Text with <b>raw</b> markup and a <div class=\"box\">block</div>.\n\n\
```mermaid\ngraph TD;\nA-->B;\n```\n\n\
Math: $x^2$ and\n\n$$\ny = 1\n$$\n\n\
| a | b |\n| --- | --- |\n| 1 | 2 |\n\n\
> [!note] Titled\n> body\n";

#[test]
fn the_note_entry_point_is_untouched() {
    let fragment = render_markdown_fragment(NOTE);
    assert!(fragment.has_mermaid);
    assert!(fragment.has_math);
    assert!(fragment.html.contains("<b>raw</b>"), "{}", fragment.html);
    assert!(
        fragment.html.contains("<div class=\"box\">"),
        "{}",
        fragment.html
    );
    assert!(
        fragment.html.contains("<pre class=\"mermaid\">"),
        "{}",
        fragment.html
    );
    assert!(
        fragment.html.contains("class=\"math math-inline\""),
        "{}",
        fragment.html
    );
    assert!(
        fragment.html.contains("class=\"writ-callout\""),
        "{}",
        fragment.html
    );
}

#[test]
fn the_two_variants_agree_on_everything_but_the_untrusted_parts() {
    let untrusted = render_markdown_fragment_untrusted(NOTE);
    assert!(!untrusted.has_mermaid);
    assert!(!untrusted.has_math);
    assert!(!untrusted.html.contains("<b>raw</b>"));
    assert!(!untrusted.html.contains("<div class=\"box\">"));
    assert!(!untrusted.html.contains("<pre class=\"mermaid\">"));
    assert!(untrusted.html.contains("<h1>Title</h1>"));
    assert!(untrusted.html.contains("<table>"));
    assert!(untrusted.html.contains("<td>2</td>"));
    assert!(untrusted.html.contains("raw"));
    assert!(untrusted.html.contains("block"));
}

#[test]
fn a_link_the_pane_cannot_open_keeps_its_text_and_loses_its_anchor() {
    for (destination, gone) in [
        ("javascript:alert(1)", "javascript:"),
        ("data:text/html;base64,PHNjcmlwdD4=", "data:"),
        ("file:///etc/passwd", "file:"),
        ("../Launch.md", "../Launch.md"),
    ] {
        let fragment =
            render_markdown_fragment_untrusted(&format!("read [the note]({destination}) now"));
        assert!(!fragment.html.contains("<a "), "{}", fragment.html);
        assert!(!fragment.html.contains(gone), "{}", fragment.html);
        assert!(fragment.html.contains("the note"), "{}", fragment.html);
        assert!(fragment.html.contains("now"), "{}", fragment.html);
    }
}

#[test]
fn a_web_link_and_a_mailto_are_kept() {
    let fragment = render_markdown_fragment_untrusted(
        "[one](https://example.com/a) [two](http://example.com/b) \
         [three](mailto:someone@example.com) <https://example.com/c>",
    );
    assert!(
        fragment.html.contains("href=\"https://example.com/a\""),
        "{}",
        fragment.html
    );
    assert!(
        fragment.html.contains("href=\"http://example.com/b\""),
        "{}",
        fragment.html
    );
    assert!(
        fragment
            .html
            .contains("href=\"mailto:someone@example.com\""),
        "{}",
        fragment.html
    );
    assert!(
        fragment.html.contains("href=\"https://example.com/c\""),
        "{}",
        fragment.html
    );
}

#[test]
fn an_image_is_its_alt_text_and_fetches_nothing() {
    let fragment = render_markdown_fragment_untrusted(
        "before ![a beacon](https://evil.example/beacon.png) after\n\n\
         ![inline](data:image/svg+xml;base64,PHN2Zz4=)\n",
    );
    assert!(!fragment.html.contains("<img"), "{}", fragment.html);
    assert!(!fragment.html.contains("evil.example"), "{}", fragment.html);
    assert!(!fragment.html.contains("data:"), "{}", fragment.html);
    assert!(fragment.html.contains("a beacon"), "{}", fragment.html);
    assert!(fragment.html.contains("inline"), "{}", fragment.html);
    assert!(fragment.html.contains("before"), "{}", fragment.html);
    assert!(fragment.html.contains("after"), "{}", fragment.html);
}

#[test]
fn a_note_keeps_the_links_and_the_images_it_holds() {
    let fragment = render_markdown_fragment(
        "[one](javascript:alert(1)) [two](../Launch.md)\n\n![alt](https://example.com/a.png)\n",
    );
    assert!(
        fragment.html.contains("href=\"javascript:alert(1)\""),
        "{}",
        fragment.html
    );
    assert!(
        fragment.html.contains("href=\"../Launch.md\""),
        "{}",
        fragment.html
    );
    assert!(
        fragment
            .html
            .contains("<img src=\"https://example.com/a.png\""),
        "{}",
        fragment.html
    );
}
