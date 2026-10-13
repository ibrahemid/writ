//! Preview security verification suite (ADR-009 and ADR-011, amended by
//! ADR-043).
//!
//! What this suite proves, and what proves the rest:
//!
//! 1. **The policy bytes.** `preview::csp` locks the document and chrome
//!    policies for each platform, and `tests/app_csp_tests.rs` the app's
//!    `frame-src`. Whether an engine enforces them is answered by the engine
//!    probe (`tests/engine_probe.rs`), which fires every exfil vector at a
//!    real WebKit, WebKit2GTK or WebView2 behind local sinks (ADR-043). This
//!    suite asserts what the policy must always say.
//!
//! 2. **The document rewrite.** Resource hints are disabled and frame
//!    navigations disarmed before a document is served
//!    (`writ_core::preview::hints` and `writ_core::preview::frames`, tested
//!    against a spec tokenizer in that crate). Here, the documents the
//!    renderers write themselves are shown to pass through both unchanged.
//!
//! 3. **The scope/traversal boundary is what the recorder observes.** The
//!    handler genuinely receives `writ-preview://` URLs and decides
//!    chrome-vs-document routing and traversal refusal. Those dispositions
//!    are asserted via the debug-only request recorder against the real
//!    `resolve()` pipeline.

use std::borrow::Cow;

use writ_core::preview::frames::disarm_frame_navigations;
use writ_core::preview::hints::neutralize_resource_hints;
use writ_core::preview::{AssetScope, ContentRenderer, RenderRequest};
use writ_tauri_lib::preview::csp::build_document_csp;
use writ_tauri_lib::preview::embeds::IndexEmbeds;
use writ_tauri_lib::preview::handler::{chrome_asset, resolve, RenderedDoc};
use writ_tauri_lib::preview::protocol::{clear_records, drain_records, Disposition, RefusalReason};
use writ_tauri_lib::preview::renderers::{HtmlRenderer, MarkdownRenderer};

// --- 1. What the document policy must always say --------------------------

#[test]
fn the_document_policy_shuts_the_network_and_sandboxes_the_frame() {
    for scripts in [true, false] {
        let csp = build_document_csp(scripts);
        assert!(csp.contains("connect-src 'none'"), "{csp}");
        assert!(csp.contains("form-action 'none'"), "{csp}");
        assert!(csp.contains("frame-src 'none'"), "{csp}");
        assert!(csp.contains("object-src 'none'"), "{csp}");
        assert!(
            csp.contains("sandbox allow-scripts allow-same-origin"),
            "sandbox directive present: {csp}"
        );
        assert!(!csp.contains("allow-top-navigation"), "{csp}");
        assert!(!csp.contains("allow-popups"), "{csp}");
        assert!(!csp.contains("allow-forms"), "{csp}");
    }
}

#[test]
fn only_an_opted_in_document_may_run_inline_script() {
    let script_src = |scripts: bool| {
        build_document_csp(scripts)
            .split("; ")
            .find(|clause| clause.starts_with("script-src"))
            .expect("script-src")
            .to_string()
    };
    assert!(script_src(true).contains("'unsafe-inline'"));
    assert!(!script_src(false).contains("'unsafe-inline'"));
    // The first-party runtimes load either way.
    assert!(script_src(false).contains("'self'"));
}

// --- 2. The documents the renderers write survive the rewrite ------------

fn request(content_type: writ_core::preview::ContentTypeId, text: &str) -> RenderRequest {
    RenderRequest {
        content_type,
        buffer_text: text.to_string(),
        theme: Default::default(),
        zoom: 1.0,
        assets: None,
    }
}

/// A Markdown note with math (the KaTeX stylesheet link) and a Mermaid fence
/// (its runtime), and an HTML file with its own stylesheet link.
fn first_party_documents() -> [String; 2] {
    let markdown = MarkdownRenderer::without_index()
        .render(request(
            MarkdownRenderer::content_type_id(),
            "# Title\n\nInline $x^2$ and display:\n\n$$E=mc^2$$\n\n```mermaid\ngraph TD; A-->B\n```\n",
        ))
        .expect("markdown render")
        .document_html;
    let html = HtmlRenderer
        .render(request(
            HtmlRenderer::content_type_id(),
            "<link rel=\"stylesheet\" href=\"site.css\"><h1>x</h1>",
        ))
        .expect("html render")
        .document_html;
    [markdown, html]
}

#[test]
fn the_links_the_renderers_write_pass_the_hint_rewrite_unchanged() {
    for document in first_party_documents() {
        assert!(
            document.to_ascii_lowercase().contains("<link"),
            "the render carries no link to check: {document}"
        );
        assert!(
            matches!(neutralize_resource_hints(&document), Cow::Borrowed(_)),
            "a first-party link was rewritten: {document}"
        );
    }
}

