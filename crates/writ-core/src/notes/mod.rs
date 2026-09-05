//! Notes-folder policy.
//!
//! Every note is a file in one folder the user can open in Finder, and the
//! file is the only copy of the text (ADR-028). This module holds the pure
//! half of that: where the folder is, how a title becomes a filename that
//! survives all three platforms, and how a colliding name is deduped. The
//! mechanism — creating the folder, listing it, writing the file — lives in
//! `writ-storage` and `writ-tauri`.

/// What a note says about itself: links, properties, tags and headings.
pub mod facts;
/// The write guard: whether a save may land on the file it is aimed at.
pub mod guard;
/// Which line ending a file uses, and how to keep it across a save.
pub mod line_ending;
/// Link syntax and link resolution (ADR-034).
pub mod links;
/// The sentence a link sits in, for the backlink list.
pub mod snippet;

/// File identity: telling a file that moved from one that was deleted.
pub mod identity;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local, Utc};
use unicode_segmentation::UnicodeSegmentation;

/// Folder name of the default notes folder, under the user's home folder.
pub const DEFAULT_NOTES_FOLDER: &str = "Writ";

/// Longest filename stem Writ mints, in grapheme clusters.
pub const MAX_TITLE_GRAPHEMES: usize = 120;

/// Longest filename stem Writ mints, in UTF-8 bytes. APFS caps a filename at
/// 255 bytes rather than 255 characters, so the grapheme cap alone is not
/// enough for a title written in a script that costs more than one byte per
/// character.
pub const MAX_TITLE_BYTES: usize = 200;

/// Characters no filename may carry on any of the three platforms.
const ILLEGAL_CHARS: &[char] = &['/', '\\', '<', '>', ':', '"', '|', '?', '*'];

/// Errors from resolving or validating a notes-folder path.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NotesRootError {
    /// No home folder could be resolved, so the default has no anchor.
    #[error("no home folder is available")]
    NoHome,
    /// The path is relative, which would make the folder depend on the process
    /// working directory.
    #[error("the notes folder path must be absolute: {path}")]
    NotAbsolute {
        /// The path as configured.
        path: String,
    },
}

/// Every source the notes folder can come from, highest precedence first.
///
/// A blank string counts as unset everywhere, so a hand-edited config or an
/// exported-but-empty environment variable falls through to the next source
/// instead of stopping the launch.
#[derive(Debug, Default, Clone, Copy)]
pub struct NotesRootSources<'a> {
    /// `WRIT_NOTES_DIR`. Overrides the config the way `WRIT_DATA_DIR`
    /// overrides the data folder, so a test or a recording instance can be
    /// pointed somewhere disposable without editing the user's config.
    pub env_override: Option<&'a str>,
    /// `config.notes.root`, the folder the user chose in Settings.
    pub configured: Option<&'a str>,
    /// The data folder, passed only when `WRIT_DATA_DIR` is in force. A dev
    /// instance keeps its notes beside its own database rather than writing
    /// into the notes folder the user actually reads.
    pub data_dir: Option<&'a Path>,
    /// The user's home folder, which anchors the default.
    pub home: Option<&'a Path>,
}

/// Resolves the notes folder to an absolute path.
///
/// Precedence: `WRIT_NOTES_DIR`, then `config.notes.root`, then
/// `<WRIT_DATA_DIR>/Writ` when a data folder override is in force, then
/// `<home>/Writ`. A leading `~/` is expanded against `home`. The path is not
/// created or canonicalised here — this is policy, and both touch the disk.
pub fn resolve_notes_root_from(sources: NotesRootSources<'_>) -> Result<PathBuf, NotesRootError> {
    for candidate in [sources.env_override, sources.configured] {
        let Some(value) = candidate.map(str::trim).filter(|value| !value.is_empty()) else {
            continue;
        };
        return require_absolute(expand_home(value, sources.home)?);
    }

    if let Some(data_dir) = sources.data_dir {
        return require_absolute(data_dir.join(DEFAULT_NOTES_FOLDER));
    }

    Ok(sources
        .home
        .ok_or(NotesRootError::NoHome)?
        .join(DEFAULT_NOTES_FOLDER))
}

