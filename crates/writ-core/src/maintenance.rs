//! Database maintenance policy.
//!
//! SQLite never returns freed pages to the filesystem on its own: a deleted
//! row leaves its pages on the freelist, and a table that is written and
//! pruned on a timer therefore grows a file that is almost entirely free
//! space. This module decides when reclaiming that space is worth a `VACUUM`.
//! The SQL itself lives in `writ-storage`.

/// Free-page share, in percent of total pages, above which a database is
/// considered bloated.
pub const VACUUM_FREE_PERCENT: u64 = 50;

/// Minimum freelist size, in pages, before a vacuum is considered at all.
///
/// At SQLite's 4 KiB default page size this is 4 MiB. Below it the reclaim is
/// not worth rewriting the file, and a small database is routinely more than
/// half free right after a delete.
pub const VACUUM_MIN_FREE_PAGES: u64 = 1024;

/// Returns `true` when free pages dominate the database file enough to
/// justify a `VACUUM`.
///
/// `page_count` and `freelist_count` are the values reported by the
/// same-named SQLite pragmas.
pub fn needs_vacuum(page_count: u64, freelist_count: u64) -> bool {
    if freelist_count < VACUUM_MIN_FREE_PAGES || freelist_count > page_count {
        return false;
    }
    freelist_count * 100 > page_count * VACUUM_FREE_PERCENT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_database_never_vacuums() {
        assert!(!needs_vacuum(0, 0));
    }

    #[test]
    fn small_database_never_vacuums_however_free_it_is() {
        assert!(!needs_vacuum(1000, 999));
    }

    #[test]
    fn large_mostly_free_database_vacuums() {
        assert!(needs_vacuum(306_714, 305_662));
    }

    #[test]
    fn half_free_is_not_enough() {
        assert!(!needs_vacuum(4000, 2000));
    }

    #[test]
    fn just_over_half_free_is_enough() {
        assert!(needs_vacuum(4000, 2001));
    }

    #[test]
    fn freelist_at_the_minimum_is_below_the_bar() {
        assert!(!needs_vacuum(1500, VACUUM_MIN_FREE_PAGES - 1));
        assert!(needs_vacuum(1500, VACUUM_MIN_FREE_PAGES));
    }

    #[test]
    fn nonsensical_freelist_larger_than_the_file_is_ignored() {
        assert!(!needs_vacuum(100, 5000));
    }
}
