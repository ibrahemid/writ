//! Navigation guard for the main app webview.
//!
//! Tauri installs no navigation guard by default, so a script in the window,
//! or a document in the preview iframe where the engine routes subframe
//! navigations through the same handler (macOS and Linux), could set
//! `top.location` to an external URL and carry note text out in the query
//! string (TS-02). This refuses any navigation that is not the app's own
//! origin, the preview scheme, or an inert `about:` page, and under
//! `tauri dev` the dev server's exact origin.
//!
//! It is one layer of several: the preview iframe is sandboxed without
//! `allow-top-navigation`, the app CSP's `frame-src` lists only the preview
//! origins, and the document CSP serves no remote scheme. WebView2 raises its
//! navigation event for the main frame only, so on Windows the frame-level
//! block is `frame-src`, which Chromium applies only after connecting to the
//! target; the document rewrite removes the frame navigations a document
//! starts without a script (ADR-043). This guard covers the main frame on
//! every engine.
//!
//! The predicate is pure so it is unit-tested without a running webview; it is
//! wired to every webview through an inline plugin's `on_navigation` hook in
//! `lib.rs`.

use url::Url;

/// Whether the main webview may navigate to `url` on this build's platform.
/// `dev_server` is the dev server the app is served from, if any
/// ([`dev_server`]).
pub fn is_allowed_navigation(url: &str, dev_server: Option<&Url>) -> bool {
    is_allowed_navigation_for(url, cfg!(target_os = "windows"), dev_server)
}

/// The dev server the app is served from under `tauri dev`: the app's
/// `devUrl`. `None` in a build that serves the app itself.
pub fn dev_server(config: &tauri::Config) -> Option<&Url> {
    if tauri::is_dev() {
        config.build.dev_url.as_ref()
    } else {
        None
    }
}