/// Resolves the notes folder from the config alone.
///
/// `configured` is `config.notes.root`; `None` or a blank string yields
/// `<home>/Writ`. See [`resolve_notes_root_from`] for the full order the app
/// resolves in.
pub fn resolve_notes_root(
    configured: Option<&str>,
    home: Option<&Path>,
) -> Result<PathBuf, NotesRootError> {
    resolve_notes_root_from(NotesRootSources {
        configured,
        home,
        ..NotesRootSources::default()
    })
}

fn expand_home(value: &str, home: Option<&Path>) -> Result<PathBuf, NotesRootError> {
    if value == "~" {
        return Ok(home.ok_or(NotesRootError::NoHome)?.to_path_buf());
    }
    if let Some(rest) = value.strip_prefix("~/") {
        return Ok(home.ok_or(NotesRootError::NoHome)?.join(rest));
    }
    Ok(PathBuf::from(value))
}

fn require_absolute(path: PathBuf) -> Result<PathBuf, NotesRootError> {
    if path.is_absolute() {
        Ok(path)
    } else {
        Err(NotesRootError::NotAbsolute {
            path: path.to_string_lossy().into_owned(),
        })
    }
}

/// Why a folder cannot become the notes folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotesRootRefusal {
    /// The folder is Writ's own data folder, holds it, or sits inside it.
    HoldsWritData,
    /// The folder is inside the notes folder being moved out of.
    InsideNotesFolder,
}

/// Whether a picked folder may become the notes folder.
///
/// The three paths are compared as spelled, so a caller hands over canonical
/// spellings or gets an answer about paths rather than about folders.
///
/// Writ's data folder is compared both ways. An ancestor of it would pull the
/// database into the notes folder, the folder itself is the same case, and one
/// inside it is the case that does real damage: the archive Writ offers to
/// empty lives there, and a notes folder holding it makes the archive and its
/// destination the same directory.
///
/// One folder inside the data folder is allowed: `<writ_dir>/`[`DEFAULT_NOTES_FOLDER`],
/// which is what [`resolve_notes_root_from`] resolves to when a data-folder
/// override is in force. An instance pointed at its own data folder keeps its
/// notes beside its own database, so the rule cannot answer "no" to the
/// default the app itself picks.
///
/// The notes folder is compared one way only. A destination inside the folder
/// being moved has nowhere to be once the move starts, while a destination
/// that merely contains it is an ordinary move.
///
/// A `destination` equal to `current` is not a refusal; it is a move with
/// nothing to do.
pub fn refuse_notes_root(
    destination: &Path,
    current: &Path,
    writ_dir: &Path,
) -> Option<NotesRootRefusal> {
    if (destination.starts_with(writ_dir) || writ_dir.starts_with(destination))
        && destination != writ_dir.join(DEFAULT_NOTES_FOLDER)
    {
        return Some(NotesRootRefusal::HoldsWritData);
    }
    if destination != current && destination.starts_with(current) {
        return Some(NotesRootRefusal::InsideNotesFolder);
    }
    None
}

/// Collapses a leading home prefix back to `~` for display.
pub fn display_path(path: &Path, home: Option<&Path>) -> String {
    if let Some(home) = home {
        if let Ok(rest) = path.strip_prefix(home) {
            if rest.as_os_str().is_empty() {
                return "~".to_string();
            }
            return Path::new("~").join(rest).to_string_lossy().into_owned();
        }
    }
    path.to_string_lossy().into_owned()
}

