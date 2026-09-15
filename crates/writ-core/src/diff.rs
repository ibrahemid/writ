//! A line diff, for showing what a proposal would change.
//!
//! One proposal carries the note as it was read and the whole text the model
//! would put in its place ([`crate::chat::Proposal`]). Both are already in
//! memory when the reply is parsed, so the comparison is made here, once, and
//! the card draws what it is given rather than computing it a second time in
//! the pane.
//!
//! The search is Myers' `O(ND)`: a forward pass that records the furthest
//! reaching path on each diagonal, keeping one row per edit distance so the
//! script can be walked back out of it. Two texts that share a head and a tail
//! — which is what a rewritten note and its original are — pay only for the
//! middle, because the shared lines are matched off before the search starts.
//!
//! Two limits keep a comparison bounded. A side over [`MAX_DIFF_BYTES`] is
//! refused outright (ADR-031 rule 4.8). A middle whose edit distance passes
//! [`MAX_EDIT_DISTANCE`] stops the search and reports every line of one side
//! removed and every line of the other added: past that point the two texts
//! have nothing in common worth showing line by line, and an unbounded search
//! over two large unrelated texts is quadratic.

use serde::{Deserialize, Serialize};

/// The largest text either side may hold (ADR-031 rule 4.8).
pub const MAX_DIFF_BYTES: usize = 2 * 1024 * 1024;

/// How many edits the search follows before it gives up on a minimal script.
pub const MAX_EDIT_DISTANCE: usize = 1000;

/// Unchanged lines kept on each side of a change.
const CONTEXT: usize = 3;

/// What one line of a hunk is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LineKind {
    /// Present in both texts.
    Context,
    /// In the first text only.
    Removed,
    /// In the second text only.
    Added,
}

/// One line of a hunk, without its line ending.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffLine {
    /// Which side it belongs to.
    pub kind: LineKind,
    /// The line itself.
    pub text: String,
}

/// One run of change with its surrounding context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hunk {
    /// The 1-based line in the first text this hunk starts at, or 0 when it
    /// carries no line from that text.
    pub before_start: usize,
    /// The 1-based line in the second text this hunk starts at, or 0 when it
    /// carries no line from that text.
    pub after_start: usize,
    /// The hunk's lines, in reading order.
    pub lines: Vec<DiffLine>,
}

/// Why a comparison was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DiffError {
    /// One side is larger than the crate compares.
    #[error("this text is too large to compare ({bytes} bytes, limit {limit})")]
    TooLarge {
        /// The larger of the two sides, in bytes.
        bytes: usize,
        /// [`MAX_DIFF_BYTES`].
        limit: usize,
    },
}

/// Compares two texts line by line.
///
/// A trailing newline does not make an empty last line, so a note that ends
/// with one and a proposal that does not read as the same text. Identical
/// texts produce no hunks. Adjacent changes share one hunk when their context
/// touches, so a hunk is never separated from the next by a gap of nothing.
pub fn line_diff(before: &str, after: &str) -> Result<Vec<Hunk>, DiffError> {
    let bytes = before.len().max(after.len());
    if bytes > MAX_DIFF_BYTES {
        return Err(DiffError::TooLarge {
            bytes,
            limit: MAX_DIFF_BYTES,
        });
    }
    let before: Vec<&str> = before.lines().collect();
    let after: Vec<&str> = after.lines().collect();
    Ok(hunks(&edit_script(&before, &after)))
}

/// One entry of the script, before it is cut into hunks.
struct Entry<'a> {
    kind: LineKind,
    text: &'a str,
}

