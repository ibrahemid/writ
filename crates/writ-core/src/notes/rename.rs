//! Rewriting the links that name a note being renamed.
//!
//! A rename moves one file, and every note that pointed at it by its old name
//! now points at nothing. This is the pure half of putting that right: given
//! one note's text, it hands back the same text with the links that named the
//! renamed note naming it by its new name, and everything else — the alias,
//! the heading, the folder the link was written with, the label of a markdown
//! link, the spacing around all of it — exactly as its author left it.
//!
//! Which links count is [`links::resolve`]'s answer, not a second rule: a link
//! that carries a folder has to match the renamed note's folders, so
//! `[[archive/Note]]` is left alone when the note being renamed is not in an
//! `archive`. A link that names two notes at once resolves to neither and is
//! never touched here; the caller keeps those out by asking the index which
//! links resolved (ADR-034).

use std::ops::Range;

use crate::notes::links::{self, Resolution, WikilinkTarget};

/// `text` with every link that names `old` naming `new_name` instead, or
/// `None` when nothing in it does.
///
/// `old` is the renamed note as a link would name it: its name without a note
/// extension, and the folder it sits in when the caller has one to give. Only
/// the name part of each link is replaced, so `[[ideas/Old note#Later|see]]`
/// becomes `[[ideas/New note#Later|see]]` and `[a](ideas/Old%20note.md)`
/// becomes `[a](ideas/New%20note.md)`.
///
/// Pure: it reads nothing and writes nothing. A caller decides which files to
/// run it over and how the result reaches the disk.
pub fn rewrite_links(text: &str, old: &WikilinkTarget, new_name: &str) -> Option<String> {
    let new_name = new_name.trim();
    if new_name.is_empty() || old.name.trim().is_empty() {
        return None;
    }
    // The renamed note as a resolution candidate. One candidate, so the
    // ordering `resolve` applies among several never comes into it and the
    // note the link is written in has no bearing on the answer.
    let candidates = [match &old.folder {
        Some(folder) => format!("{folder}/{}", old.name),
        None => old.name.clone(),
    }];

    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    for link in links::scan(text) {
        let target = link.wikilink_target();
        if !matches!(
            links::resolve(&target, "", &candidates),
            Resolution::Resolved(_)
        ) {
            continue;
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
        return None;
    }
    // Back to front, so an earlier edit cannot move the range a later one was
    // measured against.
    let mut out = text.to_string();
    for (range, replacement) in edits.into_iter().rev() {
        out.replace_range(range, &replacement);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(name: &str) -> WikilinkTarget {
        WikilinkTarget {
            name: name.to_string(),
            ..WikilinkTarget::default()
        }
    }

    fn in_folder(folder: &str, name: &str) -> WikilinkTarget {
        WikilinkTarget {
            name: name.to_string(),
            folder: Some(folder.to_string()),
            ..WikilinkTarget::default()
        }
    }

    #[test]
    fn a_plain_wikilink_takes_the_new_name() {
        let out = rewrite_links("see [[Old note]] for more", &target("Old note"), "New note");
        assert_eq!(out.as_deref(), Some("see [[New note]] for more"));
    }

    #[test]
    fn an_alias_survives_the_rewrite() {
        let out = rewrite_links("[[Old note|what I meant]]", &target("Old note"), "New note");
        assert_eq!(out.as_deref(), Some("[[New note|what I meant]]"));
    }

    #[test]
    fn a_heading_survives_the_rewrite() {
        let out = rewrite_links("[[Old note#Later on]]", &target("Old note"), "New note");
        assert_eq!(out.as_deref(), Some("[[New note#Later on]]"));
    }

    #[test]
    fn a_heading_and_an_alias_together_survive_the_rewrite() {
        let out = rewrite_links(
            "[[Old note#Later on|see this]]",
            &target("Old note"),
            "New note",
        );
        assert_eq!(out.as_deref(), Some("[[New note#Later on|see this]]"));
    }

    #[test]
    fn a_folder_prefix_survives_the_rewrite() {
        let out = rewrite_links(
            "[[ideas/Old note]]",
            &in_folder("ideas", "Old note"),
            "New note",
        );
        assert_eq!(out.as_deref(), Some("[[ideas/New note]]"));
    }

    #[test]
    fn a_spelled_out_extension_survives_the_rewrite() {
        let out = rewrite_links("[[Old note.md]]", &target("Old note"), "New note");
        assert_eq!(out.as_deref(), Some("[[New note.md]]"));
    }

    #[test]
    fn a_markdown_link_is_rewritten_and_keeps_its_label() {
        let out = rewrite_links(
            "[what I wrote](ideas/Old%20note.md)",
            &in_folder("ideas", "Old note"),
            "New note",
        );
        assert_eq!(out.as_deref(), Some("[what I wrote](ideas/New%20note.md)"));
    }

    #[test]
    fn a_markdown_link_keeps_its_heading_and_its_title() {
        let out = rewrite_links("[a](Old.md#later \"Title\")", &target("Old"), "New");
        assert_eq!(out.as_deref(), Some("[a](New.md#later \"Title\")"));
    }

    #[test]
    fn a_bracketed_markdown_destination_stays_bracketed() {
        let out = rewrite_links("[a](<Old note.md>)", &target("Old note"), "New note");
        assert_eq!(out.as_deref(), Some("[a](<New note.md>)"));
    }

    #[test]
    fn a_link_in_a_folder_the_note_is_not_in_is_left_alone() {
        let out = rewrite_links("[[archive/Old note]]", &target("Old note"), "New note");
        assert_eq!(out, None);
    }

    #[test]
    fn a_link_to_another_note_is_left_alone() {
        let out = rewrite_links("[[Something else]]", &target("Old note"), "New note");
        assert_eq!(out, None);
    }

    #[test]
    fn a_link_inside_code_is_left_alone() {
        let text = "```\n[[Old note]]\n```\nand `[[Old note]]` too";
        assert_eq!(rewrite_links(text, &target("Old note"), "New note"), None);
    }

    #[test]
    fn a_name_differing_only_in_case_still_matches() {
        let out = rewrite_links("[[old NOTE]]", &target("Old note"), "New note");
        assert_eq!(out.as_deref(), Some("[[New note]]"));
    }

    #[test]
    fn several_links_on_one_line_are_all_rewritten() {
        let out = rewrite_links(
            "[[Old]] then [[Old|again]] then [[Old#top]]",
            &target("Old"),
            "New",
        );
        assert_eq!(
            out.as_deref(),
            Some("[[New]] then [[New|again]] then [[New#top]]")
        );
    }

    #[test]
    fn text_that_already_names_the_new_name_is_unchanged() {
        assert_eq!(rewrite_links("[[New]]", &target("New"), "New"), None);
    }

    #[test]
    fn an_empty_new_name_rewrites_nothing() {
        assert_eq!(rewrite_links("[[Old]]", &target("Old"), "  "), None);
    }

    #[test]
    fn a_bare_markdown_destination_percent_encodes_a_space() {
        let out = rewrite_links("[a](Old.md)", &target("Old"), "New note");
        assert_eq!(out.as_deref(), Some("[a](New%20note.md)"));
    }

    #[test]
    fn the_reverse_rewrite_restores_the_text_it_started_from() {
        let before = "[[ideas/Old note#Later|see]] and [a](ideas/Old%20note.md)";
        let after = rewrite_links(before, &in_folder("ideas", "Old note"), "New note")
            .expect("the links name the note");
        let back = rewrite_links(&after, &in_folder("ideas", "New note"), "Old note")
            .expect("the links name the note under its new name");
        assert_eq!(back, before);
    }
}
