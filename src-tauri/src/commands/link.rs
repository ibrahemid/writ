use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;
use writ_core::link::{classify_link, LinkTarget, RejectReason};

/// The classification of a link, as the UI sees it. `url` is the normalized
/// destination and is present only when the link is allowed; `reason` is the
/// machine-readable refusal code and is present only when it is not.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct LinkVerdict {
    pub allowed: bool,
    pub url: Option<String>,
    pub reason: Option<String>,
    pub message: Option<String>,
}

/// User-facing refusal text. Names the rule that refused the link, so the
/// message tells the user something the link itself does not.
fn message_for(reason: RejectReason) -> String {
    match reason {
        RejectReason::Scheme => "Writ opens http, https, and mailto links only.".to_string(),
        RejectReason::Credentials => "That link carries a sign-in name in its address.".to_string(),
        RejectReason::NoHost | RejectReason::Control | RejectReason::Unparseable => {
            "That is not a valid web address.".to_string()
        }
    }
}

/// Classifies without opening. The read-only half of the surface: the same
/// policy `open_external_url` enforces, so the UI can render a refused link as
/// inert instead of offering an action that will fail.
fn verdict_for(raw: &str) -> LinkVerdict {
    match classify_link(raw) {
        LinkTarget::External(url) => LinkVerdict {
            allowed: true,
            url: Some(url),
            reason: None,
            message: None,
        },
        LinkTarget::Rejected(reason) => LinkVerdict {
            allowed: false,
            url: None,
            reason: Some(reason.code().to_string()),
            message: Some(message_for(reason)),
        },
    }
}

/// IPC: classify a link without opening anything.
#[tauri::command]
pub fn classify_external_url(url: String) -> LinkVerdict {
    verdict_for(&url)
}

/// IPC: open a link in the user's default browser or mail client.
///
/// The only path from Writ to an external handler. Classification runs first
/// and the opener receives the normalized string from the parser, never the
/// caller's raw input. `opener:allow-open-url` is deliberately absent from
/// `capabilities/default.json`, so the frontend cannot reach the plugin's own
/// command and bypass this check (ADR-025).
#[tauri::command]
pub fn open_external_url(app: AppHandle, url: String) -> Result<(), String> {
    match classify_link(&url) {
        LinkTarget::Rejected(reason) => {
            // The reason is safe to record; the URL is not.
            tracing::warn!(reason = reason.code(), "external link refused");
            Err(message_for(reason))
        }
        LinkTarget::External(target) => app.opener().open_url(target, None::<&str>).map_err(|e| {
            tracing::warn!(error = %e, "opening an external link failed");
            "Could not open the link.".to_string()
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allowed_schemes_pass_with_the_normalized_url() {
        let v = verdict_for("https://example.com");
        assert!(v.allowed);
        assert_eq!(v.url.as_deref(), Some("https://example.com/"));
        assert_eq!(v.reason, None);
        assert_eq!(v.message, None);
    }

    #[test]
    fn mailto_passes() {
        let v = verdict_for("mailto:a@example.com");
        assert!(v.allowed);
        assert_eq!(v.url.as_deref(), Some("mailto:a@example.com"));
    }

    #[test]
    fn every_disallowed_scheme_is_refused_before_the_opener() {
        for raw in [
            "javascript:alert(1)",
            "JavaScript:alert(1)",
            "java\nscript:alert(1)",
            "data:text/html,<script>1</script>",
            "file:///etc/passwd",
            "vbscript:msgbox(1)",
            "blob:https://example.com/1",
            "about:blank",
            "ms-msdt:/id",
            "https://user:pw@example.com/",
            "  https://example.com",
            "not a url",
        ] {
            let v = verdict_for(raw);
            assert!(!v.allowed, "link was allowed: {raw}");
            assert_eq!(v.url, None, "a refused link carried a url: {raw}");
            assert!(
                v.reason.is_some(),
                "a refused link carried no reason: {raw}"
            );
            assert!(
                v.message.is_some(),
                "a refused link carried no message: {raw}"
            );
        }
    }

    #[test]
    fn a_refused_verdict_never_echoes_the_url() {
        let v = verdict_for("javascript:alert(document.cookie)");
        assert_eq!(v.reason.as_deref(), Some("scheme"));
        assert!(!v.message.unwrap().contains("alert"));
    }

    #[test]
    fn refusal_messages_name_the_rule() {
        assert!(message_for(RejectReason::Scheme).contains("mailto"));
        assert_ne!(
            message_for(RejectReason::Credentials),
            message_for(RejectReason::Scheme)
        );
        assert_eq!(
            message_for(RejectReason::Control),
            message_for(RejectReason::Unparseable)
        );
    }

    #[test]
    fn verdict_serializes_the_keys_the_ui_reads() {
        let json = serde_json::to_value(verdict_for("https://example.com/")).unwrap();
        assert_eq!(json["allowed"], serde_json::json!(true));
        assert_eq!(json["url"], serde_json::json!("https://example.com/"));
        assert_eq!(json["reason"], serde_json::Value::Null);

        let json = serde_json::to_value(verdict_for("file:///etc/passwd")).unwrap();
        assert_eq!(json["allowed"], serde_json::json!(false));
        assert_eq!(json["url"], serde_json::Value::Null);
        assert_eq!(json["reason"], serde_json::json!("scheme"));
    }
}
