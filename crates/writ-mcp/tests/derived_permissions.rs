//! What a verdict grants, and that nothing is granted no caller uses.
//!
//! The gate decides a direction and the tool layer turns that verdict into a
//! permission set (ADR-032 section 2). Two sets, seven capabilities, and their
//! union is the whole enum: a capability outside it would be a grant the sandbox
//! defines and no tool call can reach, which is the shape section 1 records
//! against.

use writ_core::notes::host::{Capability, PermissionSet};
use writ_mcp::tools::{read_permissions, write_permissions};

/// The capabilities in `set`, in the order [`Capability::ALL`] names them.
fn listed(set: &PermissionSet) -> Vec<Capability> {
    Capability::ALL
        .iter()
        .copied()
        .filter(|capability| set.contains(*capability))
        .collect()
}

#[test]
fn an_allowed_read_grants_the_four_reads_and_no_write() {
    let reading = read_permissions();

    assert_eq!(
        listed(&reading),
        vec![
            Capability::ListNotes,
            Capability::ReadNote,
            Capability::SearchNotes,
            Capability::ReadIndex,
        ]
    );
    assert!(reading.is_read_only());
}

#[test]
fn an_allowed_write_grants_the_three_writes_and_no_read() {
    let writing = write_permissions();

    assert_eq!(
        listed(&writing),
        vec![
            Capability::WriteNote,
            Capability::CreateNote,
            Capability::RenameNote,
        ]
    );
    assert!(!writing.is_read_only());
}

#[test]
fn the_two_sets_together_are_every_capability_and_overlap_in_none() {
    let reading = read_permissions();
    let writing = write_permissions();

    let both: Vec<Capability> = Capability::ALL
        .iter()
        .copied()
        .filter(|capability| reading.contains(*capability) || writing.contains(*capability))
        .collect();
    assert_eq!(both, Capability::ALL.to_vec(), "a capability has no caller");

    let shared: Vec<Capability> = Capability::ALL
        .iter()
        .copied()
        .filter(|capability| reading.contains(*capability) && writing.contains(*capability))
        .collect();
    assert!(
        shared.is_empty(),
        "a read verdict and a write verdict grant different things: {shared:?}"
    );
}
