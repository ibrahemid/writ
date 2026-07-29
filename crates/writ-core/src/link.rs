//! External-link policy — ADR-025.
//!
//! Buffers and the preview routinely hold text written by an agent or pulled
//! off the network, so a link is never handed to the operating system on the
//! strength of how it looks. [`classify_link`] is the single decision point:
//! it parses the raw string with `url::Url` and returns either an
//! [`LinkTarget::External`] carrying the parser's own normalized serialization
//! or a [`LinkTarget::Rejected`] with the reason.
//!
//! The allowlist is `http`, `https`, and `mailto`. Everything else is refused,
//! including schemes that look harmless: an unregistered scheme is exactly how
//! a link hands control to an arbitrary installed application.

use url::Url;

/// Schemes that may be handed to the operating system.
const ALLOWED_SCHEMES: [&str; 3] = ["http", "https", "mailto"];

/// Why a raw link string was refused.
///
/// The reason is safe to log; the URL that produced it is not, so callers
/// record the reason alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectReason {
    /// The scheme is outside the allowlist.
    Scheme,
    /// An `http`/`https` URL with no host, or a `mailto:` with no address.
    NoHost,
    /// The authority carries a username or password.
    Credentials,
    /// The raw string holds a control character or is padded with whitespace.
    Control,
    /// The raw string is not a URL at all.
    Unparseable,
}

impl RejectReason {
    /// Stable machine-readable code, used in logs and over IPC.
    pub fn code(self) -> &'static str {
        match self {
            RejectReason::Scheme => "scheme",
            RejectReason::NoHost => "no_host",
            RejectReason::Credentials => "credentials",
            RejectReason::Control => "control",
            RejectReason::Unparseable => "unparseable",
        }
    }
}

/// The verdict on a raw link string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkTarget {
    /// Safe to open. Carries `Url::as_str` from the parsed URL, never the
    /// caller's raw string.
    External(String),
    /// Refused, with the reason.
    Rejected(RejectReason),
}

