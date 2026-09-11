//! What a consumer of the note surface is allowed to ask for.

use std::collections::BTreeSet;

/// One thing a consumer may do to the notes folder.
///
/// `ReadNote` and `ReadIndex` are separate grants because they have different
/// backing and different failure: with no index the folder still lists and
/// reads, while every index-derived answer is unavailable. The three writes are
/// separate because a consumer approved to update a note it was pointed at is
/// not by that fact approved to add files to the folder or to move one
/// (ADR-032 section 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Capability {
    /// List the notes in the folder.
    ListNotes,
    /// Read a note's text, or its size.
    ReadNote,
    /// Search the folder's text.
    SearchNotes,
    /// Read what the index holds about a note: links, backlinks, properties,
    /// tags, and the folder's tags.
    ReadIndex,
    /// Replace the text of a note that is already there.
    WriteNote,
    /// Mint a note the folder does not hold.
    CreateNote,
    /// Rename a note inside the folder it is in.
    RenameNote,
}

impl Capability {
    /// Every capability, reads first.
    pub const ALL: &'static [Capability] = &[
        Capability::ListNotes,
        Capability::ReadNote,
        Capability::SearchNotes,
        Capability::ReadIndex,
        Capability::WriteNote,
        Capability::CreateNote,
        Capability::RenameNote,
    ];

    /// Whether holding this changes a note.
    pub fn is_write(self) -> bool {
        matches!(
            self,
            Capability::WriteNote | Capability::CreateNote | Capability::RenameNote
        )
    }
}

/// The capabilities one consumer holds.
///
/// A consumer whose set has no write capability has no code path to a write,
/// because the only write path is behind the check the implementation makes
/// first. The empty set is the default, so a wiring mistake costs a refusal
/// rather than a read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PermissionSet {
    held: BTreeSet<Capability>,
}

impl PermissionSet {
    /// Whether this set holds `capability`.
    pub fn contains(&self, capability: Capability) -> bool {
        self.held.contains(&capability)
    }

    /// Whether nothing in this set changes a note.
    ///
    /// True of the empty set: a consumer that may do nothing may not write.
    pub fn is_read_only(&self) -> bool {
        !self.held.iter().copied().any(Capability::is_write)
    }

    /// The capabilities held, in the order [`Capability::ALL`] names them.
    pub fn iter(&self) -> impl Iterator<Item = Capability> + '_ {
        self.held.iter().copied()
    }
}

impl FromIterator<Capability> for PermissionSet {
    fn from_iter<T: IntoIterator<Item = Capability>>(held: T) -> Self {
        Self {
            held: held.into_iter().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_capability_is_named_once() {
        let mut seen = BTreeSet::new();
        for capability in Capability::ALL {
            match capability {
                Capability::ListNotes
                | Capability::ReadNote
                | Capability::SearchNotes
                | Capability::ReadIndex
                | Capability::WriteNote
                | Capability::CreateNote
                | Capability::RenameNote => {}
            }
            assert!(seen.insert(*capability), "{capability:?} is listed twice");
        }
        assert_eq!(
            seen.len(),
            7,
            "a capability the enum holds is missing from ALL"
        );
    }

    #[test]
    fn a_set_that_only_reads_is_read_only() {
        let reading: PermissionSet = [Capability::ReadNote].into_iter().collect();
        assert!(reading.is_read_only());
        assert!(PermissionSet::default().is_read_only());
    }

    #[test]
    fn a_set_holding_any_write_is_not_read_only() {
        for capability in Capability::ALL.iter().copied().filter(|c| c.is_write()) {
            let held: PermissionSet = [Capability::ReadNote, capability].into_iter().collect();
            assert!(!held.is_read_only(), "{capability:?}");
        }
    }

    #[test]
    fn a_set_holds_what_it_was_built_from_and_nothing_else() {
        let held: PermissionSet = [Capability::ListNotes, Capability::ReadIndex]
            .into_iter()
            .collect();
        assert!(held.contains(Capability::ListNotes));
        assert!(held.contains(Capability::ReadIndex));
        assert!(!held.contains(Capability::ReadNote));
        assert_eq!(held.iter().count(), 2);
    }
}
