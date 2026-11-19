//! An image pasted or dropped into a Markdown file (ADR-035): where it is
//! saved, the name it is saved under, the reference the file gets, and what is
//! turned away.
//!
//! Pure path and string work. Reading the clipboard, reading and writing the
//! files, and canonicalising paths are the caller's.

use std::path::{Component, Path, PathBuf};

use chrono::NaiveDateTime;
use unicode_segmentation::UnicodeSegmentation;

use crate::config::files::ImageFolder;
use crate::preview::protocol::{sniff_image_mime, ASSET_MAX_BYTES};

/// The largest image a paste or a drop saves or refers to: the most the
/// editor draws (ADR-035).
pub const IMAGE_MAX_BYTES: u64 = ASSET_MAX_BYTES;

/// [`IMAGE_MAX_BYTES`] as a message states it.
pub const IMAGE_MAX_MB: u64 = IMAGE_MAX_BYTES / (1024 * 1024);

/// The most numbered names a save tries, `name-2` up to `name-999`, before
/// it gives up.
pub const MAX_NUMBERED_NAMES: u32 = 999;

/// The stem a file name falls back to when nothing in it is usable.
const FALLBACK_STEM: &str = "image";

/// The most characters a saved name's stem keeps.
const MAX_STEM_CHARS: usize = 96;

/// The most bytes a saved name's stem keeps, so the longest name a save
/// makes stays within the 255 a file name holds: the stem, a clipboard
/// picture's `-YYYYMMDD-HHMMSS`, a clash's `-999` and `.webp`.
const MAX_STEM_BYTES: usize = 255 - "-20261117-090507".len() - "-999".len() - ".webp".len();

/// The extensions an image file is named with, which a copy trades for the
/// one its bytes call for.
const IMAGE_EXTENSIONS: &[&str] = &[
    "apng", "avif", "bmp", "gif", "heic", "heif", "ico", "jfif", "jpe", "jpeg", "jpg", "png",
    "svg", "tif", "tiff", "webp",
];

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
    #[error("{name} is not an image Jotra can show")]
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
    /// Every numbered form of the name is taken by other bytes.
    #[error("Could not find a free name for {name} in the folder {folder}")]
    NoFreeName {
        /// The name the image would have had.
        name: String,
        /// The folder, as the reference spells it, or `.` for the file's own.
        folder: String,
    },
    /// The image folder, or a link on the way to it, resolves outside the
    /// file's folder.
    #[error("The folder {folder} leads outside the file's folder")]
    FolderOutside {
        /// The folder, as the reference spells it.
        folder: String,
    },
    /// A clipboard picture over [`IMAGE_MAX_BYTES`] as PNG, and as JPEG when
    /// it has no transparency.
    #[error("The copied picture is over the {limit_mb} MB limit for images")]
    PictureTooLarge {
        /// The limit in megabytes.
        limit_mb: u64,
    },
}

/// Whether `text`, trimmed, is one `http` or `https` address and nothing
/// else, as a browser puts beside a picture it copies.
pub fn is_lone_web_address(text: &str) -> bool {
    let trimmed = text.trim();
    !trimmed.is_empty()
        && !trimmed.contains(char::is_whitespace)
        && url::Url::parse(trimmed).is_ok_and(|address| {
            matches!(address.scheme(), "http" | "https") && address.host().is_some()
        })
}

/// The notice for `count` copied files a paste passed over because they are
/// not images, or `None` when it passed over none.
pub fn not_images_notice(count: usize) -> Option<String> {
    match count {
        0 => None,
        1 => Some("1 file is not an image and was not pasted".to_string()),
        n => Some(format!("{n} files are not images and were not pasted")),
    }
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
    let stem = safe_file_stem(stem);
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

/// The name a copied image is saved under: its own name made safe for a
/// link, with the extension its bytes call for in place of any image
/// extension the name carried.
pub fn copied_file_name(name: &str, mime: &str) -> String {
    let Some(extension) = extension_for_mime(mime) else {
        return link_safe_file_name(name);
    };
    let (stem, own) = split_extension(name);
    let stem = if IMAGE_EXTENSIONS.contains(&own.to_ascii_lowercase().as_str()) {
        stem
    } else {
        name
    };
    format!("{}.{extension}", safe_file_stem(stem))
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
/// `<file stem>-<YYYYMMDD-HHMMSS>.<extension>`.
pub fn clipboard_image_name(note_stem: &str, taken_at: NaiveDateTime, extension: &str) -> String {
    format!(
        "{}-{}.{extension}",
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
    let mut capped = String::with_capacity(MAX_STEM_BYTES.min(safe.len()));
    let mut chars = 0;
    for cluster in safe.trim_matches(['.', '-']).graphemes(true) {
        chars += cluster.chars().count();
        if chars > MAX_STEM_CHARS || capped.len() + cluster.len() > MAX_STEM_BYTES {
            break;
        }
        capped.push_str(cluster);
    }
    let trimmed = capped.trim_end_matches(['.', '-']);
    if trimmed.is_empty() {
        FALLBACK_STEM.to_string()
    } else {
        trimmed.to_string()
    }
}

/// [`safe_stem`] for a name that stands alone, with a device name such as
/// `CON` moved off it.
fn safe_file_stem(stem: &str) -> String {
    super::guard_reserved_name(&safe_stem(stem))
}
