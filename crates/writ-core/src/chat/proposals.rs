//! The changes a finished reply asks for, read out of its `writ-proposal` blocks.

use serde::{Deserialize, Serialize};

use super::*;

/// A change to one note the model asked for and nobody has applied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proposal {
    /// The attached note it changes.
    pub path: String,
    /// What that note held when it was attached.
    pub before_hash: String,
    /// The whole text the note would hold.
    pub new_content: String,
    /// The one line the model gave for it, empty when it gave none.
    pub summary: String,
    /// What the change looks like line by line, empty when the note is larger
    /// than [`crate::diff::MAX_DIFF_BYTES`] and the card shows the summary
    /// alone.
    pub hunks: Vec<crate::diff::Hunk>,
}

/// The info-string word that makes a fenced block a proposal.
const PROPOSAL_WORD: &str = "writ-proposal";

/// The attribute a header line has to carry, so a bare fence over a note that
/// happens to say `writ-proposal` stays an ordinary code block.
const PROPOSAL_PATH: &str = "path=";

/// The shortest run of fence characters that opens a block (CommonMark).
const MIN_FENCE: usize = 3;

/// The fence a block was opened with: which character it is made of, and how
/// many of them a closing fence has to match or beat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Fence {
    /// A backtick or a tilde. A block opened with one is not closed by the
    /// other.
    marker: char,
    /// How long the opening run was.
    len: usize,
}

/// Reads a line's leading fence run, and what follows it.
///
/// Up to three spaces of indentation are allowed before the run, which is the
/// CommonMark rule and what a local model writes when it indents a block
/// inside a list.
fn fence_run(line: &str) -> Option<(Fence, &str)> {
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let rest = &line[indent..];
    let marker = rest.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let len = rest
        .chars()
        .take_while(|character| *character == marker)
        .count();
    if len < MIN_FENCE {
        return None;
    }
    Some((Fence { marker, len }, &rest[len..]))
}

/// Reads the line that opens a proposal, and the info string after the word.
pub(super) fn open_proposal(line: &str) -> Option<(Fence, &str)> {
    let (fence, info) = fence_run(line)?;
    let info = info
        .trim_start_matches([' ', '\t'])
        .strip_prefix(PROPOSAL_WORD)?;
    Some((fence, info))
}

/// A line that is nothing but a fence run, and the fence it is.
pub(super) fn bare_fence(line: &str) -> Option<Fence> {
    let (fence, rest) = fence_run(line)?;
    rest.trim().is_empty().then_some(fence)
}

/// Reads a proposal's header line, and the attributes after the word.
///
/// A header is the line under a bare fence, carrying what an info string
/// carries. It has to name a path: a bare fence is an ordinary code block
/// until its first line does, so the word alone never turns a note whose first
/// line reads `writ-proposal` into a block nobody asked for.
pub(super) fn header_info(line: &str) -> Option<&str> {
    let info = line
        .trim_start_matches([' ', '\t'])
        .strip_prefix(PROPOSAL_WORD)?;
    info.trim_start_matches([' ', '\t'])
        .starts_with(PROPOSAL_PATH)
        .then_some(info)
}

/// What opens a proposal: the fence, the attributes, and where the body starts.
struct Opening<'a> {
    /// The fence the block was opened with, which its close has to match.
    fence: Fence,
    /// The attributes, as an info string would have carried them.
    info: &'a str,
    /// The first line of the body.
    body: usize,
}

/// Reads the proposal `lines[index]` opens, in either form.
///
/// A fence whose info string is the word opens one; so does a bare fence whose
/// next line is a [`header_info`] line, which is what a local model writes
/// when it puts the fence on a line of its own. The header is part of the
/// opening, not the first line of the body, and the block closes by the same
/// rule either way.
fn open_proposal_at<'a>(lines: &[&'a str], index: usize) -> Option<Opening<'a>> {
    if let Some((fence, info)) = open_proposal(lines[index]) {
        return Some(Opening {
            fence,
            info,
            body: index + 1,
        });
    }
    let fence = bare_fence(lines[index])?;
    let info = header_info(lines.get(index + 1)?)?;
    Some(Opening {
        fence,
        info,
        body: index + 2,
    })
}

