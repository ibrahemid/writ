use std::time::Duration;

use writ_core::recovery::{should_force_exit, QUIT_FLUSH_TIMEOUT};

#[test]
fn should_force_exit_is_false_before_the_timeout_without_confirmation() {
    assert!(!should_force_exit(Duration::from_millis(0), false));
    assert!(!should_force_exit(
        QUIT_FLUSH_TIMEOUT - Duration::from_millis(1),
        false
    ));
}

#[test]
fn should_force_exit_is_true_at_the_timeout() {
    assert!(should_force_exit(QUIT_FLUSH_TIMEOUT, false));
    assert!(should_force_exit(
        QUIT_FLUSH_TIMEOUT + Duration::from_millis(1),
        false
    ));
}

#[test]
fn should_force_exit_is_true_immediately_once_confirmed() {
    assert!(should_force_exit(Duration::from_millis(0), true));
}
