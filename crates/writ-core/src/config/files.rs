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

    /// The extension `value` names, or `None` when it names neither.
    ///
    /// Case-insensitive and without the dot, which is how the two are written
    /// in the config file and how a file name carries them.
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        if value.eq_ignore_ascii_case("txt") {
            Some(Self::Txt)
        } else if value.eq_ignore_ascii_case("md") {
            Some(Self::Md)
        } else {
            None
        }
    }
}

/// Files configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct FilesConfig {
    /// The extension every file Writ mints carries.
    #[serde(default)]
    pub default_extension: FileExtension,
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
        let Some(files) = document.get("files") else {
            return Self::default();
        };
        files.clone().try_into().unwrap_or_default()
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
        assert_eq!(FileExtension::parse("rtf"), None);
        assert_eq!(FileExtension::parse("markdown"), None);
    }

    #[test]
    fn parsing_a_name_ignores_case_and_surrounding_space() {
        assert_eq!(FileExtension::parse("TXT"), Some(FileExtension::Txt));
        assert_eq!(FileExtension::parse(" Md "), Some(FileExtension::Md));
    }

    #[test]
    fn round_trips_through_toml() {
        let c = FilesConfig {
            default_extension: FileExtension::Md,
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
}
