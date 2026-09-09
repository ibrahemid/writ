//! The tools a connected program can call, named once.
//!
//! Three surfaces have to agree about this list: the server that registers the
//! tools, the gate that decides read from write, and the settings row that
//! shows the user what an approved program can do. A hand-written copy in the
//! settings row would go stale the first time a tool is added, and the user
//! would be reading a promise the server no longer keeps. So the names live
//! here, in the crate all three depend on, and a test in `writ-mcp` asserts
//! the registered set is this one.
//!
//! Pure: naming the tools is policy. Answering a call is `writ-mcp`'s half.
//!
//! What is absent is part of the record. No tool deletes, trashes or moves a
//! note (ADR-031 rule 4.6, ADR-032 section 6). `rename_note` is the closest
//! thing to a move that exists and it stays inside the notes folder.

/// The tools that only read, in the order `tools/list` reports them.
pub const READ_TOOLS: &[&str] = &[
    "list_notes",
    "search_notes",
    "read_note",
    "note_links",
    "note_backlinks",
    "note_properties",
    "note_tags",
    "folder_tags",
];

/// The tools that change a note, in the order `tools/list` reports them.
pub const WRITE_TOOLS: &[&str] = &["write_note", "create_note", "rename_note"];

/// Every tool a program can call, reads first.
pub fn tool_names() -> Vec<&'static str> {
    READ_TOOLS
        .iter()
        .chain(WRITE_TOOLS.iter())
        .copied()
        .collect()
}

/// Whether `tool` changes a note.
pub fn is_write_tool(tool: &str) -> bool {
    WRITE_TOOLS.contains(&tool)
}

/// Whether `tool` only reads.
pub fn is_read_tool(tool: &str) -> bool {
    READ_TOOLS.contains(&tool)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The verbs no tool is allowed to carry, read as a word inside a name:
    /// `delete_note` and `note_delete` are both caught.
    const ABSENT_VERBS: &[&str] = &["delete", "trash", "move", "remove", "destroy"];

    #[test]
    fn no_tool_deletes_trashes_or_moves_a_note() {
        for tool in tool_names() {
            for verb in ABSENT_VERBS {
                assert!(
                    !tool.contains(verb),
                    "{tool} carries {verb}, which no tool may do (ADR-031 rule 4.6)"
                );
            }
        }
    }

    #[test]
    fn the_three_write_tools_are_the_whole_write_half() {
        assert_eq!(WRITE_TOOLS, &["write_note", "create_note", "rename_note"]);
    }

    #[test]
    fn every_tool_is_named_once() {
        let names = tool_names();
        let mut seen = names.clone();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), names.len(), "a tool is named twice: {names:?}");
    }

    #[test]
    fn a_tool_is_a_read_or_a_write_and_never_both() {
        for tool in tool_names() {
            assert_ne!(
                is_read_tool(tool),
                is_write_tool(tool),
                "{tool} is on both halves of the list"
            );
        }
    }

    #[test]
    fn a_name_no_tool_carries_is_neither_half() {
        assert!(!is_read_tool("delete_note"));
        assert!(!is_write_tool("delete_note"));
    }
}
