//! Pasting and dropping images into a Markdown file (ADR-035).
//!
//! The clipboard is read here, never in the webview, so an image crosses IPC
//! only as the Markdown that refers to it. Each file is typed by its leading
//! bytes, capped, and either referred to where it is (inside the note's
//! folder) or copied into the configured image folder through the atomic
//! write, stamped for the watcher first.

use std::io::Read;
use std::path::{Path, PathBuf};

use chrono::NaiveDateTime;
use jotra_core::buffer::BufferStatus;
use jotra_core::config::ImageFolder;
use jotra_core::notes::image_import::{
    check_image_head, check_image_size, clipboard_image_name, image_destination,
    image_folder_label, image_markdown, is_lone_web_address, not_images_notice, place_image_file,
    saved_image_reference, ImageImportError, ImagePlacement, IMAGE_MAX_BYTES, IMAGE_MAX_MB,
};
use jotra_core::preview::protocol::SNIFF_BYTES;
use jotra_storage::image_store::{make_folder_within, save_image, ImageSaveError};
use jotra_storage::paths::strip_verbatim_prefix;
use serde::Serialize;
use tauri::{AppHandle, Runtime};
use tracing::{info, warn};

use crate::commands::buffer::{ignore_stamper, ignore_unstamper};
use crate::commands::offload::{run_blocking, ERR_COMMAND_STOPPED};
use crate::poison::recover_poison;
use crate::state::AppState;

/// Failure code when the tab named is not open.
pub const ERR_TAB_NOT_OPEN: &str = "ERR_TAB_NOT_OPEN";

/// The JPEG quality an opaque clipboard picture is saved at when its PNG is
/// over the cap (ADR-035).
const JPEG_QUALITY: u8 = 90;

/// What a refusal calls a clipboard picture, which has no name of its own.
const COPIED_PICTURE: &str = "the copied picture";

/// What the system clipboard holds, in the order a paste looks for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardContents {
    /// Files copied in a file manager.
    Files(Vec<PathBuf>),
    /// Plain text and no file list.
    Text,
    /// A picture, with no text beside it or one web address.
    Image(ClipboardPicture),
    /// Nothing a paste can use.
    Empty,
}

/// A picture on the clipboard: `width * height` RGBA pixels, row by row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardPicture {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl ClipboardPicture {
    /// A picture of `width` by `height` pixels from its RGBA bytes.
    ///
    /// # Errors
    ///
    /// [`PictureEncodeError::WrongLength`] when `rgba` is not
    /// `width * height * 4` bytes, a side is zero, or a side is over `u32`.
    pub fn new(width: usize, height: usize, rgba: Vec<u8>) -> Result<Self, PictureEncodeError> {
        let (width, height) = rgba_sides(width, height, rgba.len())?;
        Ok(Self {
            width,
            height,
            rgba,
        })
    }

    fn is_opaque(&self) -> bool {
        self.rgba
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[3] == u8::MAX)
    }
}

/// A clipboard picture as the bytes a save writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncodedPicture {
    /// The encoded file.
    pub bytes: Vec<u8>,
    /// The extension the file takes, without the dot.
    pub extension: &'static str,
}

/// What a paste takes from a clipboard holding `files`, `text` and, read only
/// when it is wanted, a picture.
///
/// Copied files come first, then text, except that a picture beside nothing
/// but one web address is the picture: a browser's Copy Image puts both.
pub fn choose_clipboard_contents(
    files: Vec<PathBuf>,
    text: &str,
    picture: impl FnOnce() -> Option<ClipboardPicture>,
) -> ClipboardContents {
    if !files.is_empty() {
        return ClipboardContents::Files(files);
    }
    if text.is_empty() {
        return picture().map_or(ClipboardContents::Empty, ClipboardContents::Image);
    }
    if is_lone_web_address(text) {
        return picture().map_or(ClipboardContents::Text, ClipboardContents::Image);
    }
    ClipboardContents::Text
}

/// Reads the clipboard. The system one in the app, a fixed one in tests.
pub trait ClipboardReader {
    /// What the clipboard holds now.
    fn read_contents(&self) -> ClipboardContents;
}

