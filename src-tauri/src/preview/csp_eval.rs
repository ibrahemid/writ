//! A minimal, independent CSP evaluator — the in-repo oracle for the
//! exfil-denial suite.
//!
//! # Why this exists
//!
//! The `writ-preview://` protocol handler only ever receives
//! `writ-preview://` URLs (see [`super::handler::resolve`]). A remote
//! exfil request — `<img src="https://attacker/">`, `fetch('https://…')`,
//! a `wss://` socket — never reaches the handler: the webview's CSP
//! enforcement blocks it before any network or protocol dispatch. So the
//! disposition recorder **cannot** witness remote-exfil denial. The actual
//! boundary against exfil *is* the Content-Security-Policy, and the denial
//! is therefore a property of the CSP bytes, not of the handler.
//!
//! This module evaluates a CSP string against a request the way a
//! conformant engine would, so the suite can assert "the locked document
//! CSP denies each exfil vector" as a checkable property. The disposition
//! recorder is still used — for the scope/traversal boundary, which the
//! handler genuinely does observe.
//!
//! # Not a tautology
//!
//! An evaluator written to agree with our own policy string proves
//! nothing. This one implements real scheme/keyword matching and the
//! `default-src` fallback, and is validated three ways (see the tests):
//! the locked policy must **deny** the exfil vectors, must **allow** its
//! permitted sources, and must produce the obvious verdict on
//! **independent oracle policies** decoupled from Writ (`img-src https:`
//! allows https, `img-src 'none'` denies it, `img-src 'self'` denies
//! cross-origin but allows same-origin). A deny-everything stub passes the
//! deny class and fails the allow + oracle classes — which is the point.

use std::collections::HashMap;

/// A CSP fetch directive relevant to the preview threat model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Directive {
    /// `script-src`
    Script,
    /// `style-src`
    Style,
    /// `img-src`
    Img,
    /// `font-src`
    Font,
    /// `connect-src` — `fetch`, XHR, `sendBeacon`, `WebSocket`, `EventSource`.
    Connect,
    /// `media-src`
    Media,
}

impl Directive {
    fn name(self) -> &'static str {
        match self {
            Self::Script => "script-src",
            Self::Style => "style-src",
            Self::Img => "img-src",
            Self::Font => "font-src",
            Self::Connect => "connect-src",
            Self::Media => "media-src",
        }
    }
}

/// What a document is asking to load or run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resource {
    /// A URL fetch with a scheme (no trailing colon), e.g. `https`,
    /// `data`, `wss`, `writ-preview`. `same_origin` is true when the URL's
    /// origin equals the document's (the `'self'` keyword).
    Url {
        /// URL scheme, lowercased, without the trailing colon.
        scheme: String,
        /// Whether the resource is same-origin with the document.
        same_origin: bool,
    },
    /// Inline `<script>` / `<style>` / event-handler / inline-style — gated
    /// by `'unsafe-inline'`.
    Inline,
    /// `eval` / `new Function(...)` — gated by `'unsafe-eval'`.
    Eval,
}

impl Resource {
    /// A cross-origin URL of the given scheme.
    pub fn remote(scheme: &str) -> Self {
        Self::Url {
            scheme: scheme.to_string(),
            same_origin: false,
        }
    }

    /// A same-origin URL of the given scheme.
    pub fn same_origin(scheme: &str) -> Self {
        Self::Url {
            scheme: scheme.to_string(),
            same_origin: true,
        }
    }
}

/// The verdict for a request under a policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The policy permits the resource.
    Allow,
    /// The policy forbids the resource.
    Deny,
}

/// A parsed Content-Security-Policy: directive name → ordered source list.
#[derive(Debug, Clone)]
pub struct Csp {
    directives: HashMap<String, Vec<String>>,
}

impl Csp {
    /// Parse a CSP header value (`a 'b'; c d`) into directives.
    pub fn parse(header: &str) -> Self {
        let mut directives = HashMap::new();
        for clause in header.split(';') {
            let mut parts = clause.split_whitespace();
            let Some(name) = parts.next() else { continue };
            let sources: Vec<String> = parts.map(|s| s.to_string()).collect();
            directives.insert(name.to_ascii_lowercase(), sources);
        }
        Self { directives }
    }

    /// The effective source list for a directive: the directive itself if
    /// present, else `default-src`, else `None` (CSP imposes no restriction
    /// when neither is present).
    fn effective_sources(&self, directive: Directive) -> Option<&[String]> {
        self.directives
            .get(directive.name())
            .or_else(|| self.directives.get("default-src"))
            .map(|v| v.as_slice())
    }

    /// Evaluate a request: does the policy permit `resource` under
    /// `directive`?
    pub fn evaluate(&self, directive: Directive, resource: &Resource) -> Verdict {
        let Some(sources) = self.effective_sources(directive) else {
            // No matching directive and no default-src → unrestricted.
            return Verdict::Allow;
        };
        // `'none'` denies everything; per spec it is the sole value, but we
        // treat its presence as a hard deny defensively.
        if sources.iter().any(|s| s == "'none'") {
            return Verdict::Deny;
        }
        for source in sources {
            if source_matches(source, resource) {
                return Verdict::Allow;
            }
        }
        Verdict::Deny
    }
}

