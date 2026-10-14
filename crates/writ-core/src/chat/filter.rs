//! What the pane may show of a reply while it arrives.

use super::*;

/// Where the filter is in the line it is reading.
enum FilterState {
    /// Ordinary text, released as it arrives.
    Text {
        /// True when the next character opens a line.
        at_line_start: bool,
    },
    /// A line's leading whitespace and fence characters, held back while they
    /// are still the start of a line that could open a proposal.
    MaybeFence {
        /// What has been held back, verbatim.
        buffered: String,
    },
    /// A line that was nothing but a fence, held back until the line under it
    /// shows whether it is a header. This is the one line the filter's
    /// contract allows it to be late by.
    MaybeHeader {
        /// The fence line and its newline, verbatim.
        opening: String,
        /// The fence it was, which the block's close has to match.
        fence: Fence,
        /// What has been read of the line under it, verbatim.
        read: String,
    },
    /// Inside a proposal, where nothing is released.
    Withholding {
        /// The line being read, so the closing fence is recognised.
        line: String,
        /// The fence the block was opened with, which is what its close has
        /// to match.
        fence: Fence,
        /// The inner code fence the body has left open, which a closing line
        /// may belong to instead of the proposal.
        inner: Option<Fence>,
        /// True until the line that opened the block ends, because that line
        /// is the fence itself rather than a line of the body.
        opening: bool,
    },
    /// After a line that closed an open inner code fence and the proposal
    /// both. Which of the two it was is only decided by what the rest of the
    /// reply holds, so the rest is held back and read at the end.
    Deciding {
        /// The fence the proposal was opened with.
        fence: Fence,
        /// Everything read since the ambiguous line.
        buffered: String,
    },
}

/// Keeps a proposal out of the reply the user watches arrive.
///
/// [`parse_proposals`] reads the whole reply once it is complete; this reads
/// the same grammar one delta at a time, so a proposal never flashes in the
/// pane on its way to a card. The two agree by construction: a line opens a
/// block when [`open_proposal_at`] reads one out of it, and closes it when
/// [`closes_proposal`] says so, which is what [`parse_proposals`] asks of the
/// same lines. A bare fence is held until the line under it says whether it is
/// a header, and both lines are released when it is not.
///
/// Anything else is released as soon as it can no longer become a fence, so an
/// ordinary code block arrives byte for byte, one line late at most. A block
/// the reply never closed is never released: the pane shows the prose that came
/// before it and the card carries the rest.
pub struct ProposalFilter {
    state: FilterState,
}

impl ProposalFilter {
    /// A filter positioned at the start of a reply.
    pub fn new() -> Self {
        Self {
            state: FilterState::Text {
                at_line_start: true,
            },
        }
    }

    /// Reads one delta and returns what may be shown now.
    pub fn push(&mut self, delta: &str) -> String {
        let mut out = String::with_capacity(delta.len());
        for character in delta.chars() {
            self.read(character, &mut out);
        }
        out
    }

    /// Ends the reply, releasing a prefix that never became a fence.
    ///
    /// A proposal that was still open is dropped: a block cut off mid-write is
    /// not a whole note. A block whose end was ambiguous is read the way
    /// [`parse_proposals`] reads it: what the block turned out to hold stays
    /// withheld, and what fell outside it is released. When neither reading
    /// holds and the parser drops the block, what follows the ambiguous fence
    /// is read the way [`parse_proposals`] reads it: the blocks in it are
    /// withheld and everything between and after them is shown, so the pane
    /// and the cards account for the whole reply between them.
    pub fn finish(&mut self) -> String {
        match std::mem::replace(
            &mut self.state,
            FilterState::Text {
                at_line_start: true,
            },
        ) {
            FilterState::MaybeFence { buffered } => buffered,
            FilterState::MaybeHeader {
                mut opening, read, ..
            } => {
                opening.push_str(&read);
                opening
            }
            FilterState::Deciding { fence, buffered } => {
                let lines: Vec<&str> = buffered.lines().collect();
                match last_close(&lines, 0, fence) {
                    Some(close) => prose_in(&lines[close + 1..]),
                    None => prose_in(&lines),
                }
            }
            FilterState::Text { .. } | FilterState::Withholding { .. } => String::new(),
        }
    }