/// The system clipboard.
pub struct SystemClipboard;

impl ClipboardReader for SystemClipboard {
    fn read_contents(&self) -> ClipboardContents {
        let mut clipboard = match arboard::Clipboard::new() {
            Ok(clipboard) => clipboard,
            Err(error) => {
                warn!(%error, "the clipboard could not be opened");
                return ClipboardContents::Empty;
            }
        };
        let files = clipboard.get().file_list().unwrap_or_default();
        let text = clipboard.get_text().unwrap_or_default();
        choose_clipboard_contents(files, &text, || {
            let image = clipboard.get_image().ok()?;
            ClipboardPicture::new(image.width, image.height, image.bytes.into_owned())
                .inspect_err(|error| warn!(%error, "the clipboard picture could not be read"))
                .ok()
        })
    }
}

/// Why a picture could not be encoded.
#[derive(Debug, thiserror::Error)]
pub enum PictureEncodeError {
    /// The pixel buffer does not hold `width * height` RGBA pixels.
    #[error("{len} bytes do not make a {width}x{height} RGBA picture")]
    WrongLength {
        /// Width in pixels.
        width: usize,
        /// Height in pixels.
        height: usize,
        /// Bytes given.
        len: usize,
    },
    /// The encoder failed.
    #[error("png encoding failed: {0}")]
    Encoder(#[from] png::EncodingError),
    /// The picture is over the cap as PNG and, when it has no transparency,
    /// as JPEG.
    #[error("the picture is over the size cap")]
    TooLarge,
}

/// The sides of a `width` by `height` RGBA picture held in `len` bytes.
fn rgba_sides(width: usize, height: usize, len: usize) -> Result<(u32, u32), PictureEncodeError> {
    let wrong = || PictureEncodeError::WrongLength { width, height, len };
    let expected = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(wrong)?;
    if expected != len || width == 0 || height == 0 {
        return Err(wrong());
    }
    let w = u32::try_from(width).map_err(|_| wrong())?;
    let h = u32::try_from(height).map_err(|_| wrong())?;
    Ok((w, h))
}

/// Encodes an RGBA picture as PNG.
///
/// # Errors
///
/// [`PictureEncodeError::WrongLength`] when `rgba` is not
/// `width * height * 4` bytes or a side is over `u32`,
/// [`PictureEncodeError::Encoder`] when the encoder fails.
pub fn encode_png(width: usize, height: usize, rgba: &[u8]) -> Result<Vec<u8>, PictureEncodeError> {
    let (w, h) = rgba_sides(width, height, rgba.len())?;
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, w, h);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(rgba)?;
        writer.finish()?;
    }
    Ok(out)
}

/// Encodes a clipboard picture as PNG, or as JPEG at [`JPEG_QUALITY`] when
/// the PNG is over `max_bytes` and every pixel is opaque (ADR-035).
///
/// # Errors
///
/// [`PictureEncodeError::TooLarge`] when the PNG is over `max_bytes` and the
/// picture has transparency, or its JPEG is over `max_bytes` too;
/// [`PictureEncodeError::Encoder`] when the PNG encoder fails.
pub fn encode_clipboard_picture(
    picture: &ClipboardPicture,
    max_bytes: u64,
) -> Result<EncodedPicture, PictureEncodeError> {
    let png = encode_png(
        picture.width as usize,
        picture.height as usize,
        &picture.rgba,
    )?;
    if png.len() as u64 <= max_bytes {
        return Ok(EncodedPicture {
            bytes: png,
            extension: "png",
        });
    }
    if !picture.is_opaque() {
        return Err(PictureEncodeError::TooLarge);
    }
    match encode_jpeg(picture) {
        Ok(jpeg) if jpeg.len() as u64 <= max_bytes => Ok(EncodedPicture {
            bytes: jpeg,
            extension: "jpg",
        }),
        Ok(_) => Err(PictureEncodeError::TooLarge),
        Err(error) => {
            warn!(%error, "the clipboard picture could not be encoded as jpeg");
            Err(PictureEncodeError::TooLarge)
        }
    }
}

