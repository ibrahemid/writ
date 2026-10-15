//! The file open in Writ, as a program allowed to see it is answered
//! (ADR-053).
//!
//! The app writes an [`OpenContext`] for one `get_context` call and `writ mcp`
//! reads it back, so the two sides share one shape. Pure: the file it travels
//! in is `writ_storage::open_context`'s, and the grant that decides whether it
//! is asked for at all is `[mcp] approved_clients`.

use serde::{Deserialize, Serialize};

use crate::notes::facts::headings;
use crate::notes::links::heading_slug;

/// Most selections one answer carries.
pub const MAX_SELECTIONS: usize = 100;

/// Most bytes of selected text one answer carries: the ceiling a tool reads a
/// whole file to.
pub const MAX_SELECTED_BYTES: usize = crate::notes::host::MAX_NOTE_BYTES as usize;

/// A place in a file: the 1-based line and the 1-based column, counted in
/// characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextPoint {
    /// 1-based line.
    pub line: u32,
    /// 1-based column, in characters.
    pub column: u32,
}

/// One selected range, with its text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selected {
    /// The text as the window holds it.
    pub text: String,
    /// Where it starts.
    pub from: TextPoint,
    /// Where it ends.
    pub to: TextPoint,
}

/// The file in front in Writ's window and what is selected in it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenContext {
    /// The file, as the app knows it. `None` when Writ is not running, its
    /// window is not ready, or no tab holding a file is in front.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// The non-empty selections, in the window's order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub selections: Vec<Selected>,
    /// Where the caret is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<TextPoint>,
    /// The tab holds text the file on disk does not have yet.
    #[serde(default)]
    pub unsaved: bool,
    /// Selections or text past [`MAX_SELECTIONS`] or [`MAX_SELECTED_BYTES`]
    /// were left out.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub truncated: bool,
}

impl OpenContext {
    /// The answer when no file is open.
    pub fn nothing_open() -> Self {
        Self::default()
    }

    /// The answer held to [`MAX_SELECTIONS`] and [`MAX_SELECTED_BYTES`].
    ///
    /// Empty selections are dropped. A selection that would pass the byte
    /// ceiling is cut at the last character that fits, its end moved to
    /// match, and the ones after it are dropped; `truncated` says so.
    pub fn fitted(mut self) -> Self {
        let mut kept = Vec::new();
        let mut budget = MAX_SELECTED_BYTES;
        let mut cut = false;
        for selected in std::mem::take(&mut self.selections) {
            if selected.text.is_empty() {
                continue;
            }
            if kept.len() == MAX_SELECTIONS || budget == 0 {
                cut = true;
                break;
            }
            if selected.text.len() <= budget {
                budget -= selected.text.len();
                kept.push(selected);
                continue;
            }
            let mut end = budget;
            while !selected.text.is_char_boundary(end) {
                end -= 1;
            }
            cut = true;
            if end > 0 {
                let text = selected.text[..end].to_string();
                let to = point_after(selected.from, &text);
                kept.push(Selected {
                    text,
                    from: selected.from,
                    to,
                });
            }
            break;
        }
        self.selections = kept;
        self.truncated |= cut;
        self
    }
}

/// Where `text` ends when it starts at `from`.
fn point_after(from: TextPoint, text: &str) -> TextPoint {
    match text.rsplit_once('\n') {
        Some((before, last)) => TextPoint {
            line: from.line.saturating_add(
                u32::try_from(before.matches('\n').count() + 1).unwrap_or(u32::MAX),
            ),
            column: u32::try_from(last.chars().count() + 1).unwrap_or(u32::MAX),
        },
        None => TextPoint {
            line: from.line,
            column: from
                .column
                .saturating_add(u32::try_from(text.chars().count()).unwrap_or(u32::MAX)),
        },
    }
}