    /// Reads one character, pushing onto `out` whatever it releases.
    fn read(&mut self, character: char, out: &mut String) {
        let next = match &mut self.state {
            FilterState::Text { at_line_start } => {
                if *at_line_start && character != '\n' && is_fence_lead(character) {
                    Some(FilterState::MaybeFence {
                        buffered: character.to_string(),
                    })
                } else {
                    out.push(character);
                    *at_line_start = character == '\n';
                    None
                }
            }
            FilterState::MaybeFence { buffered } if character == '\n' => {
                match bare_fence(buffered) {
                    Some(fence) => {
                        let mut opening = std::mem::take(buffered);
                        opening.push('\n');
                        Some(FilterState::MaybeHeader {
                            opening,
                            fence,
                            read: String::new(),
                        })
                    }
                    None => {
                        out.push_str(buffered);
                        out.push('\n');
                        Some(FilterState::Text {
                            at_line_start: true,
                        })
                    }
                }
            }
            FilterState::MaybeHeader { opening, read, .. } if character == '\n' => {
                // No prefix of the line was a header, so neither line opened a
                // block. What was read cannot itself be a fence: every prefix
                // of it passed `could_be_header`, whose first character is the
                // word's.
                out.push_str(opening);
                out.push_str(read);
                out.push('\n');
                Some(FilterState::Text {
                    at_line_start: true,
                })
            }
            FilterState::MaybeHeader {
                opening,
                fence,
                read,
            } => {
                read.push(character);
                if header_info(read).is_some() {
                    Some(FilterState::Withholding {
                        line: std::mem::take(read),
                        fence: *fence,
                        inner: None,
                        opening: true,
                    })
                } else if could_be_header(read) {
                    None
                } else {
                    out.push_str(opening);
                    let read = std::mem::take(read);
                    match could_open_proposal(&read) {
                        true => Some(FilterState::MaybeFence { buffered: read }),
                        false => {
                            out.push_str(&read);
                            Some(FilterState::Text {
                                at_line_start: false,
                            })
                        }
                    }
                }
            }
            FilterState::MaybeFence { buffered } => {
                buffered.push(character);
                match open_proposal(buffered) {
                    Some((fence, _)) => Some(FilterState::Withholding {
                        line: std::mem::take(buffered),
                        fence,
                        inner: None,
                        opening: true,
                    }),
                    None if could_open_proposal(buffered) => None,
                    None => {
                        out.push_str(buffered);
                        Some(FilterState::Text {
                            at_line_start: false,
                        })
                    }
                }
            }
            FilterState::Withholding { line, .. } if character != '\n' => {
                line.push(character);
                None
            }
            FilterState::Withholding {
                line,
                fence,
                inner,
                opening,
            } => {
                if *opening {
                    *opening = false;
                    line.clear();
                    return;
                }
                match line_in_block(line, *fence, *inner) {
                    BlockLine::Closes => Some(FilterState::Text {
                        at_line_start: true,
                    }),
                    BlockLine::Ambiguous => Some(FilterState::Deciding {
                        fence: *fence,
                        buffered: String::new(),
                    }),
                    BlockLine::Body(next) => {
                        *inner = next;
                        line.clear();
                        None
                    }
                }
            }
            FilterState::Deciding { buffered, .. } => {
                buffered.push(character);
                None
            }
        };
        if let Some(state) = next {
            self.state = state;
        }
    }
}

impl Default for ProposalFilter {
    fn default() -> Self {
        Self::new()
    }
}

/// True for a character that can stand before a proposal's fence on its line.
fn is_fence_lead(character: char) -> bool {
    character == '`' || character == '~' || character.is_whitespace()
}