fn encode_jpeg(picture: &ClipboardPicture) -> Result<Vec<u8>, image::ImageError> {
    let rgb: Vec<u8> = picture
        .rgba
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|&[red, green, blue, _]| [red, green, blue])
        .collect();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY).encode(
        &rgb,
        picture.width,
        picture.height,
        image::ExtendedColorType::Rgb8,
    )?;
    Ok(out)
}

/// Outcome of a paste into a Markdown file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ClipboardPasteResult {
    /// Images were taken: insert `markdown` at the cursor. `notices` name the
    /// files that were not.
    Inserted {
        /// One image reference per line.
        markdown: String,
        /// One message per file not taken.
        notices: Vec<String>,
    },
    /// The clipboard held images and none was taken.
    Refused {
        /// One message per image not taken.
        notices: Vec<String>,
    },
    /// The clipboard holds text, which pastes as text.
    Text,
    /// The clipboard holds nothing to paste.
    Empty,
}

/// Outcome of files dropped on a Markdown file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct DroppedImagesResult {
    /// One image reference per line; empty when no image was taken.
    pub markdown: String,
    /// The dropped paths that are not images, to open as tabs.
    pub others: Vec<String>,
    /// One message per image not taken.
    pub notices: Vec<String>,
}

/// Paste the images on the clipboard into the Markdown file `buffer_id`.
#[tauri::command]
pub async fn paste_clipboard_images<R: Runtime>(
    app: AppHandle<R>,
    buffer_id: String,
) -> Result<ClipboardPasteResult, String> {
    run_blocking(app, ERR_COMMAND_STOPPED, move |state| {
        paste_clipboard_images_inner(
            state,
            &buffer_id,
            &SystemClipboard,
            chrono::Local::now().naive_local(),
        )
    })
    .await
}

/// Take the images among `paths`, dropped on the Markdown file `buffer_id`.
///
/// Only paths the drop itself authorized are read; each image spends its
/// authorization, and every other path keeps it for the open that follows.
#[tauri::command]
pub async fn import_dropped_images<R: Runtime>(
    app: AppHandle<R>,
    buffer_id: String,
    paths: Vec<String>,
) -> Result<DroppedImagesResult, String> {
    run_blocking(app, ERR_COMMAND_STOPPED, move |state| {
        import_dropped_images_inner(state, &buffer_id, &paths)
    })
    .await
}

/// [`paste_clipboard_images`] without Tauri, with the clipboard and the
/// clock injected.
///
/// # Errors
///
/// [`ERR_TAB_NOT_OPEN`] when no open tab is `buffer_id`; a lock or database
/// failure as its message.
pub fn paste_clipboard_images_inner(
    state: &AppState,
    buffer_id: &str,
    clipboard: &dyn ClipboardReader,
    now: NaiveDateTime,
) -> Result<ClipboardPasteResult, String> {
    match clipboard.read_contents() {
        ClipboardContents::Empty => Ok(ClipboardPasteResult::Empty),
        ClipboardContents::Text => Ok(ClipboardPasteResult::Text),
        ClipboardContents::Files(files) => {
            let copied = files.len();
            let images: Vec<PathBuf> = files.into_iter().filter(|file| is_image(file)).collect();
            if images.is_empty() {
                return Ok(ClipboardPasteResult::Text);
            }
            let Some(note) = note_target(state, buffer_id)? else {
                return Ok(no_file_on_disk());
            };
            let mut taken = Vec::new();
            let mut notices = Vec::new();
            for file in &images {
                match import_image_file(state, &note, file) {
                    Ok(reference) => taken.push(reference),
                    Err(err) => notices.push(err.to_string()),
                }
            }
            notices.extend(not_images_notice(copied - images.len()));
            Ok(paste_answer(buffer_id, taken, notices))
        }
        ClipboardContents::Image(picture) => {
            let Some(note) = note_target(state, buffer_id)? else {
                return Ok(no_file_on_disk());
            };
            let encoded = match encode_clipboard_picture(&picture, IMAGE_MAX_BYTES) {
                Ok(encoded) => encoded,
                Err(error) => {
                    let refusal = match error {
                        PictureEncodeError::TooLarge => ImageImportError::PictureTooLarge {
                            limit_mb: IMAGE_MAX_MB,
                        },
                        other => {
                            warn!(error = %other, "the clipboard picture could not be encoded");
                            ImageImportError::Unreadable {
                                name: COPIED_PICTURE.to_string(),
                            }
                        }
                    };
                    return Ok(paste_answer(
                        buffer_id,
                        Vec::new(),
                        vec![refusal.to_string()],
                    ));
                }
            };
            let name = clipboard_image_name(&note.stem, now, encoded.extension);
            Ok(
                match import_image_bytes(state, &note, &name, &encoded.bytes) {
                    Ok(reference) => paste_answer(buffer_id, vec![reference], Vec::new()),
                    Err(err) => paste_answer(buffer_id, Vec::new(), vec![err.to_string()]),
                },
            )
        }
    }
}