/// True when `line` closes a block opened with `fence`.
///
/// The CommonMark rule: the same character, at least as long, and nothing but
/// whitespace after it. A three-backtick line inside a four-backtick block is
/// therefore body text, which is what keeps a proposal for a note holding a
/// code block whole.
fn closes_proposal(line: &str, fence: Fence) -> bool {
    let Some((closing, rest)) = fence_run(line) else {
        return false;
    };
    closing.marker == fence.marker && closing.len >= fence.len && rest.trim().is_empty()
}

/// True while `read` could still grow into a line that opens a proposal.
///
/// This is what [`ProposalFilter`] holds a line back on, so it and
/// [`open_proposal`] have to agree: anything this rejects is released to the
/// pane, and anything it accepts is withheld one character longer.
pub(super) fn could_open_proposal(read: &str) -> bool {
    let indent = read.len() - read.trim_start_matches(' ').len();
    if indent > 3 {
        return false;
    }
    let rest = &read[indent..];
    let Some(marker) = rest.chars().next() else {
        return true;
    };
    if marker != '`' && marker != '~' {
        return false;
    }
    let len = rest
        .chars()
        .take_while(|character| *character == marker)
        .count();
    let info = &rest[len..];
    if info.is_empty() {
        return true;
    }
    if len < MIN_FENCE {
        return false;
    }
    // A carriage return is trimmed with the spaces, so a reply written with
    // CRLF line endings holds a bare fence back the same as one written with
    // LF: `bare_fence` reads that line as bare either way, and what this
    // releases the pane has shown for good.
    let info = info.trim_start_matches([' ', '\t', '\r']);
    info.is_empty() || PROPOSAL_WORD.starts_with(info) || info.starts_with(PROPOSAL_WORD)
}

/// True while `read` could still grow into a header line.
///
/// The counterpart of [`could_open_proposal`] for the line under a bare fence:
/// what this rejects releases both lines to the pane, so it and
/// [`header_info`] have to agree the same way.
pub(super) fn could_be_header(read: &str) -> bool {
    let rest = read.trim_start_matches([' ', '\t']);
    if rest.is_empty() {
        return true;
    }
    let Some(info) = rest.strip_prefix(PROPOSAL_WORD) else {
        return PROPOSAL_WORD.starts_with(rest);
    };
    let after = info.trim_start_matches([' ', '\t']);
    after.is_empty() || PROPOSAL_PATH.starts_with(after) || after.starts_with(PROPOSAL_PATH)
}

/// A proposal a reply carried that nobody can be offered.
///
/// It holds the path the model named and why it was dropped. No note text and
/// no reply text reaches it: a path the model wrote and a reason are the whole
/// of what a person needs to see the difference between a broken feature and a
/// model that named the wrong note (ADR-031 rule 5.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DroppedProposal {
    /// The path as the model spelled it, empty when it named none.
    pub named: String,
    /// Why it was dropped.
    pub reason: DropReason,
}

/// Why a proposal a reply carried was not offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DropReason {
    /// The reply ended before the block closed, so the text is not a whole
    /// note.
    UnterminatedBlock,
    /// The path named no attached note.
    UnknownNote,
    /// The path named more than one attached note, and guessing between them
    /// would write over a note nobody chose.
    AmbiguousNote,
    /// The block held nothing but whitespace, so applying it would empty the
    /// note.
    EmptyBody,
    /// The block held the example body out of the system prompt, which the
    /// model copied instead of writing the note.
    Placeholder,
    /// An earlier block in the same reply already offered this note, and the
    /// first one is the one kept.
    Duplicate,
    /// The reply stopped at the model's token ceiling inside the block, so
    /// what it holds is the start of a note rather than the whole of one.
    Truncated,
}

/// Everything a reply's proposals came to: the ones a person can apply, and
/// the ones that were dropped with the reason.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedProposals {
    /// The proposals, in the order the reply wrote them.
    pub proposals: Vec<Proposal>,
    /// The blocks that could not become one.
    #[serde(default)]
    pub dropped: Vec<DroppedProposal>,
}

/// A path with its separators settled and its leading `./` or `/` removed.
fn normalise_path(path: &str) -> String {
    let slashed = path.replace('\\', "/");
    let trimmed = slashed.trim_start_matches("./");
    trimmed.trim_start_matches('/').to_string()
}