/// Whether a single source-expression token permits `resource`.
fn source_matches(source: &str, resource: &Resource) -> bool {
    match resource {
        Resource::Inline => source == "'unsafe-inline'",
        Resource::Eval => source == "'unsafe-eval'",
        Resource::Url {
            scheme,
            same_origin,
        } => {
            if source == "'self'" {
                return *same_origin;
            }
            // Keyword sources never match a URL fetch.
            if source.starts_with('\'') {
                return false;
            }
            // Scheme-source: `https:`, `data:`, `writ-preview:`. Match the
            // scheme exactly (case-insensitive), ignoring host-sources
            // (not used by the locked policy).
            if let Some(scheme_src) = source.strip_suffix(':') {
                if !scheme_src.contains('/') {
                    return scheme_src.eq_ignore_ascii_case(scheme);
                }
            }
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Oracle class: independent policies, obvious verdicts, decoupled
    // from Writ's own CSP. Proves the matcher implements scheme/keyword
    // semantics rather than merely agreeing with our policy string. ---

    #[test]
    fn oracle_scheme_source_allows_matching_scheme() {
        let csp = Csp::parse("img-src https:");
        assert_eq!(
            csp.evaluate(Directive::Img, &Resource::remote("https")),
            Verdict::Allow
        );
    }

    #[test]
    fn oracle_scheme_source_denies_other_schemes() {
        let csp = Csp::parse("img-src https:");
        assert_eq!(
            csp.evaluate(Directive::Img, &Resource::remote("data")),
            Verdict::Deny
        );
        assert_eq!(
            csp.evaluate(Directive::Img, &Resource::remote("http")),
            Verdict::Deny
        );
    }

    #[test]
    fn oracle_none_denies_everything() {
        let csp = Csp::parse("img-src 'none'");
        assert_eq!(
            csp.evaluate(Directive::Img, &Resource::remote("https")),
            Verdict::Deny
        );
        assert_eq!(
            csp.evaluate(Directive::Img, &Resource::same_origin("writ-preview")),
            Verdict::Deny
        );
    }

    #[test]
    fn oracle_self_allows_same_origin_denies_cross_origin() {
        let csp = Csp::parse("img-src 'self'");
        assert_eq!(
            csp.evaluate(Directive::Img, &Resource::same_origin("writ-preview")),
            Verdict::Allow
        );
        assert_eq!(
            csp.evaluate(Directive::Img, &Resource::remote("https")),
            Verdict::Deny
        );
    }

    #[test]
    fn oracle_default_src_is_the_fallback() {
        // img-src absent → falls back to default-src.
        let deny = Csp::parse("default-src 'none'");
        assert_eq!(
            deny.evaluate(Directive::Img, &Resource::remote("https")),
            Verdict::Deny
        );

        let allow = Csp::parse("default-src https:");
        assert_eq!(
            allow.evaluate(Directive::Img, &Resource::remote("https")),
            Verdict::Allow
        );
        assert_eq!(
            allow.evaluate(Directive::Img, &Resource::remote("data")),
            Verdict::Deny
        );
    }

    #[test]
    fn oracle_no_directive_and_no_default_is_unrestricted() {
        // Neither img-src nor default-src present → CSP imposes nothing.
        let csp = Csp::parse("script-src 'none'");
        assert_eq!(
            csp.evaluate(Directive::Img, &Resource::remote("https")),
            Verdict::Allow
        );
    }

    #[test]
    fn oracle_inline_gated_by_unsafe_inline() {
        assert_eq!(
            Csp::parse("style-src 'unsafe-inline'").evaluate(Directive::Style, &Resource::Inline),
            Verdict::Allow
        );
        assert_eq!(
            Csp::parse("style-src 'self'").evaluate(Directive::Style, &Resource::Inline),
            Verdict::Deny
        );
    }

    #[test]
    fn oracle_eval_gated_by_unsafe_eval() {
        assert_eq!(
            Csp::parse("script-src 'unsafe-eval'").evaluate(Directive::Script, &Resource::Eval),
            Verdict::Allow
        );
        assert_eq!(
            Csp::parse("script-src 'unsafe-inline'").evaluate(Directive::Script, &Resource::Eval),
            Verdict::Deny
        );
    }

    #[test]
    fn oracle_scheme_match_is_case_insensitive() {
        let csp = Csp::parse("img-src HTTPS:");
        assert_eq!(
            csp.evaluate(Directive::Img, &Resource::remote("https")),
            Verdict::Allow
        );
    }

    #[test]
    fn oracle_keyword_source_never_matches_a_url() {
        // 'unsafe-inline' permits inline content, never a URL fetch.
        let csp = Csp::parse("img-src 'unsafe-inline'");
        assert_eq!(
            csp.evaluate(Directive::Img, &Resource::remote("https")),
            Verdict::Deny
        );
    }
}