/// [`import_dropped_images`] without Tauri.
///
/// # Errors
///
/// [`ERR_TAB_NOT_OPEN`] when no open tab is `buffer_id`; a lock or database
/// failure as its message.
pub fn import_dropped_images_inner(
    state: &AppState,
    buffer_id: &str,
    paths: &[String],
) -> Result<DroppedImagesResult, String> {
    let mut images = Vec::new();
    let mut result = DroppedImagesResult::default();
    for path in paths {
        if state.authorized_paths.is_pending_open(path) && is_image(Path::new(path)) {
            images.push(path);
        } else {
            result.others.push(path.clone());
        }
    }
    if images.is_empty() {
        return Ok(result);
    }
    let note = note_target(state, buffer_id)?;
    let mut taken = Vec::new();
    for path in images {
        state.authorized_paths.consume_for_open(path);
        let Some(note) = note.as_ref() else {
            continue;
        };
        match import_image_file(state, note, Path::new(path)) {
            Ok(reference) => taken.push(reference),
            Err(err) => result.notices.push(err.to_string()),
        }
    }
    if note.is_none() {
        result
            .notices
            .push(ImageImportError::NoFileOnDisk.to_string());
    }
    info!(
        buffer_id,
        taken = taken.len(),
        others = result.others.len(),
        "images dropped"
    );
    result.markdown = references_markdown(&taken);
    Ok(result)
}

/// The file a paste or drop lands beside, and the folder its images go in.
struct NoteTarget {
    dir: PathBuf,
    stem: String,
    folder: ImageFolder,
}

/// The note `buffer_id` is, or `None` for a tab with no file on disk.
fn note_target(state: &AppState, buffer_id: &str) -> Result<Option<NoteTarget>, String> {
    let row = {
        let store = state.store.lock().map_err(|e| e.to_string())?;
        store.find(buffer_id).map_err(|e| e.to_string())?
    };
    let Some(row) = row.filter(|row| row.status == BufferStatus::Active) else {
        warn!(buffer_id, "image import for a tab that is not open");
        return Err(ERR_TAB_NOT_OPEN.to_string());
    };
    let Some(source) = row.source_path else {
        return Ok(None);
    };
    let path = PathBuf::from(source);
    let Some(parent) = path.parent() else {
        return Ok(None);
    };
    let dir = canonical(parent).unwrap_or_else(|| parent.to_path_buf());
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let folder = recover_poison(state.config.lock(), "commands::images::note_target")
        .files
        .image_folder
        .clone();
    Ok(Some(NoteTarget { dir, stem, folder }))
}

fn no_file_on_disk() -> ClipboardPasteResult {
    ClipboardPasteResult::Refused {
        notices: vec![ImageImportError::NoFileOnDisk.to_string()],
    }
}

fn paste_answer(buffer_id: &str, taken: Vec<String>, notices: Vec<String>) -> ClipboardPasteResult {
    info!(
        buffer_id,
        taken = taken.len(),
        not_taken = notices.len(),
        "images pasted"
    );
    if taken.is_empty() {
        ClipboardPasteResult::Refused { notices }
    } else {
        ClipboardPasteResult::Inserted {
            markdown: references_markdown(&taken),
            notices,
        }
    }
}

