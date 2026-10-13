//! Per-scope Content-Security-Policy for the preview webview.
//!
//! There is no trust matrix: one fixed document policy applies to every
//! document. The one variable is whether the document's own scripts may run,
//! off for every document and opted in per file (decision RC-D1/TS-D1), which
//! flips `script-src` between permitting author inline scripts and permitting
//! only the first-party runtimes.
//!
//! The policy shuts every fetch-shaped channel: `connect-src 'none'`, and
//! images, fonts and media only from `data:` and the document's own origin.
//! The iframe `sandbox` (set in the frontend) and this policy's `sandbox`
//! directive block top navigation, popups and forms; the app webview's
//! navigation guard and its `frame-src` refuse the frame's own navigation. What
//! the policy cannot govern (resource hints, the connection Chromium opens to a
//! frame navigation's target before refusing it, `RTCPeerConnection`) is closed
//! by the document rewrites or accepted in ADR-043.
//!
//! The document's own origin is `writ-preview://document` on macOS and Linux.
//! On Windows WebView2 serves the same scheme as `http://writ-preview.<scope>`,
//! so the Windows policy names that origin; the macOS and Linux policies do
//! not, because there it is an ordinary HTTP host a resolver would look up.

/// The preview document's own origin under the Windows WebView2 custom-scheme
/// workaround, where `writ-preview://document/…` is served as
/// `http://writ-preview.document/…`.
const WINDOWS_DOCUMENT_ORIGIN: &str = "http://writ-preview.document";

/// The Vite dev server, which is the app origin under `cargo tauri dev` on
/// every platform.
const DEV_SERVER_ORIGIN: &str = "http://localhost:1420";

/// The document policy for this build's platform. `scripts_enabled` is the
/// per-file opt-in (off by default); when off, `script-src` admits only the
/// first-party runtimes, not the document's own inline scripts.
pub fn build_document_csp(scripts_enabled: bool) -> String {
    document_csp_for(scripts_enabled, cfg!(target_os = "windows"))
}

/// The chrome policy for this build's platform.
pub fn build_chrome_csp() -> String {
    chrome_csp_for(cfg!(target_os = "windows"))
}

/// The local sources a document subresource may come from on one platform.
fn document_sources(windows: bool) -> String {
    if windows {
        format!("'self' writ-preview: {WINDOWS_DOCUMENT_ORIGIN}")
    } else {
        "'self' writ-preview:".to_string()
    }
}

/// `frame-ancestors` naming the app origins that embed the preview on one
/// platform. `'self'` would refuse the app on Windows, where the app is
/// `http://tauri.localhost` and the frame `http://writ-preview.document`, a
/// cross-origin embed Chromium enforces.
fn app_frame_ancestors(windows: bool) -> String {
    if windows {
        format!(
            "frame-ancestors http://tauri.localhost https://tauri.localhost {DEV_SERVER_ORIGIN}"
        )
    } else {
        format!("frame-ancestors tauri://localhost {DEV_SERVER_ORIGIN}")
    }
}

/// The document policy for one platform, split out so both platforms' bytes
/// are tested on every host.
pub fn document_csp_for(scripts_enabled: bool, windows: bool) -> String {
    let sources = document_sources(windows);
    let script_src = if scripts_enabled {
        format!("script-src 'unsafe-inline' {sources}")
    } else {
        // Author inline scripts cannot run, but the first-party runtimes the
        // renderers inject (the bridge, Mermaid, KaTeX) are served same-origin
        // under `_assets/` and still load, so diagrams and math draw whether
        // or not the file is opted in.
        format!("script-src {sources}")
    };
    [
        "default-src 'none'".to_string(),
        script_src,
        format!("style-src 'unsafe-inline' {sources}"),
        format!("img-src {sources} data:"),
        format!("font-src {sources} data:"),
        format!("media-src {sources} data:"),
        "connect-src 'none'".to_string(),
        "object-src 'none'".to_string(),
        "frame-src 'none'".to_string(),
        "worker-src 'none'".to_string(),
        "form-action 'none'".to_string(),
        "base-uri 'none'".to_string(),
        app_frame_ancestors(windows),
        // No shipping engine implements it, so it is not load-bearing.
        "navigate-to 'none'".to_string(),
        // `allow-same-origin` keeps the document's own origin, which `'self'`,
        // the bridge's postMessage source check and the bundled fonts need.
        "sandbox allow-scripts allow-same-origin".to_string(),
    ]
    .join("; ")
}

