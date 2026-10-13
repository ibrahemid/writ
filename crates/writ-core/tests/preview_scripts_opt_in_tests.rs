//! An opt-in stands while the bytes Writ holds for the file are the ones it
//! covers, lapses when Writ holds others, and is left alone when Writ holds
//! none; a write carries it over only from bytes it covers (ADR-043).

use writ_core::hash::Sha256Digest;
use writ_core::preview::scripts::{opt_in_carries_over_write, opt_in_standing, OptInStanding};

const COVERED: Sha256Digest = [1; 32];
const OTHER: Sha256Digest = [2; 32];

#[test]
fn an_opt_in_stands_while_writ_holds_the_bytes_it_covers() {
    assert_eq!(
        opt_in_standing(Some(COVERED), Some(COVERED)),
        OptInStanding::Stands
    );
    assert!(opt_in_carries_over_write(Some(COVERED), Some(COVERED)));
}

#[test]
fn an_opt_in_lapses_when_writ_holds_other_bytes() {
    assert_eq!(
        opt_in_standing(Some(COVERED), Some(OTHER)),
        OptInStanding::Lapses
    );
    assert!(!opt_in_carries_over_write(Some(COVERED), Some(OTHER)));
}

#[test]
fn an_opt_in_given_before_the_file_had_bytes_covers_none() {
    assert_eq!(opt_in_standing(None, None), OptInStanding::Stands);
    assert!(
        opt_in_carries_over_write(None, None),
        "the first save of a file opted in before it existed carries the opt-in"
    );
    assert_eq!(opt_in_standing(None, Some(OTHER)), OptInStanding::Lapses);
    assert!(
        !opt_in_carries_over_write(None, Some(OTHER)),
        "a file another program put at the path before the first save is not covered"
    );
}

#[test]
fn an_opt_in_with_nothing_to_compare_runs_nothing_and_carries_nothing_over_a_save() {
    assert_eq!(
        opt_in_standing(Some(COVERED), None),
        OptInStanding::Unchecked
    );
    assert!(!opt_in_carries_over_write(Some(COVERED), None));
}