/// What to call the note stored at `path`.
///
/// The file name is the note's name (ADR-028), so the display name is the last
/// component of the path without a note extension. The path is split by the
/// platform's own separators, which keeps a backslash inside the name on macOS
/// and Linux, where it is an ordinary character. A leading dot belongs to the
/// name: `.hidden.md` is `.hidden`. A name that is nothing but dots once the
/// extension is off is kept whole, because `.` names no note.
///
/// This is a display name, never an identity: [`links::name_key`] decides what
/// a link matches, and the path is what opens the file.
pub fn note_display_name(path: &str) -> String {
    let name = Path::new(path).file_name().map_or_else(
        || path.to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let stem = links::strip_note_extension(&name);
    if stem.is_empty() || stem.chars().all(|c| c == '.') {
        return name;
    }
    stem.to_string()
}

/// The strictest cross-platform filename stem, applied on every platform.
///
/// Replaces control characters and `/ \ < > : " | ? *` with a space and
/// collapses runs of whitespace to one, strips leading dots so a title never
/// mints a hidden file, strips trailing dots and spaces which Windows silently
/// drops, suffixes the reserved device names (`CON`, `PRN`, `AUX`, `NUL`,
/// `COM1` to `COM9`, `LPT1` to `LPT9`, with or without an extension) with `_`,
/// and truncates to at most [`MAX_TITLE_GRAPHEMES`] grapheme clusters and then
/// to at most [`MAX_TITLE_BYTES`] UTF-8 bytes at a grapheme boundary.
///
/// The union is applied unconditionally because the same name has to survive a
/// sync round trip onto another platform, and because the notes migration runs
/// everywhere.
///
/// Returns `None` when nothing survives.
pub fn sanitize_title(raw: &str) -> Option<String> {
    let replaced: String = raw
        .chars()
        .map(|c| {
            if c.is_control() || ILLEGAL_CHARS.contains(&c) {
                ' '
            } else {
                c
            }
        })
        .collect();

    let collapsed = collapse_whitespace(&replaced);
    let trimmed = trim_trailing_dots_and_spaces(trim_leading_dots_and_spaces(&collapsed));
    if trimmed.is_empty() {
        return None;
    }

    let guarded = guard_reserved_name(trimmed);
    let truncated = truncate_to_limits(&guarded);
    let cleaned = trim_trailing_dots_and_spaces(&truncated);
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned.to_string())
    }
}

/// [`sanitize_title`], falling back to `fallback` (already sanitised) when it
/// returns `None`.
pub fn sanitize_title_or(raw: &str, fallback: &str) -> String {
    sanitize_title(raw).unwrap_or_else(|| fallback.to_string())
}

/// What a name that survives [`rename_stem`] as nothing is called, said once
/// for every surface that asks for a name.
pub const NAME_IS_EMPTY: &str = "That name is empty.";

/// What a folder that already holds `name` is answered with, said once for
/// every surface that mints or renames a note.
pub fn name_is_taken(name: &str) -> String {
    format!("A note named \"{name}\" is already there.")
}

/// The filename stem a typed name earns when renaming `current`, with the
/// note's own extension removed first.
///
/// A note is shown by its file name, `Grocery list.md` and not `Grocery list`,
/// so a user editing that name in place hands back a string that already ends
/// in `.md`. Sanitising it whole would mint `Grocery list.md.md`. Only the
/// note's current extension is stripped: a note renamed to `Recipes.2026` keeps
/// the year, because that is not an extension this file has.
///
/// The result is a stem and never a path. [`sanitize_title`] maps `/` and `\`
/// to spaces, so an absolute name cannot come back out of this and a name
/// carrying `..` loses the separators that would make it walk anywhere; a
/// caller joining the result onto the note's own folder keeps the note in it.
///
/// Returns `None` when nothing survives, which is the empty name.
pub fn rename_stem(current: &Path, typed: &str) -> Option<String> {
    let typed = typed.trim();
    let base = match current.extension().and_then(|ext| ext.to_str()) {
        Some(extension) => {
            let suffix = format!(".{extension}");
            match typed.len() > suffix.len()
                && typed[typed.len() - suffix.len()..].eq_ignore_ascii_case(&suffix)
            {
                true => &typed[..typed.len() - suffix.len()],
                false => typed,
            }
        }
        None => typed,
    };
    sanitize_title(base)
}