/// Classifies a raw link string against the external-link policy.
///
/// The checks run in a fixed order, and the order is load-bearing. Control
/// characters and surrounding whitespace are refused *before* parsing, because
/// the WHATWG URL parser strips tab, newline, and carriage return from its
/// input: `java\nscript:alert(1)` would otherwise reach the scheme check as a
/// well-formed `javascript:` URL, and a future allowlist entry could turn the
/// same trick into an accepted link. Rejecting the raw form keeps the string
/// the user sees and the string that is judged identical.
pub fn classify_link(raw: &str) -> LinkTarget {
    if raw.chars().any(char::is_control) || raw.trim() != raw {
        return LinkTarget::Rejected(RejectReason::Control);
    }

    let Ok(url) = Url::parse(raw) else {
        return LinkTarget::Rejected(RejectReason::Unparseable);
    };

    // `Url` lowercases the scheme during parsing, so `HTTPS:` compares equal.
    if !ALLOWED_SCHEMES.contains(&url.scheme()) {
        return LinkTarget::Rejected(RejectReason::Scheme);
    }

    if url.scheme() == "mailto" {
        if url.path().is_empty() {
            return LinkTarget::Rejected(RejectReason::NoHost);
        }
        return LinkTarget::External(url.as_str().to_owned());
    }

    if !url.username().is_empty() || url.password().is_some() {
        return LinkTarget::Rejected(RejectReason::Credentials);
    }

    match url.host_str() {
        Some(host) if !host.is_empty() => LinkTarget::External(url.as_str().to_owned()),
        _ => LinkTarget::Rejected(RejectReason::NoHost),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reject(raw: &str) -> RejectReason {
        match classify_link(raw) {
            LinkTarget::Rejected(reason) => reason,
            LinkTarget::External(url) => panic!("expected rejection, got {url}"),
        }
    }

    fn external(raw: &str) -> String {
        match classify_link(raw) {
            LinkTarget::External(url) => url,
            LinkTarget::Rejected(reason) => panic!("expected acceptance, got {:?}", reason.code()),
        }
    }

    #[test]
    fn accepts_the_three_allowed_schemes() {
        assert_eq!(external("http://example.com/"), "http://example.com/");
        assert_eq!(external("https://example.com/"), "https://example.com/");
        assert_eq!(external("mailto:a@example.com"), "mailto:a@example.com");
    }

    #[test]
    fn returns_the_parsers_normalization_not_the_raw_string() {
        assert_eq!(external("https://example.com"), "https://example.com/");
        assert_eq!(external("https://EXAMPLE.com/A"), "https://example.com/A");
    }

    #[test]
    fn accepts_an_uppercase_scheme() {
        assert_eq!(external("HTTPS://example.com/"), "https://example.com/");
    }

    #[test]
    fn accepts_mailto_with_and_without_a_body() {
        assert_eq!(external("mailto:a@example.com"), "mailto:a@example.com");
        assert_eq!(
            external("mailto:a@example.com?subject=Hi&body=There"),
            "mailto:a@example.com?subject=Hi&body=There"
        );
    }

    #[test]
    fn rejects_an_empty_mailto_address() {
        assert_eq!(reject("mailto:"), RejectReason::NoHost);
    }

    #[test]
    fn rejects_javascript_in_any_casing() {
        assert_eq!(reject("javascript:alert(1)"), RejectReason::Scheme);
        assert_eq!(reject("JavaScript:alert(1)"), RejectReason::Scheme);
        assert_eq!(reject("JAVASCRIPT:alert(1)"), RejectReason::Scheme);
    }

    #[test]
    fn rejects_scheme_smuggling_through_a_control_character() {
        assert_eq!(reject("java\nscript:alert(1)"), RejectReason::Control);
        assert_eq!(reject("java\tscript:alert(1)"), RejectReason::Control);
        assert_eq!(reject("java\rscript:alert(1)"), RejectReason::Control);
        assert_eq!(reject("javascript\u{0}:alert(1)"), RejectReason::Control);
    }

    #[test]
    fn rejects_whitespace_padding_rather_than_trimming_it() {
        assert_eq!(reject("  https://example.com"), RejectReason::Control);
        assert_eq!(reject("https://example.com  "), RejectReason::Control);
        assert_eq!(reject("\u{a0}https://example.com"), RejectReason::Control);
    }

    #[test]
    fn rejects_every_other_scheme() {
        for raw in [
            "data:text/html;base64,PHNjcmlwdD4=",
            "file:///etc/passwd",
            "vbscript:msgbox(1)",
            "blob:https://example.com/1234",
            "about:blank",
            "writ://open",
            "ms-msdt:/id",
        ] {
            assert_eq!(reject(raw), RejectReason::Scheme, "scheme allowed: {raw}");
        }
    }

    #[test]
    fn rejects_credentials_in_the_authority() {
        assert_eq!(
            reject("https://user:pw@example.com/"),
            RejectReason::Credentials
        );
        assert_eq!(
            reject("https://user@example.com/"),
            RejectReason::Credentials
        );
        assert_eq!(
            reject("https://bank.example.com@evil.example/"),
            RejectReason::Credentials
        );
    }

    #[test]
    fn rejects_an_empty_host() {
        // A special scheme with an empty authority never reaches the host
        // check: the parser refuses it outright, so the verdict is
        // `Unparseable`. `NoHost` stays as the defensive check behind it.
        assert_eq!(reject("https://"), RejectReason::Unparseable);
        assert_eq!(reject("http://"), RejectReason::Unparseable);
        assert_eq!(reject("https://:8080/"), RejectReason::Unparseable);
    }

    #[test]
    fn rejects_a_unicode_lookalike_scheme() {
        // Cyrillic "һ" is not an ASCII scheme character, so this never parses
        // as a URL and can never be mistaken for https.
        assert_eq!(reject("һttps://example.com"), RejectReason::Unparseable);
        assert_eq!(reject("httрs://example.com"), RejectReason::Unparseable);
    }

    #[test]
    fn rejects_input_that_is_not_a_url() {
        assert_eq!(reject(""), RejectReason::Unparseable);
        assert_eq!(reject("example.com"), RejectReason::Unparseable);
        assert_eq!(reject("./notes.md"), RejectReason::Unparseable);
        assert_eq!(reject("/etc/passwd"), RejectReason::Unparseable);
    }

    #[test]
    fn keeps_a_percent_encoded_newline_encoded() {
        // Percent-encoded control characters are inert: they stay encoded in
        // the normalized output and never re-enter the raw string.
        let url = external("https://example.com/a%0Ab");
        assert_eq!(url, "https://example.com/a%0Ab");
        assert!(!url.contains('\n'));
    }

    #[test]
    fn reason_codes_are_stable() {
        assert_eq!(RejectReason::Scheme.code(), "scheme");
        assert_eq!(RejectReason::NoHost.code(), "no_host");
        assert_eq!(RejectReason::Credentials.code(), "credentials");
        assert_eq!(RejectReason::Control.code(), "control");
        assert_eq!(RejectReason::Unparseable.code(), "unparseable");
    }
}
