//! `[files]` configuration section.
//!
//! The extension every file Writ mints carries (ADR-041 §2). One rule for a
//! new file, Today's Note, the file the `writ` command writes from piped
//! stdin, the file a connected program creates, and the file the first launch
//! opens. A file keeps its own extension for the rest of its life, so this is
//! read at the mint and nowhere else.

use serde::{Deserialize, Serialize};

/// The extension a file Writ mints carries.
///
/// Two formats, because these are the two the editor has something to say
/// about: Markdown is rendered, linked and graphed, and plain text is the
/// format that opens anywhere. A file with any other extension is opened and
/// edited like every other text file; it is simply not one Writ makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum FileExtension {
    /// Plain text, `.txt`.
    #[default]
    Txt,
    /// Markdown, `.md`.
    Md,
}

impl FileExtension {
    /// The extension as a file name carries it, without the dot.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Txt => "txt",
            Self::Md => "md",
        }
    }
}

/// Where a pasted or dropped image is saved, relative to the folder of the
/// Markdown file it goes into.
///
/// Forward slashes separate its parts on every platform. Empty means the
/// note's own folder. A value that leaves that folder, an absolute path or
/// one with a `..` part, is refused, and so is a part no platform can name a
/// folder, so the lenient read (ADR-048) names it and runs on the default.
/// `src/lib/image-folder.ts` applies the same rules to the settings field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ImageFolder(String);

/// The folder images go to when the config names none.
pub const DEFAULT_IMAGE_FOLDER: &str = "assets";

impl ImageFolder {
    /// Reads `value` as an image folder, trimming surrounding whitespace and
    /// slashes and turning backslashes into forward slashes.
    ///
    /// # Errors
    ///
    /// [`ImageFolderError::Absolute`] for a path from the root or a drive,
    /// [`ImageFolderError::LeavesTheFolder`] for a `.` or `..` part, and
    /// [`ImageFolderError::NotAName`] for a part holding a control character
    /// or one of `< > : " | ? *`, ending in a dot or a space, or spelling a
    /// reserved device name such as `CON` or `COM1`.
    pub fn parse(value: &str) -> Result<Self, ImageFolderError> {
        let unified = value.trim().replace('\\', "/");
        let mut leading = unified.chars();
        let is_drive = matches!(
            (leading.next(), leading.next()),
            (Some(letter), Some(':')) if letter.is_ascii_alphabetic()
        );
        if unified.starts_with('/') || is_drive {
            return Err(ImageFolderError::Absolute);
        }
        let parts: Vec<&str> = unified.split('/').filter(|part| !part.is_empty()).collect();
        if parts.iter().any(|part| *part == ".." || *part == ".") {
            return Err(ImageFolderError::LeavesTheFolder);
        }
        if !parts.iter().all(|part| is_folder_name(part)) {
            return Err(ImageFolderError::NotAName);
        }
        Ok(Self(parts.join("/")))
    }

    /// The folder as written in a link: forward slashes, no leading or
    /// trailing one, and empty for the note's own folder.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The folder's parts, outermost first.
    pub fn parts(&self) -> impl Iterator<Item = &str> {
        self.0.split('/').filter(|part| !part.is_empty())
    }
}

impl Default for ImageFolder {
    fn default() -> Self {
        Self(DEFAULT_IMAGE_FOLDER.to_string())
    }
}

impl<'de> Deserialize<'de> for ImageFolder {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

fn is_folder_name(part: &str) -> bool {
    let head = part.split('.').next().unwrap_or(part);
    !part
        .chars()
        .any(|c| c.is_control() || crate::notes::ILLEGAL_CHARS.contains(&c))
        && !part.ends_with(['.', ' '])
        && !crate::notes::is_reserved_device_name(head)
}

/// Why a value is not an [`ImageFolder`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ImageFolderError {
    /// The value is an absolute path.
    #[error("image_folder must be relative to the file's folder, not an absolute path")]
    Absolute,
    /// The value has a `.` or `..` part.
    #[error("image_folder cannot have . or .. parts")]
    LeavesTheFolder,
    /// A part is not a name every platform gives a folder.
    #[error(
        "image_folder parts cannot hold control characters or < > : \" | ? *, end in a dot or a space, or be a device name such as CON"
    )]
    NotAName,
}

impl ImageFolderError {
    /// The refusal as `src/lib/image-folder.ts` names it.
    pub fn code(self) -> &'static str {
        match self {
            Self::Absolute => "absolute",
            Self::LeavesTheFolder => "leaves_the_folder",
            Self::NotAName => "not_a_name",
        }
    }
}

/// Files configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct FilesConfig {
    /// The extension every file Writ mints carries.
    #[serde(default)]
    pub default_extension: FileExtension,
    /// Where a pasted or dropped image is saved, relative to the note's
    /// folder.
    #[serde(default)]
    pub image_folder: ImageFolder,
}