/// `YYYY-MM-DD` in the local calendar day of `now`.
///
/// The local day is what the user calls today, and a note named for a day the
/// user has not reached yet reads as a bug.
pub fn date_stem(now: DateTime<Utc>) -> String {
    now.with_timezone(&Local).format("%Y-%m-%d").to_string()
}

/// Whether `title` is one Writ minted rather than one a person typed.
///
/// Every note created before 0.4 was titled `writ-<millis>`, which names
/// nothing and would mint a filename nobody recognises. The notes migration
/// and the first save of a new note both replace such a title with the note's
/// date, so the rule has to be one function.
pub fn is_minted_title(title: &str) -> bool {
    let Some(rest) = title.trim().strip_prefix("writ-") else {
        return false;
    };
    rest.starts_with(|c: char| c.is_ascii_digit())
}

/// The filename stem a note earns from its title, dated when the title names
/// nothing.
///
/// `dated_from` supplies the fallback: the moment the note reaches a file for
/// a new note, the row's creation time for one the migration is writing out.
/// The result is sanitised and never empty, so the caller only has to dedupe
/// it against the folder it is going into.
pub fn note_file_stem(title: &str, dated_from: DateTime<Utc>) -> String {
    let fallback = date_stem(dated_from);
    if title.trim().is_empty() || is_minted_title(title) {
        return fallback;
    }
    sanitize_title_or(title, &fallback)
}

/// Finder-style dedupe: `stem`, `stem 2`, `stem 3`, and so on.
///
/// `taken` holds file *names* including their extension, in whatever case and
/// whatever Unicode normalisation the folder listing gave them. Both sides go
/// through [`links::name_key`], so the check is case-insensitive the way APFS
/// and NTFS are, and a name macOS stored decomposed still counts as taken
/// against the composed spelling of the same name. A dedupe that missed that
/// hands back a name the filesystem already holds. `extension` is given
/// without a dot; pass an empty string for a name that has none.
pub fn dedupe_file_name(stem: &str, extension: &str, taken: &HashSet<String>) -> String {
    let taken: HashSet<String> = taken.iter().map(|name| links::name_key(name)).collect();
    let candidate = join_name(stem, extension);
    if !taken.contains(&links::name_key(&candidate)) {
        return candidate;
    }

    let mut counter: u64 = 2;
    loop {
        let candidate = join_name(&format!("{stem} {counter}"), extension);
        if !taken.contains(&links::name_key(&candidate)) {
            return candidate;
        }
        counter += 1;
    }
}

/// The name a dated copy of one side of a conflict takes, beside the note it
/// belongs to: `<stem> (conflict YYYY-MM-DD HH.MM.SS)` plus the extension.
///
/// A stopped save writes the text it was carrying under this name before it
/// returns, so no such save can end with the user's text nowhere (ADR-028 §5).
pub fn conflict_file_name(stem: &str, extension: &str, now: DateTime<Utc>) -> String {
    dated_copy_name(stem, extension, "conflict", now)
}

/// The name text recovered from the crash snapshot takes when the file it
/// belongs to has moved on: `<stem> (recovered YYYY-MM-DD HH.MM.SS)` plus the
/// extension.
///
/// A relaunch must not write a pre-crash snapshot over a version a sync client
/// delivered while Writ was down, and it must not drop the snapshot either, so
/// the snapshot lands under this name and both are on disk.
pub fn recovered_file_name(stem: &str, extension: &str, now: DateTime<Utc>) -> String {
    dated_copy_name(stem, extension, "recovered", now)
}

