//! The desktop extension bundle Claude Desktop installs Writ from
//! (`writ_core::mcp_clients`, ADR-031 rule 2.4).
//!
//! A zip holding `manifest.json` and the launcher, the layout `mcpb pack`
//! writes. It is written into Writ's data folder and handed to the system,
//! which opens it in Claude Desktop; Claude Desktop installs it into its own
//! folder, so Writ writes nothing of Claude Desktop's.

use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use writ_core::mcp_clients::{BUNDLE_FILE_NAME, BUNDLE_LAUNCHER_PATH};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::errors::StorageResult;

/// The folder inside the data folder the bundle is written to.
pub const FOLDER: &str = "claude-desktop";

/// Where the bundle is written inside `dir`, the data folder.
pub fn path(dir: &Path) -> PathBuf {
    dir.join(FOLDER).join(BUNDLE_FILE_NAME)
}

/// Writes the bundle for `manifest` and `launcher` into `dir`, replacing the
/// one an earlier click wrote, and answers its path.
pub fn write_bundle(dir: &Path, manifest: &str, launcher: &str) -> StorageResult<PathBuf> {
    let target = path(dir);
    if let Some(folder) = target.parent() {
        std::fs::create_dir_all(folder)?;
    }
    let bytes = bundle_bytes(manifest, launcher)?;
    crate::atomic::write_atomic(&target, &bytes)?;
    Ok(target)
}

/// The bundle's bytes: `manifest.json` and the launcher, stored uncompressed.
pub fn bundle_bytes(manifest: &str, launcher: &str) -> StorageResult<Vec<u8>> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o644);
    for (name, text) in [
        ("manifest.json", manifest),
        (BUNDLE_LAUNCHER_PATH, launcher),
    ] {
        writer.start_file(name, options).map_err(archive_error)?;
        writer.write_all(text.as_bytes())?;
    }
    let cursor = writer.finish().map_err(archive_error)?;
    Ok(cursor.into_inner())
}

fn archive_error(error: zip::result::ZipError) -> std::io::Error {
    std::io::Error::other(error)
}
