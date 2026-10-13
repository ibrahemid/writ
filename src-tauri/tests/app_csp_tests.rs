//! The app webview's Content-Security-Policy, read from the Tauri config files
//! the build merges for each platform.
//!
//! The preview frame is `writ-preview://<scope>/…` on macOS and Linux and
//! `http://writ-preview.<scope>/…` on Windows, where WebView2 serves a custom
//! scheme only in that form. `frame-src` has to name the form each platform
//! loads and only that form: on macOS and Linux `http://writ-preview.document`
//! is an ordinary HTTP host the resolver would look up. The Windows file
//! replaces the whole policy string, so the last test is what keeps the two
//! copies from drifting apart.

#[path = "support/app_csp.rs"]
mod app_csp;

use app_csp::{app_csp_for, read_config};

/// The policy as directive name to its source list, in source order.
fn directives(csp: &str) -> Vec<(String, String)> {
    csp.split(';')
        .map(str::trim)
        .filter(|clause| !clause.is_empty())
        .map(|clause| match clause.split_once(char::is_whitespace) {
            Some((name, sources)) => (name.to_string(), sources.trim().to_string()),
            None => (clause.to_string(), String::new()),
        })
        .collect()
}

fn frame_src(csp: &str) -> String {
    directives(csp)
        .into_iter()
        .find(|(name, _)| name == "frame-src")
        .map(|(_, sources)| sources)
        .expect("frame-src present")
}

#[test]
fn macos_and_linux_frame_only_the_native_preview_scheme() {
    let csp = app_csp_for(false);
    assert_eq!(frame_src(&csp), "writ-preview:");
    assert!(
        !csp.contains("writ-preview."),
        "the WebView2 preview origins are Windows-only: {csp}"
    );
}

#[test]
fn windows_frames_the_webview2_preview_origins() {
    assert_eq!(
        frame_src(&app_csp_for(true)),
        "http://writ-preview.chrome http://writ-preview.document"
    );
}

#[test]
fn the_windows_policy_differs_from_the_base_only_in_frame_src() {
    let without_frame_src = |csp: String| -> Vec<(String, String)> {
        directives(&csp)
            .into_iter()
            .filter(|(name, _)| name != "frame-src")
            .collect()
    };
    assert_eq!(
        without_frame_src(app_csp_for(true)),
        without_frame_src(app_csp_for(false))
    );
}

#[test]
fn no_other_platform_file_overrides_the_policy() {
    for name in ["tauri.macos.conf.json", "tauri.linux.conf.json"] {
        if let Some(patch) = read_config(name) {
            assert!(
                patch["app"]["security"]["csp"].is_null(),
                "{name} must not override the app CSP"
            );
        }
    }
}