fn references_markdown(references: &[String]) -> String {
    references
        .iter()
        .map(|reference| image_markdown(reference))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether `path` is a regular file whose leading bytes are an image.
fn is_image(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    metadata.is_file()
        && read_head(path)
            .ok()
            .is_some_and(|head| check_image_head("", &head).is_ok())
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

fn canonical(path: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(path).ok().map(strip_verbatim_prefix)
}

/// Take one image file: refer to it where it is when it sits inside the
/// note's folder, or copy it into the image folder.
fn import_image_file(
    state: &AppState,
    note: &NoteTarget,
    file: &Path,
) -> Result<String, ImageImportError> {
    let name = display_name(file);
    let unreadable = || ImageImportError::Unreadable { name: name.clone() };
    let metadata = std::fs::metadata(file).map_err(|_| unreadable())?;
    check_image_size(&name, metadata.len())?;
    let head = read_head(file).map_err(|_| unreadable())?;
    let mime = check_image_head(&name, &head)?;
    let file = canonical(file).ok_or_else(unreadable)?;
    match place_image_file(&note.dir, &note.folder, &file, mime) {
        ImagePlacement::InPlace { reference } => Ok(reference),
        ImagePlacement::Copy {
            name: copy_name, ..
        } => {
            let bytes = read_capped(&file).map_err(|_| unreadable())?;
            check_image_size(&name, bytes.len() as u64)?;
            check_image_head(&name, &bytes)?;
            save_into_folder(state, note, &copy_name, &bytes)
        }
    }
}

/// Take a picture held in memory, saved as `name`.
fn import_image_bytes(
    state: &AppState,
    note: &NoteTarget,
    name: &str,
    bytes: &[u8],
) -> Result<String, ImageImportError> {
    check_image_size(name, bytes.len() as u64)?;
    check_image_head(name, bytes)?;
    save_into_folder(state, note, name, bytes)
}

fn save_into_folder(
    state: &AppState,
    note: &NoteTarget,
    name: &str,
    bytes: &[u8],
) -> Result<String, ImageImportError> {
    let destination = image_destination(&note.dir, &note.folder);
    let stamp = ignore_stamper(state);
    let unstamp = ignore_unstamper(state);
    let saved = make_folder_within(&note.dir, &destination).and_then(|resolved| {
        save_image(
            &resolved,
            name,
            bytes,
            |path: &Path| stamp(path, bytes),
            |path: &Path| unstamp(path),
        )
    });
    match saved {
        Ok(saved) => Ok(saved_image_reference(&note.folder, saved.name())),
        Err(error) => {
            let folder = image_folder_label(&note.folder);
            warn!(%error, folder = %folder, "image could not be saved");
            Err(match error {
                ImageSaveError::NoFreeName(_) => ImageImportError::NoFreeName {
                    name: name.to_string(),
                    folder,
                },
                ImageSaveError::OutsideRoot => ImageImportError::FolderOutside { folder },
                ImageSaveError::FolderNotWritable | ImageSaveError::Io(_) => {
                    ImageImportError::FolderNotWritable { folder }
                }
            })
        }
    }
}

/// The first [`SNIFF_BYTES`] of a file.
fn read_head(path: &Path) -> std::io::Result<Vec<u8>> {
    let mut head = Vec::with_capacity(SNIFF_BYTES);
    std::fs::File::open(path)?
        .take(SNIFF_BYTES as u64)
        .read_to_end(&mut head)?;
    Ok(head)
}

/// The whole file, read no further than one byte past the cap so a file that
/// grew since its stat is still caught.
fn read_capped(path: &Path) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(IMAGE_MAX_BYTES + 1)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_picture_encodes_to_a_png_the_sniffer_accepts() {
        let rgba = [255u8, 0, 0, 255, 0, 255, 0, 128];
        let png = encode_png(2, 1, &rgba).unwrap();
        assert_eq!(check_image_head("x.png", &png), Ok("image/png"));
    }

    #[test]
    fn a_buffer_of_the_wrong_length_is_not_encoded() {
        assert!(matches!(
            encode_png(2, 2, &[0u8; 8]),
            Err(PictureEncodeError::WrongLength { len: 8, .. })
        ));
        assert!(encode_png(0, 0, &[]).is_err());
        assert!(encode_png(usize::MAX, 2, &[0u8; 4]).is_err());
        assert!(ClipboardPicture::new(2, 2, vec![0u8; 8]).is_err());
        assert!(ClipboardPicture::new(0, 0, Vec::new()).is_err());
    }

    /// A picture that compresses as a photo does: a gradient under a little
    /// noise, every pixel as opaque as `alpha` says.
    fn photo(alpha: impl Fn(usize) -> u8) -> ClipboardPicture {
        let (width, height) = (96, 64);
        let mut seed: u32 = 0x9e37_79b9;
        let mut rgba = Vec::with_capacity(width * height * 4);
        for pixel in 0..width * height {
            let (x, y) = (pixel % width, pixel / width);
            for channel in 0..3 {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                let base = (x + y + channel * 40) as u32;
                rgba.push(((base + (seed >> 28)) % 256) as u8);
            }
            rgba.push(alpha(pixel));
        }
        ClipboardPicture::new(width, height, rgba).unwrap()
    }

    fn png_len(picture: &ClipboardPicture) -> u64 {
        encode_clipboard_picture(picture, u64::MAX)
            .unwrap()
            .bytes
            .len() as u64
    }

    #[test]
    fn a_picture_within_the_cap_is_saved_as_png() {
        let picture = photo(|_| 255);
        let cap = png_len(&picture);
        let encoded = encode_clipboard_picture(&picture, cap).unwrap();
        assert_eq!(encoded.extension, "png");
        assert_eq!(check_image_head("x.png", &encoded.bytes), Ok("image/png"));
    }

    #[test]
    fn an_opaque_picture_over_the_cap_is_saved_as_jpeg() {
        let picture = photo(|_| 255);
        let cap = png_len(&picture) - 1;
        let encoded = encode_clipboard_picture(&picture, cap).unwrap();
        assert_eq!(encoded.extension, "jpg");
        assert!(encoded.bytes.len() as u64 <= cap);
        assert_eq!(check_image_head("x.jpg", &encoded.bytes), Ok("image/jpeg"));
    }

    #[test]
    fn a_picture_with_transparency_over_the_cap_is_refused() {
        let picture = photo(|pixel| if pixel == 7 { 254 } else { 255 });
        let cap = png_len(&picture) - 1;
        assert!(matches!(
            encode_clipboard_picture(&picture, cap),
            Err(PictureEncodeError::TooLarge)
        ));
    }

    #[test]
    fn an_opaque_picture_over_the_cap_even_as_jpeg_is_refused() {
        assert!(matches!(
            encode_clipboard_picture(&photo(|_| 255), 64),
            Err(PictureEncodeError::TooLarge)
        ));
    }

    fn small() -> ClipboardPicture {
        ClipboardPicture::new(1, 1, vec![1, 2, 3, 255]).unwrap()
    }

    #[test]
    fn copied_files_come_before_text_and_pictures() {
        let files = vec![PathBuf::from("/a.png")];
        assert_eq!(
            choose_clipboard_contents(files.clone(), "https://example.com/a.png", || Some(small())),
            ClipboardContents::Files(files)
        );
    }

    #[test]
    fn a_picture_beside_a_lone_web_address_is_the_picture() {
        assert_eq!(
            choose_clipboard_contents(Vec::new(), "https://example.com/a.png\n", || Some(small())),
            ClipboardContents::Image(small())
        );
        assert_eq!(
            choose_clipboard_contents(Vec::new(), "https://example.com/a.png", || None),
            ClipboardContents::Text
        );
    }

    #[test]
    fn other_text_beside_a_picture_pastes_as_text_without_reading_the_picture() {
        let read = std::cell::Cell::new(false);
        let contents = choose_clipboard_contents(Vec::new(), "a caption", || {
            read.set(true);
            Some(small())
        });
        assert_eq!(contents, ClipboardContents::Text);
        assert!(!read.get());
    }

    #[test]
    fn a_picture_alone_is_the_picture_and_nothing_is_empty() {
        assert_eq!(
            choose_clipboard_contents(Vec::new(), "", || Some(small())),
            ClipboardContents::Image(small())
        );
        assert_eq!(
            choose_clipboard_contents(Vec::new(), "", || None),
            ClipboardContents::Empty
        );
    }
}
