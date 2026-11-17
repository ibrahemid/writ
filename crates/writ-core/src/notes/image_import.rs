//! An image pasted or dropped into a Markdown file (ADR-035): where it is
//! saved, the name it is saved under, the reference the file gets, and what is
//! turned away.
//!
//! Pure path and string work. Reading the clipboard, reading and writing the
//! files, and canonicalising paths are the caller's.

use std::path::{Component, Path, PathBuf};

use chrono::NaiveDateTime;

use crate::config::files::ImageFolder;
use crate::preview::protocol::{sniff_image_mime, ASSET_MAX_BYTES};

/// The largest image a paste or a drop saves or refers to: the most the
/// editor draws (ADR-035).
pub const IMAGE_MAX_BYTES: u64 = ASSET_MAX_BYTES;

/// [`IMAGE_MAX_BYTES`] as a message states it.
pub const IMAGE_MAX_MB: u64 = IMAGE_MAX_BYTES / (1024 * 1024);

/// The stem a file name falls back to when nothing in it is usable.
const FALLBACK_STEM: &str = "image";

/// The most characters a saved name's stem keeps, well under every
/// filesystem's limit once a clash number and an extension are added.
const MAX_STEM_CHARS: usize = 96;

/// Why an image is not saved or referred to. The text is what the notice
/// says.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ImageImportError {
    /// The tab's file is not on disk, so there is no folder to save into.
    #[error("Save the file first, so the image has a folder to go in")]
    NoFileOnDisk,
    /// The image is over [`IMAGE_MAX_BYTES`].
    #[error("{name} is over the {limit_mb} MB limit for images")]
    TooLarge {
        /// The file's name.
        name: String,
        /// The limit in megabytes.
        limit_mb: u64,
    },
    /// The leading bytes are not an image the editor draws.
    #[error("{name} is not an image Writ can show")]
    NotAnImage {
        /// The file's name.
        name: String,
    },
    /// The file could not be read.
    #[error("Could not read {name}")]
    Unreadable {
        /// The file's name.
        name: String,
    },
    /// The folder the image goes to could not be made or written.
    #[error("Could not write to the folder {folder}")]
    FolderNotWritable {
        /// The folder, as the reference spells it, or `.` for the file's own.
        folder: String,
    },
}

/// What becomes of one image file brought into a Markdown file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImagePlacement {
    /// The file is already inside the folder tree of the Markdown file, and
    /// is referred to where it is.
    InPlace {
        /// The reference, relative to the Markdown file's folder.
        reference: String,
    },
    /// The file is copied into `folder` under `name`, or under the first
    /// numbered variant of it that is free.
    Copy {
        /// The folder the copy goes in.
        folder: PathBuf,
        /// The name the copy is saved under when nothing holds it.
        name: String,
    },
}

/// Decides where an image file goes.
///
/// `note_dir` is the folder of the Markdown file and `file` the image, both
/// canonical, so a containment test on their components is a real one. A file
/// under `note_dir` stays where it is; any other is copied into the image
/// folder under a name made safe for a link.
pub fn place_image_file(
    note_dir: &Path,
    image_folder: &ImageFolder,
    file: &Path,
    mime: &str,
) -> ImagePlacement {
    if let Some(reference) = reference_inside(note_dir, file) {
        return ImagePlacement::InPlace { reference };
    }
    let original = file
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    ImagePlacement::Copy {
        folder: image_destination(note_dir, image_folder),
        name: copied_file_name(&original, mime),
    }
}

/// The folder images are saved in for a Markdown file in `note_dir`.
pub fn image_destination(note_dir: &Path, image_folder: &ImageFolder) -> PathBuf {
    image_folder
        .parts()
        .fold(note_dir.to_path_buf(), |path, part| path.join(part))
}

/// The reference to `name` saved in the image folder, relative to the
/// Markdown file's folder.
pub fn saved_image_reference(image_folder: &ImageFolder, name: &str) -> String {
    image_folder
        .parts()
        .chain(std::iter::once(name))
        .map(encode_reference_part)
        .collect::<Vec<_>>()
        .join("/")
}