impl FilesConfig {
    /// Reads the `[files]` table out of a whole `config.toml` text.
    ///
    /// Deliberately tolerant: a missing table, a value neither format names,
    /// and a file nothing can parse all yield the default. This is for the
    /// consumers that hold no [`crate::config::WritConfig`] — the `writ`
    /// command and the MCP server — where refusing to mint a file because
    /// somebody hand-edited the config would lose the text the caller is
    /// holding.
    pub fn from_config_text(text: &str) -> Self {
        let Ok(document) = text.parse::<toml::Table>() else {
            return Self::default();
        };
        let Some(files) = document.get("files").and_then(toml::Value::as_table) else {
            return Self::default();
        };
        // Each key on its own, so a bad value in one leaves the other read.
        Self {
            default_extension: files
                .get("default_extension")
                .and_then(|value| value.clone().try_into().ok())
                .unwrap_or_default(),
            image_folder: files
                .get("image_folder")
                .and_then(|value| value.clone().try_into().ok())
                .unwrap_or_default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_the_default() {
        assert_eq!(FilesConfig::default().default_extension, FileExtension::Txt);
        assert_eq!(FileExtension::default().as_str(), "txt");
    }

    #[test]
    fn empty_table_yields_defaults() {
        let c: FilesConfig = toml::from_str("").unwrap();
        assert_eq!(c, FilesConfig::default());
    }

    #[test]
    fn markdown_is_named_the_way_a_file_name_spells_it() {
        let c: FilesConfig = toml::from_str("default_extension = \"md\"").unwrap();
        assert_eq!(c.default_extension, FileExtension::Md);
        assert_eq!(c.default_extension.as_str(), "md");
    }

    #[test]
    fn a_format_neither_name_names_is_refused() {
        assert!(toml::from_str::<FilesConfig>("default_extension = \"rtf\"").is_err());
        assert!(toml::from_str::<FilesConfig>("default_extension = \"markdown\"").is_err());
    }

    #[test]
    fn round_trips_through_toml() {
        let c = FilesConfig {
            default_extension: FileExtension::Md,
            image_folder: ImageFolder::parse("media/shots").unwrap(),
        };
        let back: FilesConfig = toml::from_str(&toml::to_string(&c).unwrap()).unwrap();
        assert_eq!(c, back);
    }

    #[test]
    fn a_config_text_without_the_table_reads_as_the_default() {
        assert_eq!(
            FilesConfig::from_config_text("[notes]\nroot = \"~/Writ\"\n"),
            FilesConfig::default()
        );
        assert_eq!(FilesConfig::from_config_text(""), FilesConfig::default());
    }

    #[test]
    fn a_config_text_nothing_can_parse_reads_as_the_default() {
        assert_eq!(
            FilesConfig::from_config_text("[files\ndefault_extension ="),
            FilesConfig::default()
        );
        assert_eq!(
            FilesConfig::from_config_text("default_extension = \"rtf\""),
            FilesConfig::default()
        );
    }

    #[test]
    fn a_config_text_naming_a_format_reads_it() {
        assert_eq!(
            FilesConfig::from_config_text(
                "[notes]\nroot = \"~/Writ\"\n\n[files]\ndefault_extension = \"md\"\n"
            )
            .default_extension,
            FileExtension::Md
        );
    }

    #[test]
    fn a_format_the_table_does_not_name_reads_as_the_default() {
        assert_eq!(
            FilesConfig::from_config_text("[files]\ndefault_extension = \"rtf\"\n"),
            FilesConfig::default()
        );
    }

    #[test]
    fn images_go_to_assets_when_the_file_does_not_name_a_folder() {
        assert_eq!(FilesConfig::default().image_folder.as_str(), "assets");
        let c: FilesConfig = toml::from_str("default_extension = \"md\"").unwrap();
        assert_eq!(c.image_folder.as_str(), "assets");
    }

    #[test]
    fn an_empty_image_folder_means_beside_the_note() {
        let c: FilesConfig = toml::from_str("image_folder = \"\"").unwrap();
        assert_eq!(c.image_folder.as_str(), "");
        assert_eq!(c.image_folder.parts().count(), 0);
    }

    #[test]
    fn an_image_folder_is_written_with_forward_slashes_and_no_outer_ones() {
        let folder = ImageFolder::parse(" /media\\shots/ ").unwrap_err();
        assert_eq!(folder, ImageFolderError::Absolute);
        let folder = ImageFolder::parse("media\\shots/").unwrap();
        assert_eq!(folder.as_str(), "media/shots");
        assert_eq!(folder.parts().collect::<Vec<_>>(), ["media", "shots"]);
        assert_eq!(ImageFolder::parse("a//b").unwrap().as_str(), "a/b");
    }

    #[test]
    fn an_image_folder_outside_the_notes_folder_is_refused() {
        for value in [
            "/tmp/images",
            "C:\\images",
            "c:/images",
            "\\\\server\\share",
        ] {
            assert_eq!(
                ImageFolder::parse(value),
                Err(ImageFolderError::Absolute),
                "{value}"
            );
        }
        for value in ["../images", "assets/../..", "./assets", "a/./b"] {
            assert_eq!(
                ImageFolder::parse(value),
                Err(ImageFolderError::LeavesTheFolder),
                "{value}"
            );
        }
        assert!(toml::from_str::<FilesConfig>("image_folder = \"../up\"").is_err());
    }

    #[test]
    fn a_bad_image_folder_leaves_the_format_read_from_config_text() {
        let c = FilesConfig::from_config_text(
            "[files]\ndefault_extension = \"md\"\nimage_folder = \"/abs\"\n",
        );
        assert_eq!(c.default_extension, FileExtension::Md);
        assert_eq!(c.image_folder, ImageFolder::default());
    }

    #[test]
    fn a_bad_image_folder_is_named_by_the_lenient_read_and_falls_back() {
        let read = crate::config::WritConfig::parse_lenient(
            "[files]\ndefault_extension = \"md\"\nimage_folder = \"../up\"\n",
        );
        assert_eq!(read.config.files.default_extension, FileExtension::Md);
        assert_eq!(read.config.files.image_folder.as_str(), "assets");
        assert_eq!(read.diagnostics.len(), 1);
        assert_eq!(read.diagnostics[0].key, "files.image_folder");
        assert_eq!(read.diagnostics[0].line, 3);
    }
}