/// The 1-based line of the first heading in `text` that `heading` names,
/// written with or without its `#` markers.
///
/// Compared as `[[Note#Heading]]` compares them (ADR-034): by the anchor, so
/// case and punctuation do not matter, and `Plan-1` names the second `Plan`.
/// A heading with no letters or digits is compared as written.
pub fn heading_line(text: &str, heading: &str) -> Option<u32> {
    let wanted = heading.trim().trim_start_matches('#').trim();
    if wanted.is_empty() {
        return None;
    }
    let slug = heading_slug(wanted);
    headings(text)
        .into_iter()
        .find(|found| {
            if slug.is_empty() {
                found.text == wanted
            } else {
                found.slug == slug || heading_slug(&found.text) == slug
            }
        })
        .map(|found| found.line)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(line: u32, column: u32) -> TextPoint {
        TextPoint { line, column }
    }

    fn selected(text: &str, from: TextPoint) -> Selected {
        Selected {
            text: text.to_string(),
            from,
            to: point_after(from, text),
        }
    }

    #[test]
    fn the_end_of_a_selection_follows_its_lines_and_characters() {
        assert_eq!(point_after(at(3, 5), "abc"), at(3, 8));
        assert_eq!(point_after(at(3, 5), "ab\ncdé"), at(4, 4));
        assert_eq!(point_after(at(1, 1), "a\n\nb"), at(3, 2));
        assert_eq!(point_after(at(2, 2), "a\n"), at(3, 1));
    }

    #[test]
    fn an_answer_within_the_ceilings_is_kept_as_it_was_less_its_empty_ranges() {
        let context = OpenContext {
            path: Some("/notes/Plan.md".to_string()),
            selections: vec![selected("", at(1, 1)), selected("launch", at(2, 1))],
            cursor: Some(at(2, 7)),
            unsaved: true,
            truncated: false,
        };
        let fitted = context.clone().fitted();
        assert_eq!(fitted.selections, vec![selected("launch", at(2, 1))]);
        assert!(!fitted.truncated);
        assert!(fitted.unsaved);
    }

    #[test]
    fn too_many_selections_are_cut_and_said_to_be() {
        let context = OpenContext {
            selections: (1..=(MAX_SELECTIONS as u32 + 5))
                .map(|line| selected("x", at(line, 1)))
                .collect(),
            ..OpenContext::default()
        };
        let fitted = context.fitted();
        assert_eq!(fitted.selections.len(), MAX_SELECTIONS);
        assert!(fitted.truncated);
    }

    #[test]
    fn text_past_the_ceiling_is_cut_on_a_character_and_its_end_moved() {
        let long = format!("{}é tail", "a".repeat(MAX_SELECTED_BYTES - 1));
        let context = OpenContext {
            selections: vec![selected(&long, at(1, 1)), selected("after", at(9, 1))],
            ..OpenContext::default()
        };
        let fitted = context.fitted();
        assert_eq!(fitted.selections.len(), 1);
        let kept = &fitted.selections[0];
        assert_eq!(kept.text.len(), MAX_SELECTED_BYTES - 1);
        assert_eq!(kept.to, at(1, MAX_SELECTED_BYTES as u32));
        assert!(fitted.truncated);
    }

    #[test]
    fn an_answer_reads_back_as_it_was_written() {
        let context = OpenContext {
            path: Some("/notes/Plan.md".to_string()),
            selections: vec![selected("one\ntwo", at(4, 3))],
            cursor: Some(at(5, 4)),
            unsaved: false,
            truncated: true,
        };
        let text = serde_json::to_string(&context).expect("write");
        assert_eq!(
            serde_json::from_str::<OpenContext>(&text).expect("read"),
            context
        );
        let empty = serde_json::to_string(&OpenContext::nothing_open()).expect("write");
        assert_eq!(empty, r#"{"unsaved":false}"#);
    }

    #[test]
    fn a_heading_is_found_by_its_text_or_its_anchor() {
        let text = "---\ntitle: x\n---\n# Plan\nintro\n## Next steps!\nwork\n## Plan\n";
        assert_eq!(heading_line(text, "Plan"), Some(4));
        assert_eq!(heading_line(text, "## next steps"), Some(6));
        assert_eq!(heading_line(text, "next-steps"), Some(6));
        assert_eq!(heading_line(text, "plan-1"), Some(8));
        assert_eq!(heading_line(text, "Missing"), None);
        assert_eq!(heading_line(text, "  # "), None);
    }
}
