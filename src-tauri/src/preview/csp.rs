//! Per-scope Content-Security-Policy for the preview webview.
//!
//! Lean scope: there is no trust matrix. The preview renders the user's own
//! offline agent output, so **network is categorically off** and one fixed
//! document policy applies to every document. The only variable is an
//! app-level scripts kill switch (`preview.run_scripts`) that flips
//! `script-src` between executing and `'none'`.
//!
//! Why scripts-on is safe here: the content is the user's own (often
//! sensitive prompt data), and with `connect-src 'none'` plus image/font/
//! media restricted to `data:` and the local host-gated `writ-preview:`
//! scheme, even a prompt-injected script has no egress channel. Scripts-on
//! makes interactive agent templates (sliders, checkboxes, Mermaid, KaTeX)
//! work; network-off makes them unable to leak.
//!
//! `writ-preview:` is local and host-gated: the protocol handler serves
//! only bundled chrome runtimes and the current buffer's rendered HTML, and
//! carries no network capability of its own.

/// Build the document-scope CSP. `scripts_enabled` is the app-level kill
/// switch; when false, `script-src` is `'none'` and everything else is
/// unchanged.
pub fn build_document_csp(scripts_enabled: bool) -> String {
    let script_src = if scripts_enabled {
        "script-src 'unsafe-inline' 'self' writ-preview:"
    } else {
        "script-src 'none'"
    };
    [
        "default-src 'none'",
        script_src,
        "style-src 'unsafe-inline' 'self' writ-preview:",
        "img-src data: writ-preview:",
        "font-src data: writ-preview:",
        "media-src data: writ-preview:",
        "connect-src 'none'",
        "object-src 'none'",
        "frame-src 'none'",
        "worker-src 'none'",
        "form-action 'none'",
        "base-uri 'none'",
        // 'self' (not 'none') so the main app webview can frame the
        // preview document over writ-preview://. Clickjacking — what
        // frame-ancestors guards — is a non-threat in a desktop app, and
        // 'self' still blocks any third-party embedder.
        "frame-ancestors 'self'",
        "navigate-to 'none'",
    ]
    .join("; ")
}

/// Build the chrome-scope CSP. The chrome scope serves the host's own
/// bundled assets (the fallback stylesheet now; Mermaid/KaTeX runtimes
/// later), so its scripts are `'self'` and it likewise has no network.
pub fn build_chrome_csp() -> String {
    [
        "default-src 'none'",
        "script-src 'self'",
        "style-src 'self' 'unsafe-inline'",
        "img-src 'self' data:",
        "font-src 'self' data:",
        "media-src 'self' data:",
        "connect-src 'none'",
        "object-src 'none'",
        "frame-src 'none'",
        "worker-src 'none'",
        "form-action 'none'",
        "base-uri 'none'",
        "frame-ancestors 'none'",
        "navigate-to 'none'",
    ]
    .join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    // The locked document CSP bytes (scripts on), per the lean-scope
    // decision. Any change here is a deliberate security decision and must
    // be made consciously — this assertion is the gate.
    const DOCUMENT_CSP_SCRIPTS_ON: &str = "default-src 'none'; \
script-src 'unsafe-inline' 'self' writ-preview:; \
style-src 'unsafe-inline' 'self' writ-preview:; \
img-src data: writ-preview:; \
font-src data: writ-preview:; \
media-src data: writ-preview:; \
connect-src 'none'; \
object-src 'none'; \
frame-src 'none'; \
worker-src 'none'; \
form-action 'none'; \
base-uri 'none'; \
frame-ancestors 'self'; \
navigate-to 'none'";

    const DOCUMENT_CSP_SCRIPTS_OFF: &str = "default-src 'none'; \
script-src 'none'; \
style-src 'unsafe-inline' 'self' writ-preview:; \
img-src data: writ-preview:; \
font-src data: writ-preview:; \
media-src data: writ-preview:; \
connect-src 'none'; \
object-src 'none'; \
frame-src 'none'; \
worker-src 'none'; \
form-action 'none'; \
base-uri 'none'; \
frame-ancestors 'self'; \
navigate-to 'none'";

    #[test]
    fn document_csp_scripts_on_matches_locked_bytes() {
        assert_eq!(build_document_csp(true), DOCUMENT_CSP_SCRIPTS_ON);
    }

    #[test]
    fn document_csp_scripts_off_matches_locked_bytes() {
        assert_eq!(build_document_csp(false), DOCUMENT_CSP_SCRIPTS_OFF);
    }

    #[test]
    fn document_csp_never_permits_network() {
        for enabled in [true, false] {
            let csp = build_document_csp(enabled);
            assert!(csp.contains("connect-src 'none'"), "connect-src must be none");
            // No remote scheme appears anywhere — only data: and the local
            // host-gated writ-preview:.
            assert!(!csp.contains("https:"));
            assert!(!csp.contains("http:"));
            assert!(!csp.contains("ws:"));
            assert!(!csp.contains("writ-workspace:"));
        }
    }

    #[test]
    fn scripts_off_blocks_all_script_execution() {
        let csp = build_document_csp(false);
        assert!(csp.contains("script-src 'none'"));
        // The kill switch only touches script-src; style-src keeps
        // 'unsafe-inline' 'self' writ-preview: so author styles still apply.
        assert!(!csp.contains("script-src 'unsafe-inline'"));
        assert!(csp.contains("style-src 'unsafe-inline' 'self' writ-preview:"));
    }

    #[test]
    fn chrome_csp_runs_self_scripts_without_network() {
        let csp = build_chrome_csp();
        assert!(csp.contains("script-src 'self'"));
        assert!(csp.contains("connect-src 'none'"));
        assert!(!csp.contains("'unsafe-eval'"));
    }
}
