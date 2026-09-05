//! Rewriting the links that name a note being renamed.
//!
//! A rename moves one file, and every note that pointed at it by its old name
//! now points at nothing. This is the pure half of putting that right: given
//! one note's text, it hands back the same text with the links that named the
//! renamed note naming it by its new name, and everything else — the alias,
//! the heading, the folder the link was written with, the label of a markdown
//! link, the spacing around all of it — exactly as its author left it.
//!
//! Which links count is [`links::resolve`]'s answer, link by link, from the
//! path of the note the link is written in. Two notes can share a name, so
//! whether `[[Note]]` means the note being renamed is a question about the
//! whole notes folder and about where the link is written, and nothing else
//! can answer it: a link that reaches a different note of the same name is
//! left alone, and a link that names two notes at once resolves to neither and
//! is left alone too (ADR-034).
//!
//! A note the candidate list does not hold gets the same answer the other way
//! round: `unindexed` names the files a link could reach that the list has not
//! heard of, and a link one of those answers to stops the rewrite of the whole
//! file. A rename cannot ask a person about a note nothing has indexed, and
//! rewriting past it is how a link ends up pointing somewhere it never did.

use std::ops::Range;

use crate::notes::links::{self, Resolution};

/// What [`rewrite_links`] did with one note's text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rewrite {
    /// The text, with every link that named the renamed note naming it by its
    /// new name.
    Rewritten(String),
    /// No link in the text reaches the renamed note.
    NoLink,
    /// A link reaches the renamed note, and a file outside the candidate list
    /// answers to the name it writes. Nothing was rewritten: which note the
    /// link means is a question this cannot answer and must not guess at.
    NameNotUnique,
}