#[test]
fn the_documents_the_renderers_write_pass_the_frame_rewrite_unchanged() {
    // The Markdown shell carries `<meta charset>`, a tag the rewrite reads.
    let documents = first_party_documents();
    assert!(
        documents[0].to_ascii_lowercase().contains("<meta"),
        "the Markdown render carries no meta tag to check: {}",
        documents[0]
    );
    for document in documents {
        assert!(
            matches!(disarm_frame_navigations(&document), Cow::Borrowed(_)),
            "a first-party document was rewritten: {document}"
        );
    }
}

// --- 2. Scope/traversal boundary: what the recorder observes -------------

fn no_doc(_: &str) -> Option<RenderedDoc> {
    None
}

/// Drive a URL through the real resolve() pipeline and return its recorded
/// disposition. The recorder is the debug-build hook the handler writes to.
fn disposition_of(url: &str) -> Disposition {
    clear_records();
    let resolved = resolve(url, true, no_doc, chrome_asset);
    // resolve() itself doesn't record (serve() does); mirror serve()'s
    // record call so the suite exercises the same disposition the handler
    // would log.
    use writ_tauri_lib::preview::protocol::{record, RequestRecord};
    record(RequestRecord {
        url: url.to_string(),
        disposition: resolved.disposition.clone(),
    });
    let mut records = drain_records();
    assert_eq!(
        records.len(),
        1,
        "exactly one disposition recorded for {url}"
    );
    records.remove(0).disposition
}

#[test]
fn document_cannot_traverse_into_chrome_scope() {
    for url in [
        "writ-preview://document/../chrome/preview-base.css",
        "writ-preview://document/buf-1/../../chrome/preview-base.css",
        "writ-preview://document/%2e%2e/chrome/preview-base.css",
        "writ-preview://document/..\\chrome\\preview-base.css",
    ] {
        assert_eq!(
            disposition_of(url),
            Disposition::Refused(RefusalReason::TraversalAttempt),
            "traversal `{url}` must be refused",
        );
    }
}

#[test]
fn null_bytes_and_bad_encoding_are_refused() {
    assert_eq!(
        disposition_of("writ-preview://document/foo%00bar"),
        Disposition::Refused(RefusalReason::ProhibitedCharacter),
    );
    assert_eq!(
        disposition_of("writ-preview://document/foo%2"),
        Disposition::Refused(RefusalReason::InvalidEncoding),
    );
}

#[test]
fn foreign_schemes_and_unknown_scopes_are_refused() {
    assert_eq!(
        disposition_of("https://attacker/"),
        Disposition::Refused(RefusalReason::WrongScheme),
    );
    assert_eq!(
        disposition_of("writ-preview://attacker/x"),
        Disposition::Refused(RefusalReason::UnknownScope),
    );
}

#[test]
fn legitimate_chrome_and_document_requests_are_allowed() {
    // The boundary refuses crossings, not ordinary same-scope requests.
    assert_eq!(
        disposition_of("writ-preview://chrome/preview-base.css"),
        Disposition::Allowed,
    );
    assert_eq!(
        disposition_of("writ-preview://document/buf-1"),
        Disposition::Allowed,
    );
}

// --- Frontmatter is hidden, so its contents never reach the document ------

#[test]
fn frontmatter_is_not_rendered_as_html() {
    use writ_core::preview::{ContentRenderer, RenderRequest};
    use writ_tauri_lib::preview::renderers::MarkdownRenderer;

    let out = MarkdownRenderer::without_index()
        .render(RenderRequest {
            content_type: MarkdownRenderer::content_type_id(),
            buffer_text: "---\ntitle: <script>alert('xss')</script>\n---\n\nordinary body\n"
                .to_string(),
            theme: Default::default(),
            zoom: 1.0,
            assets: None,
        })
        .expect("markdown render failed");

    assert!(!out.document_html.contains("alert('xss')"));
    assert!(!out.document_html.contains("<script>alert"));
    assert!(out.document_html.contains("ordinary body"));
}

/// A resolver that answers with whatever a caller wants written into the page.
///
/// Both halves of a wikilink come from a note on disk: the label is the text
/// the author typed, the href is a file name the notes folder holds. Neither
/// is trusted, and `wikilink_html` is the one path in this crate that puts raw
/// HTML into the preview document, so this drives it with what a hostile name
/// would carry.
struct FixedWikilinks {
    render: writ_render::WikilinkRender,
}

impl writ_render::WikilinkResolver for FixedWikilinks {
    fn resolve(&self, _inner: &str) -> writ_render::WikilinkRender {
        writ_render::WikilinkRender {
            href: self.render.href.clone(),
            label: self.render.label.clone(),
            resolved: self.render.resolved,
        }
    }
}

fn wikilink_html(label: &str, href: Option<&str>, resolved: bool) -> String {
    let resolver = FixedWikilinks {
        render: writ_render::WikilinkRender {
            href: href.map(str::to_string),
            label: label.to_string(),
            resolved,
        },
    };
    writ_render::render_markdown_fragment_with("[[Target]]\n", None, Some(&resolver), None).html
}