/// The whole comparison as one list, shared lines included.
fn edit_script<'a>(before: &[&'a str], after: &[&'a str]) -> Vec<Entry<'a>> {
    let head = before
        .iter()
        .zip(after.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let tail = before[head..]
        .iter()
        .rev()
        .zip(after[head..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let mut script: Vec<Entry<'a>> = before[..head]
        .iter()
        .map(|text| Entry {
            kind: LineKind::Context,
            text,
        })
        .collect();
    let middle_before = &before[head..before.len() - tail];
    let middle_after = &after[head..after.len() - tail];
    match myers(middle_before, middle_after) {
        Some(middle) => script.extend(middle),
        None => {
            script.extend(middle_before.iter().map(|text| Entry {
                kind: LineKind::Removed,
                text,
            }));
            script.extend(middle_after.iter().map(|text| Entry {
                kind: LineKind::Added,
                text,
            }));
        }
    }
    script.extend(before[before.len() - tail..].iter().map(|text| Entry {
        kind: LineKind::Context,
        text,
    }));
    script
}

/// Myers' forward search over two texts that share neither head nor tail.
///
/// `None` when the edit distance passes [`MAX_EDIT_DISTANCE`].
fn myers<'a>(before: &[&'a str], after: &[&'a str]) -> Option<Vec<Entry<'a>>> {
    let n = before.len() as isize;
    let m = after.len() as isize;
    let max = (n + m).min(MAX_EDIT_DISTANCE as isize);
    // One slot per diagonal `k` in `-(n + m) ..= (n + m)`, shifted so `k = 0`
    // sits in the middle. The two extra slots are the `k ± 1` reads at the
    // edges of the band.
    let offset = n + m + 1;
    let mut furthest = vec![0isize; (2 * (n + m) + 3) as usize];
    // The band `-d ..= d` of `furthest` as it stood before each round, which
    // is all the backtrack reads. Keeping the band rather than the whole row
    // is what holds the memory at the edit distance rather than at the text.
    let mut trace: Vec<Vec<isize>> = Vec::new();
    for d in 0..=max {
        trace.push(furthest[(offset - d - 1) as usize..=(offset + d + 1) as usize].to_vec());
        let mut k = -d;
        while k <= d {
            let mut x = if k == -d
                || (k != d
                    && furthest[(k - 1 + offset) as usize] < furthest[(k + 1 + offset) as usize])
            {
                furthest[(k + 1 + offset) as usize]
            } else {
                furthest[(k - 1 + offset) as usize] + 1
            };
            let mut y = x - k;
            while x < n && y < m && before[x as usize] == after[y as usize] {
                x += 1;
                y += 1;
            }
            furthest[(k + offset) as usize] = x;
            if x >= n && y >= m {
                return Some(backtrack(before, after, &trace));
            }
            k += 2;
        }
    }
    None
}

/// Walks the recorded rounds back from the end, newest first.
fn backtrack<'a>(before: &[&'a str], after: &[&'a str], trace: &[Vec<isize>]) -> Vec<Entry<'a>> {
    let mut script: Vec<Entry<'a>> = Vec::new();
    let mut x = before.len() as isize;
    let mut y = after.len() as isize;
    for (round, band) in trace.iter().enumerate().rev() {
        let d = round as isize;
        let k = x - y;
        // The band is indexed from `-d - 1`, so a diagonal reads at `k + d + 1`.
        let at = |diagonal: isize| band[(diagonal + d + 1) as usize];
        let previous = if k == -d || (k != d && at(k - 1) < at(k + 1)) {
            k + 1
        } else {
            k - 1
        };
        let previous_x = at(previous);
        let previous_y = previous_x - previous;
        while x > previous_x && y > previous_y {
            script.push(Entry {
                kind: LineKind::Context,
                text: before[(x - 1) as usize],
            });
            x -= 1;
            y -= 1;
        }
        if d > 0 {
            if x == previous_x {
                script.push(Entry {
                    kind: LineKind::Added,
                    text: after[(y - 1) as usize],
                });
                y -= 1;
            } else {
                script.push(Entry {
                    kind: LineKind::Removed,
                    text: before[(x - 1) as usize],
                });
                x -= 1;
            }
        }
    }
    script.reverse();
    script
}

/// Cuts the script into hunks of change plus [`CONTEXT`] lines each side.
fn hunks(script: &[Entry<'_>]) -> Vec<Hunk> {
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for (index, entry) in script.iter().enumerate() {
        if entry.kind == LineKind::Context {
            continue;
        }
        let start = index.saturating_sub(CONTEXT);
        let end = (index + CONTEXT + 1).min(script.len());
        match ranges.last_mut() {
            Some(last) if start <= last.1 => last.1 = end,
            _ => ranges.push((start, end)),
        }
    }
    // The 1-based line each entry holds on the side it came from.
    let mut numbers: Vec<(usize, usize)> = Vec::with_capacity(script.len());
    let (mut before_line, mut after_line) = (1usize, 1usize);
    for entry in script {
        match entry.kind {
            LineKind::Context => {
                numbers.push((before_line, after_line));
                before_line += 1;
                after_line += 1;
            }
            LineKind::Removed => {
                numbers.push((before_line, 0));
                before_line += 1;
            }
            LineKind::Added => {
                numbers.push((0, after_line));
                after_line += 1;
            }
        }
    }
    ranges
        .into_iter()
        .map(|(start, end)| Hunk {
            before_start: numbers[start..end]
                .iter()
                .map(|(before, _)| *before)
                .find(|line| *line > 0)
                .unwrap_or(0),
            after_start: numbers[start..end]
                .iter()
                .map(|(_, after)| *after)
                .find(|line| *line > 0)
                .unwrap_or(0),
            lines: script[start..end]
                .iter()
                .map(|entry| DiffLine {
                    kind: entry.kind,
                    text: entry.text.to_string(),
                })
                .collect(),
        })
        .collect()
}