/// The shared shape of both: `<stem> (<label> YYYY-MM-DD HH.MM.SS).<extension>`.
///
/// The clock is the local one, because the name is read by a person looking at
/// a folder, and it is written with dots: a colon is illegal in a filename on
/// Windows and reads as a path separator in Finder. `extension` is given
/// without a dot; pass an empty string for a name that has none.
fn dated_copy_name(stem: &str, extension: &str, label: &str, now: DateTime<Utc>) -> String {
    let stamp = now.with_timezone(&Local).format("%Y-%m-%d %H.%M.%S");
    join_name(&format!("{stem} ({label} {stamp})"), extension)
}

fn join_name(stem: &str, extension: &str) -> String {
    if extension.is_empty() {
        stem.to_string()
    } else {
        format!("{stem}.{extension}")
    }
}

fn collapse_whitespace(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut in_whitespace = false;
    for c in value.chars() {
        if c.is_whitespace() {
            in_whitespace = true;
            continue;
        }
        if in_whitespace && !out.is_empty() {
            out.push(' ');
        }
        in_whitespace = false;
        out.push(c);
    }
    out
}

fn trim_leading_dots_and_spaces(value: &str) -> &str {
    value.trim_start_matches(|c: char| c == '.' || c.is_whitespace())
}

fn trim_trailing_dots_and_spaces(value: &str) -> &str {
    value.trim_end_matches(|c: char| c == '.' || c.is_whitespace())
}

/// Suffixes a reserved Windows device name with `_`.
///
/// The check applies to the part before the first dot, because Windows treats
/// `NUL.md` as the device too, and suffixing the whole name would leave the
/// device name in front of the dot.
fn guard_reserved_name(value: &str) -> String {
    let (head, rest) = match value.find('.') {
        Some(at) => (&value[..at], &value[at..]),
        None => (value, ""),
    };
    if is_reserved_device_name(head) {
        format!("{head}_{rest}")
    } else {
        value.to_string()
    }
}

fn is_reserved_device_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    if matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
        return true;
    }
    for prefix in ["COM", "LPT"] {
        if let Some(rest) = upper.strip_prefix(prefix) {
            if rest.len() == 1 && matches!(rest.as_bytes()[0], b'1'..=b'9') {
                return true;
            }
        }
    }
    false
}