/// Whether the main webview may navigate to `url` on one platform.
///
/// Allowed everywhere: the preview scheme (`writ-preview://`), the app origin
/// (`tauri://localhost`) and inert `about:` pages, plus the origin of
/// `dev_server` when the app is served from one. On Windows, where WebView2
/// serves every custom scheme as `http(s)://<scheme>.localhost` or
/// `http://<scheme>.<host>`, the app, IPC, asset and preview origins in that
/// form are allowed too; on macOS and Linux those are ordinary HTTP hosts and
/// are refused. Everything else, an unparseable string included, is refused.
pub fn is_allowed_navigation_for(url: &str, windows: bool, dev_server: Option<&Url>) -> bool {
    let Ok(parsed) = Url::parse(url) else {
        return false;
    };
    if dev_server.is_some_and(|dev| dev.origin() == parsed.origin()) {
        return true;
    }
    let host = parsed.host_str();
    match parsed.scheme() {
        "writ-preview" => true,
        // The iframe parks on about:blank before the preview base is known.
        "about" => true,
        "tauri" => host == Some("localhost"),
        "http" | "https" => {
            windows
                && matches!(
                    host,
                    Some(
                        "tauri.localhost"
                            | "ipc.localhost"
                            | "asset.localhost"
                            | "writ-preview.document"
                            | "writ-preview.chrome"
                    )
                )
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// The predicate with no dev server, as in a build that serves the app.
    fn allowed(url: &str, windows: bool) -> bool {
        is_allowed_navigation_for(url, windows, None)
    }

    const WEBVIEW2_FORMS: [&str; 6] = [
        "http://tauri.localhost/",
        "https://tauri.localhost/index.html",
        "http://ipc.localhost/plugin%3Aevent%7Clisten",
        "http://asset.localhost/x.png",
        "http://writ-preview.document/buf-1",
        "http://writ-preview.chrome/blank",
    ];

    #[test]
    fn the_app_origin_and_the_preview_scheme_are_allowed_everywhere() {
        for windows in [false, true] {
            assert!(allowed("tauri://localhost", windows));
            assert!(allowed("tauri://localhost/index.html", windows));
            assert!(allowed("writ-preview://document/buf-1", windows));
            assert!(allowed("writ-preview://chrome/blank", windows));
        }
    }

    #[test]
    fn the_webview2_forms_are_allowed_only_on_windows() {
        for url in WEBVIEW2_FORMS {
            assert!(allowed(url, true), "{url} on Windows");
            assert!(!allowed(url, false), "{url} on macOS/Linux");
        }
    }

    #[test]
    fn this_build_applies_its_own_platforms_rule() {
        let windows = cfg!(target_os = "windows");
        for url in WEBVIEW2_FORMS {
            assert_eq!(is_allowed_navigation(url, None), windows, "{url}");
        }
        assert!(!is_allowed_navigation(
            "https://attacker.example/?d=secret",
            None
        ));
    }

    #[test]
    fn inert_about_pages_are_allowed() {
        for windows in [false, true] {
            assert!(allowed("about:blank", windows));
            assert!(allowed("about:srcdoc", windows));
        }
    }

    #[test]
    fn an_external_url_is_refused() {
        for windows in [false, true] {
            assert!(!allowed("https://attacker.example/?d=secret", windows));
            assert!(!allowed("http://attacker.example/", windows));
            assert!(!allowed("https://github.com/", windows));
            assert!(!allowed("tauri://attacker.example/", windows));
        }
    }

    #[test]
    fn a_lookalike_preview_host_is_refused() {
        // `writ-preview.document` is an exact host; a registrable domain that
        // merely starts with it must not pass.
        for windows in [false, true] {
            assert!(!allowed("http://writ-preview.evil.example/", windows));
            assert!(!allowed("http://writ-preview.document.evil/", windows));
            assert!(!allowed("http://tauri.localhost.evil/", windows));
        }
    }

    #[test]
    fn dangerous_schemes_are_refused() {
        for windows in [false, true] {
            assert!(!allowed("data:text/html,<script>1</script>", windows));
            assert!(!allowed("javascript:alert(1)", windows));
            assert!(!allowed("file:///etc/passwd", windows));
            assert!(!allowed("blob:https://x/abc", windows));
        }
    }

    #[test]
    fn an_unparseable_url_is_refused() {
        for windows in [false, true] {
            assert!(!allowed("", windows));
            assert!(!allowed("not a url", windows));
            assert!(!allowed("://missing-scheme", windows));
        }
    }

    const DEV_SERVER: &str = "http://localhost:1420";

    #[test]
    fn the_dev_servers_origin_is_allowed_when_the_app_is_served_from_it() {
        let dev = Url::parse(DEV_SERVER).expect("dev url");
        for windows in [false, true] {
            assert!(is_allowed_navigation_for(
                "http://localhost:1420/",
                windows,
                Some(&dev)
            ));
            assert!(is_allowed_navigation_for(
                "http://localhost:1420/src/main.tsx",
                windows,
                Some(&dev)
            ));
        }
    }

    #[test]
    fn only_the_dev_servers_own_origin_is_allowed() {
        let dev = Url::parse(DEV_SERVER).expect("dev url");
        for windows in [false, true] {
            for url in [
                "http://localhost:9999/",
                "https://localhost:1420/",
                "http://localhost/",
                "ws://localhost:1420/",
                "http://127.0.0.1:1420/",
                "http://localhost.evil.example:1420/",
            ] {
                assert!(
                    !is_allowed_navigation_for(url, windows, Some(&dev)),
                    "{url}"
                );
            }
        }
    }

    #[test]
    fn no_localhost_url_is_allowed_without_a_dev_server() {
        for windows in [false, true] {
            assert!(!allowed("http://localhost:1420/", windows));
            assert!(!allowed("http://localhost/", windows));
        }
    }

    #[test]
    fn the_dev_server_is_the_configured_dev_url_under_tauri_dev() {
        let mut config = tauri::Config::default();
        assert_eq!(dev_server(&config), None);
        let dev = Url::parse(DEV_SERVER).expect("dev url");
        config.build.dev_url = Some(dev.clone());
        // `cargo test` builds tauri without its custom-protocol feature, as
        // `tauri dev` does; `tauri build` turns it on and serves the app.
        let expected = tauri::is_dev().then_some(&dev);
        assert_eq!(dev_server(&config), expected);
    }
}
