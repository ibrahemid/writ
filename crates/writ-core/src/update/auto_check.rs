//! Policy for the silent startup update check.
//!
//! The check used to fire unconditionally a few seconds after every launch,
//! so a quick open-and-quit still phoned the release endpoint. This module
//! decides whether a silent check should run, given the user's opt-out and
//! when the last silent check happened. The timestamp itself is persisted by
//! the `writ-tauri` adapter outside `config.toml`; this function is pure so
//! the gating is unit-testable without I/O or a clock.

/// Minimum spacing between silent startup checks: once per 24 hours. A manual
/// "Check for Updates…" is never gated by this.
pub const MIN_CHECK_INTERVAL_MS: u64 = 24 * 60 * 60 * 1000;

/// Returns whether a silent startup update check should run now.
///
/// - `auto_check`: the user's `updater.auto_check` preference.
/// - `last_check_ms`: epoch-ms of the last silent check, or `None` if never.
/// - `now_ms`: current epoch-ms.
/// - `min_interval_ms`: minimum spacing between silent checks.
///
/// A check runs only when auto-check is enabled and either no check has ever
/// run or at least `min_interval_ms` has elapsed since the last one. A
/// `last_check_ms` in the future (clock moved backwards) is treated as "too
/// soon" rather than triggering a check, via saturating subtraction.
pub fn should_auto_check(
    auto_check: bool,
    last_check_ms: Option<u64>,
    now_ms: u64,
    min_interval_ms: u64,
) -> bool {
    if !auto_check {
        return false;
    }
    match last_check_ms {
        None => true,
        Some(last) => now_ms.saturating_sub(last) >= min_interval_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = MIN_CHECK_INTERVAL_MS;

    #[test]
    fn disabled_never_checks() {
        assert!(!should_auto_check(false, None, 10 * DAY, DAY));
        assert!(!should_auto_check(false, Some(0), 10 * DAY, DAY));
    }

    #[test]
    fn first_ever_check_runs_when_enabled() {
        assert!(should_auto_check(true, None, 1_000, DAY));
    }

    #[test]
    fn recent_check_is_skipped() {
        // Half a day since the last check: too soon.
        assert!(!should_auto_check(
            true,
            Some(10 * DAY),
            10 * DAY + DAY / 2,
            DAY
        ));
    }

    #[test]
    fn check_runs_once_the_interval_has_elapsed() {
        // A full day later: due.
        assert!(should_auto_check(true, Some(10 * DAY), 11 * DAY, DAY));
        // One ms short of the interval: still too soon.
        assert!(!should_auto_check(true, Some(10 * DAY), 11 * DAY - 1, DAY));
        // Exactly at the boundary counts as elapsed.
        assert!(should_auto_check(true, Some(0), DAY, DAY));
    }

    #[test]
    fn future_last_check_does_not_trigger() {
        // Clock skew: recorded check is in the "future". Must not check.
        assert!(!should_auto_check(true, Some(20 * DAY), 10 * DAY, DAY));
    }
}