/// `text` with every link that resolves to `target` naming `new_name` instead.
///
/// `from` is the path of the note holding `text` and `target` the path of the
/// note being renamed, both spelled the way `candidates` spells them —
/// `candidates` being every note a link could reach. The three are what
/// [`links::resolve`] needs, so the answer here and the answer the backlink
/// list gives are the same answer. `unindexed` is every file a link could
/// reach that `candidates` leaves out; a link one of them answers to makes the
/// whole file [`Rewrite::NameNotUnique`].
///
/// Only the name part of each link is replaced, so `[[ideas/Old note#Later|see]]`
/// becomes `[[ideas/New note#Later|see]]` and `[a](ideas/Old%20note.md)`
/// becomes `[a](ideas/New%20note.md)`. A link whose name is spelled in another
/// case, or in another unicode normalisation, matches and is rewritten in the
/// new name's own spelling: the match is folded ([`links::name_key`]) and the
/// replacement is not, so a rewrite and its reverse restore a link's target
/// but not necessarily its original spelling.
///
/// Pure: it reads nothing and writes nothing. A caller decides which files to
/// run it over and how the result reaches the disk.
pub fn rewrite_links(
    text: &str,
    from: &str,
    target: &str,
    new_name: &str,
    candidates: &[String],
    unindexed: &[String],
) -> Rewrite {
    let new_name = new_name.trim();
    if new_name.is_empty() || target.is_empty() {
        return Rewrite::NoLink;
    }

    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    for link in links::scan(text) {
        let written = link.wikilink_target();
        match links::resolve(&written, from, candidates) {
            Resolution::Resolved(path) if path == target => {}
            _ => continue,
        }
        // The link reaches the renamed note as far as the index knows. A file
        // the index has not seen answering to the same name is the case the
        // index cannot rule out, and it stops the file rather than this link:
        // half a rewritten file is worse than none, because the half that
        // landed is the half nobody was told about.
        if !matches!(
            links::resolve(&written, from, unindexed),
            Resolution::Missing
        ) {
            return Rewrite::NameNotUnique;
        }
        let slice = &text[link.byte_range.clone()];
        let Some(span) = links::name_span(slice) else {
            continue;
        };
        let range =
            link.byte_range.start + span.range.start..link.byte_range.start + span.range.end;
        let replacement = links::escape_name(new_name, span.escaping);
        if text[range.clone()] == replacement {
            continue;
        }
        edits.push((range, replacement));
    }

    if edits.is_empty() {
        return Rewrite::NoLink;
    }
    // Back to front, so an earlier edit cannot move the range a later one was
    // measured against.
    let mut out = text.to_string();
    for (range, replacement) in edits.into_iter().rev() {
        out.replace_range(range, &replacement);
    }
    Rewrite::Rewritten(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const READER: &str = "/notes/Reader.md";

    fn notes(paths: &[&str]) -> Vec<String> {
        paths.iter().map(|p| p.to_string()).collect()
    }

    /// The rewrite as `Reader.md` sees it, in a folder holding it and the note
    /// being renamed, every note of it indexed.
    fn rewrite(text: &str, target: &str, new_name: &str) -> Option<String> {
        match rewrite_links(
            text,
            READER,
            target,
            new_name,
            &notes(&[READER, target]),
            &[],
        ) {
            Rewrite::Rewritten(out) => Some(out),
            Rewrite::NoLink => None,
            Rewrite::NameNotUnique => panic!("nothing is outside the candidate list here"),
        }
    }

    #[test]
    fn a_plain_wikilink_takes_the_new_name() {
        let out = rewrite(
            "see [[Old note]] for more",
            "/notes/Old note.md",
            "New note",
        );
        assert_eq!(out.as_deref(), Some("see [[New note]] for more"));
    }

    #[test]
    fn an_alias_survives_the_rewrite() {
        let out = rewrite(
            "[[Old note|what I meant]]",
            "/notes/Old note.md",
            "New note",
        );
        assert_eq!(out.as_deref(), Some("[[New note|what I meant]]"));
    }

    #[test]
    fn a_heading_survives_the_rewrite() {
        let out = rewrite("[[Old note#Later on]]", "/notes/Old note.md", "New note");
        assert_eq!(out.as_deref(), Some("[[New note#Later on]]"));
    }

    #[test]
    fn a_heading_and_an_alias_together_survive_the_rewrite() {
        let out = rewrite(
            "[[Old note#Later on|see this]]",
            "/notes/Old note.md",
            "New note",
        );
        assert_eq!(out.as_deref(), Some("[[New note#Later on|see this]]"));
    }

    #[test]
    fn a_folder_prefix_survives_the_rewrite() {
        let out = rewrite("[[ideas/Old note]]", "/notes/ideas/Old note.md", "New note");
        assert_eq!(out.as_deref(), Some("[[ideas/New note]]"));
    }

    #[test]
    fn a_spelled_out_extension_survives_the_rewrite() {
        let out = rewrite("[[Old note.md]]", "/notes/Old note.md", "New note");
        assert_eq!(out.as_deref(), Some("[[New note.md]]"));
    }

    #[test]
    fn a_markdown_link_is_rewritten_and_keeps_its_label() {
        let out = rewrite(
            "[what I wrote](ideas/Old%20note.md)",
            "/notes/ideas/Old note.md",
            "New note",
        );
        assert_eq!(out.as_deref(), Some("[what I wrote](ideas/New%20note.md)"));
    }

    #[test]
    fn a_markdown_link_keeps_its_heading_and_its_title() {
        let out = rewrite("[a](Old.md#later \"Title\")", "/notes/Old.md", "New");
        assert_eq!(out.as_deref(), Some("[a](New.md#later \"Title\")"));
    }

    #[test]
    fn a_bracketed_markdown_destination_stays_bracketed() {
        let out = rewrite("[a](<Old note.md>)", "/notes/Old note.md", "New note");
        assert_eq!(out.as_deref(), Some("[a](<New note.md>)"));
    }

    #[test]
    fn a_link_in_a_folder_the_note_is_not_in_is_left_alone() {
        assert_eq!(
            rewrite("[[archive/Old note]]", "/notes/Old note.md", "New note"),
            None
        );
    }

    #[test]
    fn a_link_to_another_note_is_left_alone() {
        assert_eq!(
            rewrite("[[Something else]]", "/notes/Old note.md", "New note"),
            None
        );
    }

    #[test]
    fn a_link_inside_code_is_left_alone() {
        let text = "```\n[[Old note]]\n```\nand `[[Old note]]` too";
        assert_eq!(rewrite(text, "/notes/Old note.md", "New note"), None);
    }

    #[test]
    fn a_name_differing_only_in_case_still_matches() {
        let out = rewrite("[[old NOTE]]", "/notes/Old note.md", "New note");
        assert_eq!(out.as_deref(), Some("[[New note]]"));
    }

    #[test]
    fn several_links_on_one_line_are_all_rewritten() {
        let out = rewrite(
            "[[Old]] then [[Old|again]] then [[Old#top]]",
            "/notes/Old.md",
            "New",
        );
        assert_eq!(
            out.as_deref(),
            Some("[[New]] then [[New|again]] then [[New#top]]")
        );
    }

    #[test]
    fn text_that_already_names_the_new_name_is_unchanged() {
        assert_eq!(rewrite("[[New]]", "/notes/New.md", "New"), None);
    }

    #[test]
    fn an_empty_new_name_rewrites_nothing() {
        assert_eq!(rewrite("[[Old]]", "/notes/Old.md", "  "), None);
    }

    #[test]
    fn a_bare_markdown_destination_percent_encodes_a_space() {
        let out = rewrite("[a](Old.md)", "/notes/Old.md", "New note");
        assert_eq!(out.as_deref(), Some("[a](New%20note.md)"));
    }

    /// The case the folder prefix alone cannot decide: one file holds a link
    /// written with the folder and a bare one that reaches the *other* note of
    /// that name. Only the first is the renamed note.
    #[test]
    fn a_bare_link_reaching_another_note_of_the_same_name_is_left_alone() {
        let all = notes(&[
            "/notes/one/Note.md",
            "/notes/two/Note.md",
            "/notes/two/Reader.md",
        ]);
        let out = rewrite_links(
            "see [[one/Note]] and [[Note]]\n",
            "/notes/two/Reader.md",
            "/notes/one/Note.md",
            "Renamed",
            &all,
            &[],
        );
        assert_eq!(
            out,
            Rewrite::Rewritten("see [[one/Renamed]] and [[Note]]\n".to_string())
        );
    }

    /// A bare link the ordering cannot separate names two notes at once. It is
    /// left as it is: picking one is what rewrites the wrong file (ADR-034).
    #[test]
    fn a_link_that_names_two_notes_at_once_is_left_alone() {
        let all = notes(&["/notes/one/Note.md", "/notes/two/Note.md", READER]);
        let out = rewrite_links(
            "see [[Note]]",
            READER,
            "/notes/one/Note.md",
            "Renamed",
            &all,
            &[],
        );
        assert_eq!(out, Rewrite::NoLink);
    }

    /// A link resolves after the round trip, but not in the spelling its
    /// author used: matching folds case and unicode normalisation and the
    /// replacement is the name's own spelling, so the reverse rewrite restores
    /// the target, not the typing.
    #[test]
    fn a_link_spelled_in_another_case_comes_back_in_the_file_s_spelling() {
        let after = rewrite("[[old NOTE]]", "/notes/Old note.md", "New note")
            .expect("the link names the note");
        assert_eq!(after, "[[New note]]");
        let back = rewrite(&after, "/notes/New note.md", "Old note")
            .expect("the link names the note under its new name");
        assert_eq!(back, "[[Old note]]");
    }

    #[test]
    fn the_reverse_rewrite_restores_the_text_it_started_from() {
        let before = "[[ideas/Old note#Later|see]] and [a](ideas/Old%20note.md)";
        let after = rewrite(before, "/notes/ideas/Old note.md", "New note")
            .expect("the links name the note");
        let back = rewrite(&after, "/notes/ideas/New note.md", "Old note")
            .expect("the links name the note under its new name");
        assert_eq!(back, before);
    }

    /// The note the index has not heard of: on disk, sharing the name, and
    /// therefore the note a bare link might mean. Nothing is rewritten and the
    /// caller is told which of the two it is.
    #[test]
    fn a_name_a_file_outside_the_candidate_list_answers_to_stops_the_file() {
        let all = notes(&["/notes/one/Note.md", "/notes/two/Reader.md"]);
        let unindexed = notes(&["/notes/two/Note.md"]);

        let out = rewrite_links(
            "see [[one/Note]] and [[Note]]\n",
            "/notes/two/Reader.md",
            "/notes/one/Note.md",
            "Renamed",
            &all,
            &unindexed,
        );

        assert_eq!(out, Rewrite::NameNotUnique);
    }

    /// The same unknown note, and a link that spells the folder. The folder
    /// says which note it means, so the unknown one is not a candidate for it.
    #[test]
    fn a_folder_says_which_note_a_link_means_even_against_an_unknown_one() {
        let all = notes(&["/notes/one/Note.md", "/notes/two/Reader.md"]);
        let unindexed = notes(&["/notes/two/Note.md"]);

        let out = rewrite_links(
            "see [[one/Note]]\n",
            "/notes/two/Reader.md",
            "/notes/one/Note.md",
            "Renamed",
            &all,
            &unindexed,
        );

        assert_eq!(out, Rewrite::Rewritten("see [[one/Renamed]]\n".to_string()));
    }

    /// A file outside the list that no link in this text could reach changes
    /// nothing.
    #[test]
    fn a_file_outside_the_candidate_list_under_another_name_changes_nothing() {
        let all = notes(&[READER, "/notes/Old note.md"]);
        let unindexed = notes(&["/notes/two/Something else.md"]);

        let out = rewrite_links(
            "see [[Old note]]",
            READER,
            "/notes/Old note.md",
            "New note",
            &all,
            &unindexed,
        );

        assert_eq!(out, Rewrite::Rewritten("see [[New note]]".to_string()));
    }
}