/// The reference to `file` from `note_dir` when `file` is inside it, with
/// each part percent-encoded where a link destination needs it.
///
/// Both paths are compared as given, so the caller passes canonical ones.
pub fn reference_inside(note_dir: &Path, file: &Path) -> Option<String> {
    let relative = file.strip_prefix(note_dir).ok()?;
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => parts.push(encode_reference_part(&part.to_string_lossy())),
            _ => return None,
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// The Markdown that draws `reference`.
pub fn image_markdown(reference: &str) -> String {
    format!("![]({reference})")
}

/// `part` with every character a link destination would need escaped written
/// as `%XX`. The reader of the reference decodes it (ADR-035).
pub fn encode_reference_part(part: &str) -> String {
    let mut encoded = String::with_capacity(part.len());
    for character in part.chars() {
        if is_link_safe(character) {
            encoded.push(character);
        } else {
            let mut bytes = [0u8; 4];
            for byte in character.encode_utf8(&mut bytes).bytes() {
                encoded.push_str(&format!("%{byte:02X}"));
            }
        }
    }
    encoded
}

fn is_link_safe(character: char) -> bool {
    character.is_alphanumeric() || matches!(character, '-' | '_' | '.' | '~')
}

/// `name` made safe to write in a link without escaping: letters and digits
/// in any script, `-`, `_` and `.` are kept, every run of anything else
/// becomes one `-`, and the extension is lower-cased.
pub fn link_safe_file_name(name: &str) -> String {
    let (stem, extension) = split_extension(name);
    let stem = safe_stem(stem);
    let extension: String = extension
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    if extension.is_empty() {
        stem
    } else {
        format!("{stem}.{extension}")
    }
}

/// The name a copied image is saved under: [`link_safe_file_name`] of its
/// own name, with the extension its bytes call for when it has none.
pub fn copied_file_name(name: &str, mime: &str) -> String {
    let safe = link_safe_file_name(name);
    if split_extension(&safe).1.is_empty() {
        if let Some(extension) = extension_for_mime(mime) {
            return format!("{safe}.{extension}");
        }
    }
    safe
}

/// `name` with `-n` before its extension: the name a clash moves to.
/// `number` below 2 is the name itself.
pub fn numbered_file_name(name: &str, number: u32) -> String {
    if number < 2 {
        return name.to_string();
    }
    let (stem, extension) = split_extension(name);
    if extension.is_empty() {
        format!("{stem}-{number}")
    } else {
        format!("{stem}-{number}.{extension}")
    }
}

/// The name a clipboard picture is saved under:
/// `<file stem>-<YYYYMMDD-HHMMSS>.png`.
pub fn clipboard_image_name(note_stem: &str, taken_at: NaiveDateTime) -> String {
    format!(
        "{}-{}.png",
        safe_stem(note_stem),
        taken_at.format("%Y%m%d-%H%M%S")
    )
}

/// The file extension for an image MIME type [`sniff_image_mime`] answers.
pub fn extension_for_mime(mime: &str) -> Option<&'static str> {
    Some(match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/bmp" => "bmp",
        "image/avif" => "avif",
        "image/svg+xml" => "svg",
        _ => return None,
    })
}

/// Turns away an image over [`IMAGE_MAX_BYTES`].
///
/// # Errors
///
/// [`ImageImportError::TooLarge`] naming `name` and the limit.
pub fn check_image_size(name: &str, byte_len: u64) -> Result<(), ImageImportError> {
    if byte_len > IMAGE_MAX_BYTES {
        return Err(ImageImportError::TooLarge {
            name: name.to_string(),
            limit_mb: IMAGE_MAX_MB,
        });
    }
    Ok(())
}

/// The MIME type of an image from its leading bytes, so a saved file is
/// always one the editor draws.
///
/// # Errors
///
/// [`ImageImportError::NotAnImage`] naming `name`.
pub fn check_image_head(name: &str, head: &[u8]) -> Result<&'static str, ImageImportError> {
    sniff_image_mime(head).ok_or_else(|| ImageImportError::NotAnImage {
        name: name.to_string(),
    })
}

/// The image folder as a notice names it: `.` for the file's own folder.
pub fn image_folder_label(image_folder: &ImageFolder) -> String {
    if image_folder.as_str().is_empty() {
        ".".to_string()
    } else {
        image_folder.as_str().to_string()
    }
}

fn split_extension(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(dot) if dot > 0 => (&name[..dot], &name[dot + 1..]),
        _ => (name, ""),
    }
}

fn safe_stem(stem: &str) -> String {
    let mut safe = String::with_capacity(stem.len());
    let mut pending_dash = false;
    for character in stem.chars() {
        if character.is_alphanumeric() || matches!(character, '_' | '.') {
            if pending_dash && !safe.is_empty() {
                safe.push('-');
            }
            pending_dash = false;
            safe.push(character);
        } else {
            pending_dash = true;
        }
    }
    let trimmed: String = safe
        .trim_matches(|c| c == '.' || c == '-')
        .chars()
        .take(MAX_STEM_CHARS)
        .collect();
    let trimmed = trimmed.trim_end_matches(['.', '-']).to_string();
    if trimmed.is_empty() {
        FALLBACK_STEM.to_string()
    } else {
        trimmed
    }
}
