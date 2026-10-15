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
///
/// `open_in_writ` and `reveal` change no file: they show one in the app,
/// through a `writ://` link (ADR-052, ADR-053), and are approved with reading.
pub const READ_TOOLS: &[&str] = &[
    "list_notes",
    "search_notes",
    "read_note",
    "note_links",
    "note_backlinks",
    "note_properties",
    "note_tags",
    "folder_tags",
    "open_in_writ",
    "reveal",
];

/// The tools that change a note, in the order `tools/list` reports them.
///
/// `today` is here because it makes the day's file when there is none.
pub const WRITE_TOOLS: &[&str] = &[
    "write_note",
    "create_note",
    "rename_note",
    "append_note",
    "replace_in_note",
    "today",
];

/// The tools that see the file open in Writ and its selection, approved by
/// their own grant (ADR-053): neither reading nor writing implies it.
pub const CONTEXT_TOOLS: &[&str] = &["get_context"];

/// Every tool a program can call: reads, then writes, then the open file.
pub fn tool_names() -> Vec<&'static str> {
    READ_TOOLS
        .iter()
        .chain(WRITE_TOOLS.iter())
        .chain(CONTEXT_TOOLS.iter())
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

/// Whether `tool` sees the file open in Writ.
pub fn is_context_tool(tool: &str) -> bool {
    CONTEXT_TOOLS.contains(&tool)
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
    fn the_six_write_tools_are_the_whole_write_half() {
        assert_eq!(
            WRITE_TOOLS,
            &[
                "write_note",
                "create_note",
                "rename_note",
                "append_note",
                "replace_in_note",
                "today"
            ]
        );
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
    fn a_tool_is_on_exactly_one_list() {
        for tool in tool_names() {
            let lists = [
                is_read_tool(tool),
                is_write_tool(tool),
                is_context_tool(tool),
            ];
            assert_eq!(
                lists.iter().filter(|on| **on).count(),
                1,
                "{tool} is on {lists:?}"
            );
        }
    }

    #[test]
    fn seeing_the_open_file_is_its_own_list() {
        assert_eq!(CONTEXT_TOOLS, &["get_context"]);
        assert!(!is_read_tool("get_context"));
        assert!(is_read_tool("reveal"));
    }

    #[test]
    fn a_name_no_tool_carries_is_on_no_list() {
        assert!(!is_read_tool("delete_note"));
        assert!(!is_write_tool("delete_note"));
        assert!(!is_context_tool("delete_note"));
    }
}