fn truncate_to_limits(value: &str) -> String {
    if value.len() <= MAX_TITLE_BYTES && value.graphemes(true).count() <= MAX_TITLE_GRAPHEMES {
        return value.to_string();
    }

    let mut out = String::with_capacity(MAX_TITLE_BYTES.min(value.len()));
    for (graphemes, cluster) in value.graphemes(true).enumerate() {
        if graphemes >= MAX_TITLE_GRAPHEMES || out.len() + cluster.len() > MAX_TITLE_BYTES {
            break;
        }
        out.push_str(cluster);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_note_is_called_by_its_file_name_without_the_extension() {
        assert_eq!(note_display_name("/notes/folder/name.md"), "name");
        assert_eq!(note_display_name("/notes/name.tar.md"), "name.tar");
        assert_eq!(note_display_name("/notes/Meeting.markdown"), "Meeting");
        assert_eq!(note_display_name("/notes/notes.txt"), "notes.txt");
        assert_eq!(note_display_name("Loose.md"), "Loose");
    }

    #[test]
    fn a_leading_dot_is_part_of_the_name() {
        assert_eq!(note_display_name("/notes/.hidden.md"), ".hidden");
        assert_eq!(
            note_display_name("/notes/..md"),
            "..md",
            "a name of nothing but dots is kept whole"
        );
        assert_eq!(note_display_name("/notes/.md"), ".md");
    }

    #[test]
    #[cfg(not(windows))]
    fn a_backslash_is_a_name_character_where_it_is_not_a_separator() {
        assert_eq!(note_display_name("/notes/a\\b.md"), "a\\b");
    }

    #[test]
    fn a_rename_strips_the_note_s_own_extension_once() {
        let note = Path::new("/notes/Grocery list.md");
        assert_eq!(
            rename_stem(note, "Shopping.md").as_deref(),
            Some("Shopping")
        );
        assert_eq!(
            rename_stem(note, "Shopping.MD").as_deref(),
            Some("Shopping")
        );
        assert_eq!(
            rename_stem(note, "Shopping.md.md").as_deref(),
            Some("Shopping.md"),
            "one suffix comes off, not every one"
        );
    }

    #[test]
    fn a_rename_to_nothing_but_the_extension_keeps_it_whole() {
        // Stripping here would leave nothing, so `.md` is a name and the note
        // becomes `md.md` rather than being refused.
        assert_eq!(
            rename_stem(Path::new("/notes/2026-08-29.md"), ".md").as_deref(),
            Some("md")
        );
    }

    #[test]
    fn a_rename_keeps_an_extension_the_note_does_not_have() {
        assert_eq!(
            rename_stem(Path::new("/notes/Recipes.md"), "Recipes.2026").as_deref(),
            Some("Recipes.2026")
        );
        assert_eq!(
            rename_stem(Path::new("/notes/plain"), "Notes.md").as_deref(),
            Some("Notes.md"),
            "a file with no extension strips nothing"
        );
    }

    #[test]
    fn a_rename_cannot_name_a_path() {
        let note = Path::new("/notes/One.md");
        // The stem a caller joins onto the note's folder. Nothing here may
        // carry a separator, be absolute, or start with a parent-directory
        // step, or the join would put the note somewhere else entirely.
        for typed in [
            "/tmp/pwned",
            "../../escaped",
            "..\\..\\escaped",
            "/etc/passwd",
            "~/elsewhere/note",
            "a/b/c",
        ] {
            let stem = rename_stem(note, typed).expect("a stem survives");
            assert!(
                !stem.contains('/') && !stem.contains('\\'),
                "{typed} kept a separator: {stem}"
            );
            assert!(!Path::new(&stem).is_absolute(), "{typed} stayed absolute");
            assert_eq!(
                Path::new(&stem).components().count(),
                1,
                "{typed} is more than one path component: {stem}"
            );
            assert!(!stem.starts_with(".."), "{typed} still walks up: {stem}");
        }
    }

    #[test]
    fn a_rename_drops_the_characters_a_filename_may_not_carry() {
        let stem =
            rename_stem(Path::new("/notes/One.md"), "a:b?c*d<e>f\"g|h").expect("a stem survives");
        for illegal in [':', '?', '*', '<', '>', '"', '|'] {
            assert!(!stem.contains(illegal), "{illegal} survived: {stem}");
        }
    }

    #[test]
    fn a_rename_to_nothing_is_refused() {
        for typed in ["", "   ", "...", "///"] {
            assert_eq!(
                rename_stem(Path::new("/notes/One.md"), typed),
                None,
                "{typed} should not name a note"
            );
        }
    }

    #[test]
    fn reserved_names_are_recognised_case_insensitively() {
        for name in [
            "CON", "con", "Prn", "AUX", "nul", "COM1", "com9", "LPT1", "lpt9",
        ] {
            assert!(is_reserved_device_name(name), "{name} is reserved");
        }
        for name in ["COM0", "COM10", "CONTRACT", "LPT", "notes"] {
            assert!(!is_reserved_device_name(name), "{name} is not reserved");
        }
    }

    #[test]
    fn truncation_never_splits_a_grapheme_cluster() {
        let flag = "\u{1F1F8}\u{1F1E6}";
        let raw = flag.repeat(60);
        let truncated = truncate_to_limits(&raw);
        assert!(truncated.len() <= MAX_TITLE_BYTES);
        assert_eq!(truncated.len() % flag.len(), 0);
        assert!(raw.starts_with(&truncated));
    }
}
