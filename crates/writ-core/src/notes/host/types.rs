//! What the surface answers with.
//!
//! These are declared here rather than taken from the index's read model
//! because a `writ-core` trait cannot name a `writ-storage` row (ADR-032
//! section 8). The implementation maps the rows into them.

/// One note in the folder.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NoteSummary {
    /// The note's path, in the spelling every other answer takes back.
    pub path: String,
    /// What the note is called: the file name without its extension.
    pub name: String,
    /// The file's length in bytes.
    pub bytes: u64,
}

/// A note's text, as the file holds it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NoteContent {
    /// The note's path.
    pub path: String,
    /// The file's length in bytes.
    pub bytes: u64,
    /// SHA-256 of the file's bytes, in lowercase hex.
    ///
    /// This is what a write takes as its last known state: a consumer that
    /// reads a note, thinks, and writes it back hands this value over and the
    /// write is made only if the note still holds the text this hash names.
    pub hash: String,
    /// The whole file, frontmatter included.
    pub text: String,
}

/// Where a write landed and what the file holds afterwards.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct WriteReceipt {
    /// The note's path, in the spelling every other answer takes back.
    pub path: String,
    /// The file's length in bytes.
    pub bytes: u64,
    /// SHA-256 of the file's bytes, the value the next write passes as its
    /// last known state.
    pub hash: String,
}

/// Where a renamed note went, where it was, and how long it is.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RenameReceipt {
    /// The note's path now.
    pub path: String,
    /// The path it had before.
    pub previous_path: String,
    /// The file's length in bytes, which a rename does not change.
    pub bytes: u64,
}

/// One search hit.
///
/// A path, a line and an excerpt read from the file, rather than the editor's
/// own hit: a consumer of this surface holds no buffer and opens none
/// (ADR-032 section 8).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NoteHit {
    /// The note's path.
    pub path: String,
    /// What the note is called: the file name without its extension, the same
    /// shape a listing takes back.
    pub name: String,
    /// 1-based line the match is on, or `None` when the name matched.
    pub line: Option<u32>,
    /// The matching line, cut to a readable length.
    pub excerpt: String,
}

/// One link written in a note.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NoteLink {
    /// The link's target as it was written: no alias, no heading.
    pub target: String,
    /// The note the target resolved to. `None` when it resolved to nothing, and
    /// `None` when it names more than one note: an ambiguous link is never
    /// resolved to a guess (ADR-036 section 6).
    pub resolved_path: Option<String>,
    /// `wikilink` or `markdown`.
    pub kind: String,
    /// 1-based line the link is on.
    pub line: u32,
    /// 0-based character offset of the link inside that line.
    pub column: u32,
}

/// One link in another note that points at this one.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NoteBacklink {
    /// Path of the note the link is written in.
    pub from_path: String,
    /// What that note is called.
    pub from_name: String,
    /// The link's target as it was written.
    pub target: String,
    /// A wikilink's `|alias`, when it has one.
    pub alias: Option<String>,
    /// `wikilink` or `markdown`.
    pub kind: String,
    /// 1-based line the link is on.
    pub line: u32,
    /// 0-based character offset of the link inside that line.
    pub column: u32,
    /// The sentence the link sits in.
    pub context: String,
    /// `resolved` when the link means this note and no other, `ambiguous` when
    /// it names this one and at least one more.
    pub certainty: String,
    /// The other notes an ambiguous link might mean. Empty for a resolved one.
    pub candidates: Vec<String>,
}

/// What the index holds about one note beyond its text.
///
/// Read once and cut into slices by the consumer, because properties and tags
/// come out of one query (ADR-036 section 2).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NoteFacts {
    /// Frontmatter properties, each value as the JSON it is stored as.
    pub properties: Vec<(String, String)>,
    /// Each `#tag` and the 1-based line it is on.
    pub tags: Vec<(String, u32)>,
}

/// One tag in the folder, with how many notes carry it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FolderTag {
    /// The tag, without its `#`.
    pub tag: String,
    /// How many notes carry it.
    pub notes: usize,
}