/// What follows the last separator of a path.
fn basename_of(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// True when `haystack` ends with `needle` at a separator boundary.
fn ends_with_path(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    match haystack.strip_suffix(needle) {
        Some("") => true,
        Some(prefix) => prefix.ends_with('/'),
        None => false,
    }
}

/// The one attached note `matches` picks out, or `None` when it picks none or
/// more than one. An ambiguous rule sets `ambiguous` and falls through to the
/// next one rather than choosing.
fn one_note<'a>(
    context: &'a [AttachedNote],
    ambiguous: &mut bool,
    matches: impl Fn(&AttachedNote) -> bool,
) -> Option<&'a AttachedNote> {
    let mut found = context.iter().filter(|note| matches(note));
    let first = found.next()?;
    if found.next().is_some() {
        *ambiguous = true;
        return None;
    }
    Some(first)
}

/// The attached note a proposal's path names, or why it names none.
fn resolve_named<'a>(
    named: &str,
    context: &'a [AttachedNote],
) -> Result<&'a AttachedNote, DropReason> {
    if named.is_empty() {
        return Err(DropReason::UnknownNote);
    }
    // What the model was shown is the first and strongest rule: a reply can only
    // land on an attachment it was given that name for. A name two attachments
    // were shown under picks neither and drops here rather than falling through,
    // because every rule under this one would answer with one of them, and a
    // reply meaning the other would write the wrong file.
    let mut shown = context.iter().filter(|note| note.prompt_path == named);
    if let Some(first) = shown.next() {
        return match shown.next() {
            Some(_) => Err(DropReason::AmbiguousNote),
            None => Ok(first),
        };
    }
    if let Some(note) = context.iter().find(|note| note.path == named) {
        return Ok(note);
    }
    let wanted = normalise_path(named);
    let mut ambiguous = false;
    let found = one_note(context, &mut ambiguous, |note| {
        normalise_path(&note.path) == wanted || normalise_path(&note.prompt_path) == wanted
    })
    .or_else(|| {
        let base = basename_of(&wanted);
        one_note(context, &mut ambiguous, |note| {
            basename_of(&normalise_path(&note.path)) == base
                || basename_of(&normalise_path(&note.prompt_path)) == base
        })
    })
    .or_else(|| {
        one_note(context, &mut ambiguous, |note| {
            ends_with_path(&wanted, &normalise_path(&note.path))
        })
    });
    match (found, ambiguous) {
        (Some(note), _) => Ok(note),
        (None, true) => Err(DropReason::AmbiguousNote),
        (None, false) => Err(DropReason::UnknownNote),
    }
}

/// The attached note a proposal's path names, resolved the way a reply spells
/// paths rather than the way Writ stores them.
///
/// The name the model was shown comes first, then the key, then separators
/// settled, then a basename only one attachment answers to, then a path only
/// one attachment is the tail of. Every rule counts attachments rather than
/// spellings, so an attachment matching on both its shown name and its key is
/// one match and not two. A name that fits none of those, or more than one
/// attachment, resolves to nothing: the result is always a note the user
/// attached, which is the fence a reply can never reach past (ADR-031 rule
/// 4.3).
pub fn resolve_proposal_path<'a>(
    named: &str,
    context: &'a [AttachedNote],
) -> Option<&'a AttachedNote> {
    resolve_named(named, context).ok()
}

