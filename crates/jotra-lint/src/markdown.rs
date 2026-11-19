//! Finds the prose in a Markdown document.
//!
//! The parse uses the same options and the same set of text containers as the
//! Harper engine this crate replaced: text in paragraphs, headings, list items,
//! table cells, emphasis, strong, strikethrough and link text is prose. Code,
//! HTML, math, front matter, image alt text, autolinks, email links and the
//! target of an unpiped wikilink are not.

use std::ops::Range;

use pulldown_cmark::{Event, LinkType, Options, Parser, Tag};

/// What an open Markdown element means for the text inside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Frame {
    /// Text directly inside is prose.
    Prose,
    /// Nothing inside is prose, however deep.
    Hidden,
    /// A container whose direct text is not prose (lists, tables, quotes).
    Other,
}

fn frame_for(tag: &Tag<'_>) -> Frame {
    match tag {
        Tag::Paragraph
        | Tag::Heading { .. }
        | Tag::Item
        | Tag::TableCell
        | Tag::Emphasis
        | Tag::Strong
        | Tag::Strikethrough => Frame::Prose,
        Tag::Link { link_type, .. } => match link_type {
            LinkType::Autolink | LinkType::Email | LinkType::WikiLink { has_pothole: false } => {
                Frame::Hidden
            }
            _ => Frame::Prose,
        },
        Tag::CodeBlock(_) | Tag::HtmlBlock | Tag::MetadataBlock(_) | Tag::Image { .. } => {
            Frame::Hidden
        }
        _ => Frame::Other,
    }
}

/// Byte ranges of `text` that hold prose, in document order.
///
/// Adjacent text events (including soft and hard line breaks) merge into one
/// range, so two words on either side of a line break inside a paragraph sit in
/// the same range. Any other event ends the range: an inline code span between
/// two words keeps them apart.
pub(crate) fn prose_runs(text: &str) -> Vec<Range<usize>> {
    let options = Options::all().difference(Options::ENABLE_SMART_PUNCTUATION);
    let mut frames: Vec<Frame> = Vec::new();
    let mut hidden_depth = 0usize;
    let mut runs: Vec<Range<usize>> = Vec::new();
    let mut current: Option<Range<usize>> = None;

    for (event, range) in Parser::new_ext(text, options).into_offset_iter() {
        match event {
            Event::Text(_) | Event::SoftBreak | Event::HardBreak
                if hidden_depth == 0 && frames.last() == Some(&Frame::Prose) =>
            {
                match current.as_mut() {
                    Some(run) if run.end == range.start => run.end = range.end,
                    _ => {
                        if let Some(run) = current.take() {
                            runs.push(run);
                        }
                        current = Some(range);
                    }
                }
            }
            Event::Start(tag) => {
                if let Some(run) = current.take() {
                    runs.push(run);
                }
                let frame = frame_for(&tag);
                if frame == Frame::Hidden {
                    hidden_depth += 1;
                }
                frames.push(frame);
            }
            Event::End(_) => {
                if let Some(run) = current.take() {
                    runs.push(run);
                }
                if frames.pop() == Some(Frame::Hidden) {
                    hidden_depth = hidden_depth.saturating_sub(1);
                }
            }
            _ => {
                if let Some(run) = current.take() {
                    runs.push(run);
                }
            }
        }
    }
    if let Some(run) = current.take() {
        runs.push(run);
    }
    runs.retain(|run| run.start < run.end && run.end <= text.len());
    runs
}
