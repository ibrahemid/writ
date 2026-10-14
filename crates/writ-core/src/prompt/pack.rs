//! A context pack: several files as one prompt.
//!
//! Each file is prepared the way Copy as prompt prepares one
//! ([`super::strip_for_prompt`]) and wrapped in a `<file path="…">` element,
//! so a model can tell the files apart and name the one it means. The pack
//! carries its own token estimate, the estimate of each file, the files that
//! could not be read and why, and how many files it left out to stay under its
//! limits: the person sees what was cut before copying (ADR-026).

use serde::Serialize;

use super::{estimate_tokens, strip_for_prompt};

/// Most files one pack carries. A folder or tag with more is packed up to
/// here, and the rest are counted in [`ContextPack::omitted`].
pub const MAX_PACK_FILES: usize = 100;

/// Most prepared text one pack carries, in bytes. A file that would take the
/// pack past it is left out with every file after it.
pub const MAX_PACK_BYTES: usize = 4 * 1024 * 1024;

/// Why a file the pack named is not in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    /// Larger than a file Writ reads for a program or a prompt.
    TooLarge,
    /// Not text in any encoding Writ reads.
    NotText,
    /// In the cloud and not on this computer.
    NotDownloaded,
    /// Gone, outside the folder, or refused by the system.
    Unreadable,
}

/// One file in the pack.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PackedFile {
    /// The path the pack names it by, relative to the folder.
    pub path: String,
    /// The estimate of its prepared text.
    pub tokens: usize,
}

/// One file the pack named and could not carry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkippedFile {
    /// The path the pack names it by, relative to the folder.
    pub path: String,
    /// Why it is not in the pack.
    pub reason: SkipReason,
}

/// The prompt and what went into it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContextPack {
    /// What Copy puts on the clipboard.
    pub text: String,
    /// The estimate of `text`.
    pub tokens: usize,
    /// The files in `text`, in its order.
    pub files: Vec<PackedFile>,
    /// The files that could not be read.
    pub skipped: Vec<SkippedFile>,
    /// Files left out to stay under [`MAX_PACK_FILES`] and [`MAX_PACK_BYTES`].
    pub omitted: usize,
}

/// Builds a pack one file at a time, in the order the files are given.
#[derive(Debug, Default)]
pub struct PackBuilder {
    text: String,
    files: Vec<PackedFile>,
    skipped: Vec<SkippedFile>,
    omitted: usize,
    full: bool,
}

impl PackBuilder {
    /// An empty pack.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the pack takes no more files; every later one is omitted.
    pub fn is_full(&self) -> bool {
        self.full
    }

    /// Adds `text` as the file at `path`, or counts it as omitted once the
    /// pack is at a limit.
    pub fn add(&mut self, path: &str, text: &str) {
        if self.full || self.files.len() >= MAX_PACK_FILES {
            self.full = true;
            self.omitted += 1;
            return;
        }
        let prepared = strip_for_prompt(text);
        let element = file_element(path, &prepared);
        if self.text.len() + element.len() > MAX_PACK_BYTES {
            self.full = true;
            self.omitted += 1;
            return;
        }
        self.text.push_str(&element);
        self.files.push(PackedFile {
            path: path.to_string(),
            tokens: estimate_tokens(&prepared),
        });
    }

    /// Records a file the pack could not read.
    pub fn skip(&mut self, path: &str, reason: SkipReason) {
        self.skipped.push(SkippedFile {
            path: path.to_string(),
            reason,
        });
    }

    /// Counts a file left out without reading it, the pack being full.
    pub fn omit(&mut self) {
        self.full = true;
        self.omitted += 1;
    }

    /// The prompt, its estimate and its account of what went in.
    pub fn finish(self) -> ContextPack {
        let text = self.text.trim_end().to_string();
        ContextPack {
            tokens: estimate_tokens(&text),
            text,
            files: self.files,
            skipped: self.skipped,
            omitted: self.omitted,
        }
    }
}