#[test]
fn a_wikilink_label_is_escaped_not_executed() {
    let out = wikilink_html("<script>alert('xss')</script>", Some("Note.md"), true);
    assert!(!out.contains("<script>"), "{out}");
    assert!(out.contains("&lt;script&gt;"), "{out}");

    let missing = wikilink_html("<img src=x onerror=alert(1)>", None, false);
    assert!(!missing.contains("<img"), "{missing}");
    assert!(missing.contains("&lt;img"), "{missing}");
}

#[test]
fn a_wikilink_href_cannot_break_out_of_its_attribute() {
    let out = wikilink_html("label", Some("a\" onmouseover=\"alert(1)"), true);
    assert!(!out.contains("onmouseover=\"alert"), "{out}");
    assert!(out.contains("&quot;"), "{out}");

    let angled = wikilink_html("label", Some("a><script>alert(1)</script>"), true);
    assert!(!angled.contains("<script>"), "{angled}");
}

/// An unresolved target is a span and never an anchor, so a name the index
/// does not hold cannot become a destination.
#[test]
fn only_a_resolved_wikilink_becomes_an_anchor() {
    let resolved = wikilink_html("label", Some("Note.md"), true);
    assert!(
        resolved.contains("<a class=\"writ-wikilink\""),
        "{resolved}"
    );

    for (href, is_resolved) in [(Some("javascript:alert(1)"), false), (None, true)] {
        let out = wikilink_html("label", href, is_resolved);
        assert!(!out.contains("<a "), "{out}");
        assert!(out.contains("writ-wikilink-missing"), "{out}");
    }
}

// --- 4. Note embeds: a target outside the notes folder names nothing ------

/// A notes folder holding one note, plus a file outside it and a symlink
/// inside it pointing at that file.
fn embed_resolver() -> (tempfile::TempDir, IndexEmbeds) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("notes");
    std::fs::create_dir_all(&root).expect("notes");
    std::fs::write(root.join("Note.md"), "# Note\n\nNote body.\n").expect("note");
    std::fs::write(dir.path().join("outside.md"), OUTSIDE).expect("outside");
    #[cfg(unix)]
    std::os::unix::fs::symlink(dir.path().join("outside.md"), root.join("Linked.md"))
        .expect("symlink");

    let db_path = dir.path().join("writ.db");
    let conn = writ_storage::database::connection::open_database(&db_path).expect("open");
    writ_storage::database::migrations::run_migrations(&conn).expect("migrations");
    drop(conn);

    let index = std::sync::Arc::new(
        writ_storage::notes_index::NotesIndexStore::open(&db_path).expect("index"),
    );
    index
        .reconcile(&root, &|| false, &|_| false)
        .expect("reconcile");
    let scope = AssetScope {
        notes_root: root.clone(),
        note_dir: root,
        buffer_id: "b1".to_string(),
        token: "t1".to_string(),
    };
    (dir, IndexEmbeds::new(index, scope, local_bytes))
}

/// The text of the file outside the notes folder. A render that ever holds it
/// read something an embed had no business reaching.
const OUTSIDE: &str = "outside secret body";

/// The dataless probe for a filesystem with nothing to report.
fn local_bytes(_: &std::path::Path) -> Option<u32> {
    None
}

/// An embed names a note through the index, which performs no path join and
/// no canonicalisation, so a target written as a path names nothing. A
/// symlink out of the notes folder is not indexed either (the walk does not
/// follow links), so it cannot be named. Both render the plain span every
/// unresolved target renders, and neither opens a file.
#[test]
fn a_note_embed_cannot_name_a_file_outside_the_notes_folder() {
    let (_dir, resolver) = embed_resolver();
    for target in [
        "../../etc/passwd",
        "/etc/passwd",
        "../outside",
        "../outside.md",
        "..%2f..%2fetc%2fpasswd",
        "notes/../../outside",
        "./../outside",
        "~/.ssh/id_rsa",
        "..\\..\\outside",
        "Linked",
    ] {
        let html = writ_render::render_markdown_fragment_with(
            &format!("![[{target}]]\n"),
            None,
            None,
            Some(&resolver as &dyn writ_render::NoteEmbedResolver),
        )
        .html;
        assert!(html.contains("writ-embed-missing"), "{target}: {html}");
        assert!(!html.contains("<section"), "{target}: {html}");
        assert!(!html.contains(OUTSIDE), "{target}: {html}");
    }

    // The note that is genuinely in the folder still renders, so the loop
    // above is refusing the target rather than the resolver being inert.
    let ok = writ_render::render_markdown_fragment_with(
        "![[Note]]\n",
        None,
        None,
        Some(&resolver as &dyn writ_render::NoteEmbedResolver),
    )
    .html;
    assert!(ok.contains("Note body."), "{ok}");
}
