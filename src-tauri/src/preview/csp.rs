//! Per-webview CSP construction.
//!
//! Phase 1 ships the **default-deny baseline** for both scopes — enough to
//! prove the per-webview CSP plumbing works. Phase 3 (ADR-011) populates
//! the full eight-cell matrix (chrome × any, document × {SAFE,
//! ALLOW_SCRIPTS, ALLOW_NETWORK, ALLOW_BOTH}) and asserts each cell's
//! byte sequence in unit tests.
//!
//! The function signature `build_csp(scope, policy) -> String` is the
//! commitment Phase 3 lands against without changing call sites.

use writ_core::preview::PreviewPolicy;

use super::protocol::PreviewScope;

/// Build the CSP string for a given scope + policy combination.
///
/// Phase 1: both scopes return a default-deny baseline regardless of the
/// `policy` argument. Phase 3 expands this into eight distinct cells per
/// ADR-011's matrix. The signature is the contract.
pub fn build_csp(scope: PreviewScope, _policy: PreviewPolicy) -> String {
    match scope {
        PreviewScope::Chrome => chrome_baseline(),
        PreviewScope::Document => document_baseline(),
    }
}

fn chrome_baseline() -> String {
    [
        "default-src 'none'",
        "script-src 'self'",
        "style-src 'self'",
        "img-src 'self' data:",
        "font-src 'self'",
        "connect-src 'self'",
        "media-src 'self'",
        "object-src 'none'",
        "manifest-src 'none'",
        "prefetch-src 'none'",
        "frame-src 'none'",
        "worker-src 'none'",
        "form-action 'none'",
        "base-uri 'self'",
        "frame-ancestors 'none'",
        "navigate-to 'self'",
    ]
    .join("; ")
}

fn document_baseline() -> String {
    [
        "default-src 'none'",
        "script-src 'none'",
        "style-src 'self' 'unsafe-inline' writ-preview:",
        "img-src writ-preview: writ-workspace: data:",
        "font-src writ-preview: writ-workspace:",
        "connect-src 'none'",
        "media-src 'none'",
        "object-src 'none'",
        "manifest-src 'none'",
        "prefetch-src 'none'",
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

    #[test]
    fn chrome_baseline_has_default_deny() {
        let csp = build_csp(PreviewScope::Chrome, PreviewPolicy::Safe);
        assert!(csp.contains("default-src 'none'"));
        assert!(csp.contains("script-src 'self'"));
        assert!(csp.contains("frame-src 'none'"));
        // Chrome scope never grants 'unsafe-inline' or 'unsafe-eval'.
        assert!(!csp.contains("'unsafe-inline'"));
        assert!(!csp.contains("'unsafe-eval'"));
    }

    #[test]
    fn document_baseline_has_default_deny() {
        let csp = build_csp(PreviewScope::Document, PreviewPolicy::Safe);
        assert!(csp.contains("default-src 'none'"));
        assert!(csp.contains("script-src 'none'"));
        assert!(csp.contains("connect-src 'none'"));
        assert!(csp.contains("base-uri 'none'"));
    }

    #[test]
    fn phase_1_policy_argument_is_inert() {
        // Until Phase 3 wires the per-cell matrix, the policy argument
        // does not affect the returned bytes. This test pins the
        // baseline-only contract so the Phase 3 PR explicitly removes
        // it as part of the matrix landing.
        let safe = build_csp(PreviewScope::Document, PreviewPolicy::Safe);
        let scripts = build_csp(PreviewScope::Document, PreviewPolicy::AllowScripts);
        assert_eq!(safe, scripts);
    }
}