/// Reads every proposal a reply carries, and what became of the rest.
///
/// A block naming a note that is not in `context` is dropped rather than
/// carried with an empty hash: the state a refusal is judged against is what
/// Writ read when the request was built, and a note nobody attached has none.
/// That drop is what keeps a reply from reaching a file the user never put in
/// front of the model (ADR-031 rules 2.5 and 4.3).
///
/// A block the reply left open runs to the end of the text, which is how
/// CommonMark reads an unclosed fence and what both local models tested write
/// -- unless `truncated` says the reply stopped at the model's token ceiling,
/// in which case that block is half a note and is dropped.
/// A body that is empty, or that is the example out of the system prompt, is
/// dropped rather than offered as a note nobody wrote, and a second block for a
/// note an earlier block already offered is dropped as a repeat.
///
/// Every drop is reported, because a proposal that vanishes without a word
/// reads as a broken feature.
pub fn parse_proposals(reply: &str, context: &[AttachedNote], truncated: bool) -> ParsedProposals {
    let lines: Vec<&str> = reply.lines().collect();
    let mut parsed = ParsedProposals::default();
    let mut index = 0;
    while index < lines.len() {
        let Some(opening) = open_proposal_at(&lines, index) else {
            index += 1;
            continue;
        };
        let named = attribute(opening.info, "path").unwrap_or_default();
        let summary = attribute(opening.info, "summary").unwrap_or_default();
        let (block, next) = read_block(&lines, opening.body, opening.fence);
        index = next;
        let Some(Block { body, closed }) = block else {
            parsed.dropped.push(DroppedProposal {
                named,
                reason: DropReason::UnterminatedBlock,
            });
            continue;
        };
        if truncated && !closed {
            parsed.dropped.push(DroppedProposal {
                named,
                reason: DropReason::Truncated,
            });
            continue;
        }
        if body.trim().is_empty() {
            parsed.dropped.push(DroppedProposal {
                named,
                reason: DropReason::EmptyBody,
            });
            continue;
        }
        if is_placeholder_body(&body) {
            parsed.dropped.push(DroppedProposal {
                named,
                reason: DropReason::Placeholder,
            });
            continue;
        }
        match resolve_named(&named, context) {
            Ok(note) if parsed.proposals.iter().any(|held| held.path == note.path) => {
                parsed.dropped.push(DroppedProposal {
                    named,
                    reason: DropReason::Duplicate,
                })
            }
            Ok(note) => {
                // The note keeps its own line ending and final newline, and
                // the card counts every line the write would change.
                let new_content = crate::notes::line_ending::in_shape_of(&note.text, &body);
                parsed.proposals.push(Proposal {
                    path: note.path.clone(),
                    before_hash: note.before_hash.clone(),
                    hunks: crate::diff::line_diff_exact(&note.text, &new_content)
                        .unwrap_or_default(),
                    new_content,
                    summary,
                })
            }
            Err(reason) => parsed.dropped.push(DroppedProposal { named, reason }),
        }
    }
    parsed
}

/// True when a body is the example out of the system prompt rather than a note.
///
/// Both the current placeholder and the one an earlier prompt carried, because
/// a small local model writes back the example it was trained beside as
/// readily as the one it was sent.
fn is_placeholder_body(body: &str) -> bool {
    let body = body.trim();
    body == BODY_PLACEHOLDER || body == LEGACY_BODY_PLACEHOLDER
}

/// Reads a proposal's body from `start`, and says where the reply goes on.
///
/// The body is `None` when the block cannot be read without guessing. The
/// close is CommonMark's: the same marker, at least as long, nothing but
/// whitespace after. Two things are read on top of that rule, because both are
/// what a model writes:
///
/// A block the reply never closed runs to the end of the text. A finished
/// reply that stops inside a fence is a model that forgot the close, not a
/// half-written note, and the end of the text is where CommonMark closes it.
///
/// A line that closes an open inner code fence *and* the proposal is
/// ambiguous: it is either the note's own code block ending or the proposal
/// ending. It is read as the inner one when the rest of the reply allows it,
/// meaning either a later bare fence long enough to close the proposal with
/// nothing but whitespace after it to the end of the reply, and no block
/// opened in between (that fence is the close), or nothing but whitespace
/// after the ambiguous line itself (the
/// block runs to the end of the text, inner fence included). When prose
/// follows and no such fence does, neither reading can be trusted and the
/// block is dropped rather than offered with a body that is wrong.
fn read_block(lines: &[&str], start: usize, fence: Fence) -> (Option<Block>, usize) {
    let mut inner: Option<Fence> = None;
    for (offset, line) in lines[start..].iter().enumerate() {
        let index = start + offset;
        match line_in_block(line, fence, inner) {
            BlockLine::Body(next) => inner = next,
            BlockLine::Closes => return (Some(Block::closed(&lines[start..index])), index + 1),
            BlockLine::Ambiguous => {
                return match last_close(lines, index + 1, fence) {
                    Some(close) => (Some(Block::closed(&lines[start..close])), close + 1),
                    None if rest_is_blank(lines, index + 1) => {
                        (Some(Block::open(&lines[start..])), lines.len())
                    }
                    None => (None, index + 1),
                }
            }
        }
    }
    (Some(Block::open(&lines[start..])), lines.len())
}