/// The chrome policy for one platform. The chrome scope serves the host's own
/// bundled assets (the fallback stylesheet and the parked blank document), so
/// its scripts are `'self'` and it likewise has no network.
pub fn chrome_csp_for(windows: bool) -> String {
    [
        "default-src 'none'".to_string(),
        "script-src 'self'".to_string(),
        "style-src 'self' 'unsafe-inline'".to_string(),
        "img-src 'self' data:".to_string(),
        "font-src 'self' data:".to_string(),
        "media-src 'self' data:".to_string(),
        "connect-src 'none'".to_string(),
        "object-src 'none'".to_string(),
        "frame-src 'none'".to_string(),
        "worker-src 'none'".to_string(),
        "form-action 'none'".to_string(),
        "base-uri 'none'".to_string(),
        // The blank chrome document is parked in the preview iframe, so the app
        // webview must be allowed to frame it, the same as the document scope.
        app_frame_ancestors(windows),
        "navigate-to 'none'".to_string(),
    ]
    .join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;

    // The locked document policies. Any change here is a security decision;
    // these assertions are the gate that makes it a conscious one.
    const DOCUMENT_CSP_SCRIPTS_ON: &str = "default-src 'none'; \
script-src 'unsafe-inline' 'self' writ-preview:; \
style-src 'unsafe-inline' 'self' writ-preview:; \
img-src 'self' writ-preview: data:; \
font-src 'self' writ-preview: data:; \
media-src 'self' writ-preview: data:; \
connect-src 'none'; \
object-src 'none'; \
frame-src 'none'; \
worker-src 'none'; \
form-action 'none'; \
base-uri 'none'; \
frame-ancestors tauri://localhost http://localhost:1420; \
navigate-to 'none'; \
sandbox allow-scripts allow-same-origin";

    const DOCUMENT_CSP_SCRIPTS_OFF: &str = "default-src 'none'; \
script-src 'self' writ-preview:; \
style-src 'unsafe-inline' 'self' writ-preview:; \
img-src 'self' writ-preview: data:; \
font-src 'self' writ-preview: data:; \
media-src 'self' writ-preview: data:; \
connect-src 'none'; \
object-src 'none'; \
frame-src 'none'; \
worker-src 'none'; \
form-action 'none'; \
base-uri 'none'; \
frame-ancestors tauri://localhost http://localhost:1420; \
navigate-to 'none'; \
sandbox allow-scripts allow-same-origin";

    const WINDOWS_DOCUMENT_CSP_SCRIPTS_ON: &str = "default-src 'none'; \
script-src 'unsafe-inline' 'self' writ-preview: http://writ-preview.document; \
style-src 'unsafe-inline' 'self' writ-preview: http://writ-preview.document; \
img-src 'self' writ-preview: http://writ-preview.document data:; \
font-src 'self' writ-preview: http://writ-preview.document data:; \
media-src 'self' writ-preview: http://writ-preview.document data:; \
connect-src 'none'; \
object-src 'none'; \
frame-src 'none'; \
worker-src 'none'; \
form-action 'none'; \
base-uri 'none'; \
frame-ancestors http://tauri.localhost https://tauri.localhost http://localhost:1420; \
navigate-to 'none'; \
sandbox allow-scripts allow-same-origin";

    const WINDOWS_DOCUMENT_CSP_SCRIPTS_OFF: &str = "default-src 'none'; \
script-src 'self' writ-preview: http://writ-preview.document; \
style-src 'unsafe-inline' 'self' writ-preview: http://writ-preview.document; \
img-src 'self' writ-preview: http://writ-preview.document data:; \
font-src 'self' writ-preview: http://writ-preview.document data:; \
media-src 'self' writ-preview: http://writ-preview.document data:; \
connect-src 'none'; \
object-src 'none'; \
frame-src 'none'; \
worker-src 'none'; \
form-action 'none'; \
base-uri 'none'; \
frame-ancestors http://tauri.localhost https://tauri.localhost http://localhost:1420; \
navigate-to 'none'; \
sandbox allow-scripts allow-same-origin";

    fn clause<'a>(csp: &'a str, directive: &str) -> &'a str {
        csp.split("; ")
            .find(|c| c.split_whitespace().next() == Some(directive))
            .unwrap_or_else(|| panic!("no {directive} in {csp}"))
    }

    #[test]
    fn document_csp_matches_the_locked_bytes_on_macos_and_linux() {
        assert_eq!(document_csp_for(true, false), DOCUMENT_CSP_SCRIPTS_ON);
        assert_eq!(document_csp_for(false, false), DOCUMENT_CSP_SCRIPTS_OFF);
    }

    #[test]
    fn document_csp_matches_the_locked_bytes_on_windows() {
        assert_eq!(
            document_csp_for(true, true),
            WINDOWS_DOCUMENT_CSP_SCRIPTS_ON
        );
        assert_eq!(
            document_csp_for(false, true),
            WINDOWS_DOCUMENT_CSP_SCRIPTS_OFF
        );
    }

    #[test]
    fn this_build_serves_its_own_platforms_policy() {
        let windows = cfg!(target_os = "windows");
        for enabled in [true, false] {
            assert_eq!(
                build_document_csp(enabled),
                document_csp_for(enabled, windows)
            );
        }
        assert_eq!(build_chrome_csp(), chrome_csp_for(windows));
    }

    #[test]
    fn document_csp_never_permits_network() {
        for windows in [false, true] {
            for enabled in [true, false] {
                let csp = document_csp_for(enabled, windows);
                assert_eq!(clause(&csp, "connect-src"), "connect-src 'none'");
                // The egress directives admit no remote scheme: only data:, the
                // preview scheme and, on Windows, the document's own WebView2
                // origin. frame-ancestors names the app origins and is not an
                // egress channel, so it is not checked here.
                for directive in [
                    "script-src",
                    "style-src",
                    "img-src",
                    "font-src",
                    "media-src",
                ] {
                    let clause = clause(&csp, directive);
                    assert!(!clause.contains("https:"), "no https in {clause}");
                    assert!(!clause.contains("ws:"), "no websocket in {clause}");
                    assert!(
                        !clause.contains("writ-workspace:"),
                        "no workspace in {clause}"
                    );
                    for token in clause.split_whitespace().skip(1) {
                        if let Some(rest) = token.strip_prefix("http://") {
                            assert!(windows, "an http origin on macOS/Linux: {clause}");
                            assert_eq!(rest, "writ-preview.document", "{clause}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn document_csp_sandboxes_the_frame() {
        for windows in [false, true] {
            for enabled in [true, false] {
                let csp = document_csp_for(enabled, windows);
                // The sandbox omits top navigation, popups and forms, so a
                // document cannot navigate the window away or post a form out;
                // it keeps scripts and its own origin.
                assert_eq!(
                    clause(&csp, "sandbox"),
                    "sandbox allow-scripts allow-same-origin"
                );
                assert!(!csp.contains("allow-top-navigation"));
                assert!(!csp.contains("allow-popups"));
                assert!(!csp.contains("allow-forms"));
            }
        }
    }

    #[test]
    fn only_the_windows_policy_names_the_webview2_origin() {
        for enabled in [true, false] {
            assert!(!document_csp_for(enabled, false).contains(WINDOWS_DOCUMENT_ORIGIN));
            let windows = document_csp_for(enabled, true);
            for directive in [
                "script-src",
                "style-src",
                "img-src",
                "font-src",
                "media-src",
            ] {
                let clause = clause(&windows, directive);
                assert!(
                    clause.contains(WINDOWS_DOCUMENT_ORIGIN),
                    "{directive} must list {WINDOWS_DOCUMENT_ORIGIN}: {clause}"
                );
                assert!(clause.contains("'self'"), "{directive} must list 'self'");
            }
        }
    }

    #[test]
    fn frame_ancestors_names_each_platforms_app_origin() {
        for csp in [document_csp_for(true, false), chrome_csp_for(false)] {
            assert_eq!(
                clause(&csp, "frame-ancestors"),
                "frame-ancestors tauri://localhost http://localhost:1420"
            );
        }
        for csp in [document_csp_for(true, true), chrome_csp_for(true)] {
            assert_eq!(
                clause(&csp, "frame-ancestors"),
                "frame-ancestors http://tauri.localhost https://tauri.localhost http://localhost:1420"
            );
        }
    }

    #[test]
    fn scripts_off_blocks_author_inline_but_keeps_first_party_runtimes() {
        for windows in [false, true] {
            let csp = document_csp_for(false, windows);
            // No 'unsafe-inline' for scripts, so an author inline <script> cannot run.
            assert!(!clause(&csp, "script-src").contains("'unsafe-inline'"));
            // 'self' stays, so the bundled bridge / Mermaid / KaTeX runtimes
            // served under _assets keep loading.
            assert!(clause(&csp, "script-src").contains("'self'"));
            // Author inline styles still apply (presentational, no egress).
            assert!(clause(&csp, "style-src").contains("'unsafe-inline'"));
        }
    }

    #[test]
    fn chrome_csp_runs_self_scripts_without_network() {
        for windows in [false, true] {
            let csp = chrome_csp_for(windows);
            assert_eq!(clause(&csp, "script-src"), "script-src 'self'");
            assert_eq!(clause(&csp, "connect-src"), "connect-src 'none'");
            assert!(!csp.contains("'unsafe-eval'"));
            assert!(!clause(&csp, "frame-ancestors").contains("'none'"));
        }
    }
}