/// One file as the pack writes it, followed by a blank line.
fn file_element(path: &str, prepared: &str) -> String {
    let mut element = format!("<file path=\"{}\">\n", escape_attribute(path));
    element.push_str(prepared);
    if !prepared.is_empty() && !prepared.ends_with('\n') {
        element.push('\n');
    }
    element.push_str("</file>\n\n");
    element
}

fn escape_attribute(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_each_file_in_an_element_named_by_its_path() {
        let mut pack = PackBuilder::new();
        pack.add("plan.md", "# Plan\nShip it.\n");
        pack.add("notes/meeting.md", "Agenda\n");
        let pack = pack.finish();
        assert_eq!(
            pack.text,
            "<file path=\"plan.md\">\n# Plan\nShip it.\n</file>\n\n\
             <file path=\"notes/meeting.md\">\nAgenda\n</file>"
        );
        assert_eq!(
            pack.files
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            ["plan.md", "notes/meeting.md"]
        );
    }

    #[test]
    fn prepares_each_file_as_copy_as_prompt_does() {
        let mut pack = PackBuilder::new();
        pack.add("a.md", "---\ntitle: A\n---\nBody <!-- aside -->\n");
        let pack = pack.finish();
        assert_eq!(pack.text, "<file path=\"a.md\">\nBody\n</file>");
    }

    #[test]
    fn counts_the_whole_prompt_and_each_file() {
        let mut pack = PackBuilder::new();
        pack.add("a.md", "one two three four five six seven eight\n");
        pack.add("b.md", "nine ten\n");
        let pack = pack.finish();
        assert_eq!(pack.tokens, estimate_tokens(&pack.text));
        assert_eq!(
            pack.files[0].tokens,
            estimate_tokens("one two three four five six seven eight\n")
        );
        assert!(pack.files[0].tokens > pack.files[1].tokens);
    }

    #[test]
    fn keeps_a_path_from_closing_its_own_attribute() {
        let mut pack = PackBuilder::new();
        pack.add("say \"hi\" <now>.md", "x\n");
        assert!(pack
            .finish()
            .text
            .starts_with("<file path=\"say &quot;hi&quot; &lt;now>.md\">"));
    }

    #[test]
    fn writes_an_empty_file_as_an_empty_element() {
        let mut pack = PackBuilder::new();
        pack.add("empty.md", "");
        assert_eq!(pack.finish().text, "<file path=\"empty.md\">\n</file>");
    }

    #[test]
    fn stops_at_the_file_limit_and_counts_the_rest() {
        let mut pack = PackBuilder::new();
        for n in 0..MAX_PACK_FILES + 3 {
            pack.add(&format!("{n}.md"), "x\n");
        }
        assert!(pack.is_full());
        let pack = pack.finish();
        assert_eq!(pack.files.len(), MAX_PACK_FILES);
        assert_eq!(pack.omitted, 3);
    }

    #[test]
    fn stops_at_the_byte_limit_and_leaves_out_every_later_file() {
        let big = "a ".repeat(MAX_PACK_BYTES / 4);
        let mut pack = PackBuilder::new();
        pack.add("first.md", &big);
        pack.add("second.md", &big);
        pack.add("small.md", "x\n");
        let pack = pack.finish();
        assert_eq!(pack.files.len(), 1);
        assert_eq!(pack.omitted, 2);
        assert!(pack.text.len() <= MAX_PACK_BYTES);
    }

    #[test]
    fn lists_the_files_it_could_not_read_beside_the_ones_it_carries() {
        let mut pack = PackBuilder::new();
        pack.add("a.md", "x\n");
        pack.skip("photo.png", SkipReason::NotText);
        let pack = pack.finish();
        assert_eq!(pack.files.len(), 1);
        assert_eq!(
            pack.skipped,
            [SkippedFile {
                path: "photo.png".into(),
                reason: SkipReason::NotText
            }]
        );
    }

    #[test]
    fn an_empty_pack_is_empty_text_with_no_tokens() {
        let pack = PackBuilder::new().finish();
        assert_eq!(pack.text, "");
        assert_eq!(pack.tokens, 0);
    }
}