/// A proposal's body, and whether a fence ended it.
///
/// A block the reply never closed is still read, because a model that forgot
/// the close wrote a whole note; but a reply that stopped at the token ceiling
/// wrote half of one, and only the flag tells the two apart.
struct Block {
    /// The text between the fences.
    body: String,
    /// True when a closing fence ended it, false when the end of the reply
    /// did.
    closed: bool,
}

impl Block {
    /// A body a fence closed.
    fn closed(lines: &[&str]) -> Self {
        Self {
            body: joined(lines),
            closed: true,
        }
    }

    /// A body the end of the reply closed.
    fn open(lines: &[&str]) -> Self {
        Self {
            body: joined(lines),
            closed: false,
        }
    }
}

/// What one line inside a proposal's body turns out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BlockLine {
    /// Body text, carrying the inner code fence left open after it.
    Body(Option<Fence>),
    /// The line that closes the proposal.
    Closes,
    /// A line that closes an open inner code fence and the proposal both, so
    /// which of the two it is cannot be read off the line itself.
    Ambiguous,
}

/// Reads one line of a proposal's body against the fence it was opened with
/// and the inner code fence, if any, that is open at that line.
///
/// [`read_block`] and [`ProposalFilter`] both judge a body line through this,
/// so the reply a person watches arrive and the proposals parsed out of it
/// read the same grammar rather than two that have to be kept in step.
pub(super) fn line_in_block(line: &str, fence: Fence, inner: Option<Fence>) -> BlockLine {
    let Some((run, rest)) = fence_run(line) else {
        return BlockLine::Body(inner);
    };
    let bare = rest.trim().is_empty();
    match inner {
        Some(open) if bare && run.marker == open.marker && run.len >= open.len => {
            match closes_proposal(line, fence) {
                true => BlockLine::Ambiguous,
                false => BlockLine::Body(None),
            }
        }
        Some(_) => BlockLine::Body(inner),
        None if closes_proposal(line, fence) => BlockLine::Closes,
        None => BlockLine::Body(Some(run)),
    }
}

/// The last line from `from` on that could close `fence` with nothing but
/// whitespace after it to the end of the reply.
///
/// The search stops at the next line that opens a proposal, so one block's
/// recovery never reaches past a block that follows it and swallows both the
/// next note's text and the proposal it was written as.
pub(super) fn last_close(lines: &[&str], from: usize, fence: Fence) -> Option<usize> {
    let limit = (from..lines.len())
        .find(|index| open_proposal_at(lines, *index).is_some())
        .unwrap_or(lines.len());
    (from..limit)
        .rev()
        .find(|index| closes_proposal(lines[*index], fence) && rest_is_blank(lines, index + 1))
}

/// True when every line from `from` on is whitespace.
fn rest_is_blank(lines: &[&str], from: usize) -> bool {
    lines[from.min(lines.len())..]
        .iter()
        .all(|line| line.trim().is_empty())
}

/// The lines of `lines` that [`parse_proposals`] does not read as part of a
/// proposal, in order.
///
/// It walks the same two functions the parser walks, so a line is shown
/// exactly when no block holds it. [`ProposalFilter`] reads the tail of an
/// ambiguous block through this, which is the one place the filter cannot
/// decide a line as it arrives.
pub(super) fn prose_in(lines: &[&str]) -> String {
    let mut out = String::new();
    let mut index = 0;
    while index < lines.len() {
        match open_proposal_at(lines, index) {
            Some(opening) => index = read_block(lines, opening.body, opening.fence).1,
            None => {
                out.push_str(lines[index]);
                out.push('\n');
                index += 1;
            }
        }
    }
    out
}

/// The lines of a body, each with the newline a reply's lines lost.
fn joined(lines: &[&str]) -> String {
    let mut out = String::new();
    for line in lines {
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Reads `name=value` out of a fence's info string.
///
/// The value may be in double quotes, in single quotes, or bare, because all
/// three are what models write. A bare value ends at the next space, so a
/// summary written without quotes keeps its first word rather than swallowing
/// the rest of the line.
fn attribute(info: &str, name: &str) -> Option<String> {
    let key = format!("{name}=");
    let start = info.find(&key)? + key.len();
    let rest = &info[start..];
    let quote = rest.chars().next()?;
    if quote == '"' || quote == '\'' {
        let value = &rest[quote.len_utf8()..];
        let end = value.find(quote)?;
        return Some(value[..end].to_string());
    }
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    match &rest[..end] {
        "" => None,
        value => Some(value.to_string()),
    }
}
