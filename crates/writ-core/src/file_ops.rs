//! Pure file helpers: classification, language detection, filename extraction,
//! and hex-dump generation.

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::errors::{WritError, WritResult};

/// Files up to this size get the full feature set.
pub const THRESHOLD_NORMAL_BYTES: u64 = 5 * 1024 * 1024;

/// Files up to this size open in large-file mode (syntax / typography / wrap
/// disabled, FTS skipped, snapshots excluded).
pub const THRESHOLD_LARGE_BYTES: u64 = 50 * 1024 * 1024;

/// Files up to this size open in large-file mode after a confirm dialog.
/// Files above this are refused outright.
pub const THRESHOLD_MAX_BYTES: u64 = 500 * 1024 * 1024;

/// How many bytes to read when sniffing for NUL bytes (binary heuristic).
pub const BINARY_CHECK_BYTES: usize = 8192;

/// How many bytes of a binary file are hex-dumped. Content beyond this
/// limit is replaced with a truncation notice.
pub const HEX_DUMP_MAX_BYTES: usize = 10 * 1024 * 1024;

/// The autosave debounce in milliseconds for large-file buffers.
pub const LARGE_FILE_AUTOSAVE_DEBOUNCE_MS: u64 = 2000;

/// How the file should be opened, determined by size and binary content.
///
/// Returned by [`classify_file`] and [`classify_path`]. The frontend uses
/// this to configure the editor mode and, for [`FileOpenMode::LargeFileConfirm`],
/// to gate opening behind a user confirmation dialog.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind")]
pub enum FileOpenMode {
    /// Full feature set. ≤ [`THRESHOLD_NORMAL_BYTES`].
    Normal,
    /// Syntax, typography, and line-wrap disabled. FTS and snapshots
    /// excluded. `(THRESHOLD_NORMAL_BYTES, THRESHOLD_LARGE_BYTES]`.
    LargeFile,
    /// Same restrictions as [`FileOpenMode::LargeFile`] but the frontend
    /// must confirm before loading. `(THRESHOLD_LARGE_BYTES, THRESHOLD_MAX_BYTES]`.
    LargeFileConfirm,
    /// NUL byte found in first [`BINARY_CHECK_BYTES`]. Opens as read-only
    /// hex dump. Never FTS-indexed, never snapshotted, never saved back.
    Binary,
    /// File exceeds [`THRESHOLD_MAX_BYTES`]. Refused with a message.
    Refused {
        /// Human-readable reason for the refusal.
        reason: String,
    },
}

/// The result of classifying a path without reading its full content.
///
/// Returned by [`classify_path`]; contains both the mode decision and the
/// size so callers do not need a second `metadata()` call.
#[derive(Debug)]
pub struct FileClassification {
    /// How the file should be opened.
    pub mode: FileOpenMode,
    /// File size in bytes.
    pub size_bytes: u64,
}

/// Classifies a file by size and binary-sniff without reading its content.
///
/// Returns [`WritError::Io`] when the path does not exist, is not a regular
/// file, or cannot be read for the binary sniff.
pub fn classify_path(path: &Path) -> WritResult<FileClassification> {
    if !path.exists() {
        return Err(WritError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("file not found: {}", path.display()),
        )));
    }
    if !path.is_file() {
        return Err(WritError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("not a file: {}", path.display()),
        )));
    }

    let meta = std::fs::metadata(path)?;
    let size_bytes = meta.len();

    if size_bytes > THRESHOLD_MAX_BYTES {
        return Ok(FileClassification {
            mode: FileOpenMode::Refused {
                reason: format!(
                    "file is {} — files larger than {} cannot be opened safely",
                    format_bytes(size_bytes),
                    format_bytes(THRESHOLD_MAX_BYTES),
                ),
            },
            size_bytes,
        });
    }

    let is_binary = sniff_binary(path, size_bytes)?;

    Ok(FileClassification {
        mode: classify_file(size_bytes, is_binary),
        size_bytes,
    })
}

/// Pure ladder classification from a known size and binary flag.
///
/// No I/O; safe to call in tests with arbitrary inputs.
pub fn classify_file(size_bytes: u64, is_binary: bool) -> FileOpenMode {
    if is_binary {
        return FileOpenMode::Binary;
    }
    if size_bytes <= THRESHOLD_NORMAL_BYTES {
        FileOpenMode::Normal
    } else if size_bytes <= THRESHOLD_LARGE_BYTES {
        FileOpenMode::LargeFile
    } else {
        FileOpenMode::LargeFileConfirm
    }
}

/// Returns `true` when the first [`BINARY_CHECK_BYTES`] of `path` contain a
/// NUL byte.
///
/// An empty file is never binary. Returns [`WritError::Io`] on read failure.
fn sniff_binary(path: &Path, size_bytes: u64) -> WritResult<bool> {
    if size_bytes == 0 {
        return Ok(false);
    }
    use std::io::Read;
    let file = std::fs::File::open(path)?;
    let mut reader = std::io::BufReader::new(file);
    let read_len = BINARY_CHECK_BYTES.min(size_bytes as usize);
    let mut buf = vec![0u8; read_len];
    reader.read_exact(&mut buf)?;
    Ok(buf.contains(&0u8))
}

/// Formats a byte count as a human-readable string ("1.2 MiB", "300 KiB", …).
fn format_bytes(n: u64) -> String {
    const GIB: u64 = 1024 * 1024 * 1024;
    const MIB: u64 = 1024 * 1024;
    const KIB: u64 = 1024;
    if n >= GIB {
        format!("{:.1} GiB", n as f64 / GIB as f64)
    } else if n >= MIB {
        format!("{:.1} MiB", n as f64 / MIB as f64)
    } else if n >= KIB {
        format!("{:.0} KiB", n as f64 / KIB as f64)
    } else {
        format!("{} B", n)
    }
}

/// Generates a hex dump of `data` in the format:
///
/// ```text
/// 00000000  89 50 4e 47 0d 0a 1a 0a  00 00 00 0d 49 48 44 52  |.PNG........IHDR|
/// ```
///
/// Input is capped at [`HEX_DUMP_MAX_BYTES`]. When `data` was truncated (i.e.
/// the caller passed a slice from a larger source), `total_bytes` should be
/// the original file size so a truncation notice can be appended.
pub fn generate_hex_dump(data: &[u8], total_bytes: usize) -> String {
    let capped = &data[..data.len().min(HEX_DUMP_MAX_BYTES)];
    let row_count = capped.len().div_ceil(16);
    // Each row: 8 (offset) + 2 (spaces) + 3*8 + 1 (mid-gap) + 3*8 (hex) + 2 (spaces) + 1 (|) + 16 (ascii) + 2 (|\n)
    // Rough upper bound: 80 chars × rows, plus truncation notice
    let mut out = String::with_capacity(row_count * 80 + 100);

    for (row_idx, chunk) in capped.chunks(16).enumerate() {
        let offset = row_idx * 16;
        // Offset column
        out.push_str(&format!("{:08x}  ", offset));
        // Hex bytes — two groups of 8, separated by an extra space
        for (i, byte) in chunk.iter().enumerate() {
            if i == 8 {
                out.push(' ');
            }
            out.push_str(&format!("{:02x} ", byte));
        }
        // Pad to full width if chunk < 16
        let pad = 16 - chunk.len();
        for i in 0..pad {
            if chunk.len() + i == 8 {
                out.push(' ');
            }
            out.push_str("   ");
        }
        // ASCII gutter
        out.push(' ');
        out.push('|');
        for byte in chunk {
            out.push(if byte.is_ascii_graphic() || *byte == b' ' {
                *byte as char
            } else {
                '.'
            });
        }
        out.push('|');
        out.push('\n');
    }

    if total_bytes > HEX_DUMP_MAX_BYTES {
        out.push('\n');
        out.push_str(&format!(
            "[ truncated — showing first {} of {} ]",
            format_bytes(HEX_DUMP_MAX_BYTES as u64),
            format_bytes(total_bytes as u64),
        ));
        out.push('\n');
    }

    out
}

/// Infers a language identifier from a file path's extension.
///
/// Returns a lowercase language tag (for example `"rust"`) suitable for
/// passing to CodeMirror, or `None` if the extension is unknown.
pub fn detect_language_from_path(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_str()?.to_lowercase();
    match ext.as_str() {
        "rs" => Some("rust"),
        "js" => Some("javascript"),
        "jsx" => Some("javascript"),
        "ts" => Some("typescript"),
        "tsx" => Some("typescript"),
        "py" => Some("python"),
        "rb" => Some("ruby"),
        "go" => Some("go"),
        "java" => Some("java"),
        "kt" => Some("kotlin"),
        "kts" => Some("kotlin"),
        "swift" => Some("swift"),
        "c" => Some("c"),
        "h" => Some("c"),
        "cpp" | "cc" | "cxx" => Some("cpp"),
        "hpp" | "hxx" => Some("cpp"),
        "cs" => Some("csharp"),
        "php" => Some("php"),
        "html" | "htm" => Some("html"),
        "css" => Some("css"),
        "scss" => Some("scss"),
        "less" => Some("less"),
        "json" => Some("json"),
        "xml" => Some("xml"),
        "yaml" | "yml" => Some("yaml"),
        "toml" => Some("toml"),
        "md" | "markdown" => Some("markdown"),
        "sql" => Some("sql"),
        "sh" | "bash" | "zsh" => Some("shell"),
        "ps1" => Some("powershell"),
        "r" => Some("r"),
        "lua" => Some("lua"),
        "zig" => Some("zig"),
        "dart" => Some("dart"),
        "ex" | "exs" => Some("elixir"),
        "erl" => Some("erlang"),
        "hs" => Some("haskell"),
        "ml" | "mli" => Some("ocaml"),
        "clj" | "cljs" => Some("clojure"),
        "scala" => Some("scala"),
        "vue" => Some("vue"),
        "svelte" => Some("svelte"),
        "tf" => Some("hcl"),
        "dockerfile" => Some("dockerfile"),
        "graphql" | "gql" => Some("graphql"),
        "proto" => Some("protobuf"),
        "txt" | "text" => Some("plaintext"),
        "csv" => Some("csv"),
        "log" => Some("plaintext"),
        "ini" | "cfg" => Some("ini"),
        "env" => Some("properties"),
        _ => None,
    }
    .map(String::from)
}

/// Filters a sequence of process arguments down to the set of file paths
/// that exist on disk and can plausibly be opened as buffers.
///
/// Designed to be fed `std::env::args_os().skip(1)` at cold launch, or
/// the `argv` slice handed to the `tauri-plugin-single-instance` callback
/// on a warm second launch. The caller is responsible for skipping arg0
/// (the binary path) — this helper does not.
///
/// The helper:
///
/// - drops arguments that begin with `-` (flags),
/// - drops paths that do not exist or are not regular files,
/// - converts to absolute paths via [`std::fs::canonicalize`], falling back
///   to the input path when canonicalisation fails (rare but possible on
///   exotic filesystems),
/// - de-duplicates while preserving first-seen order.
///
/// Canonicalisation also strips Windows `\\?\` UNC prefixes via a final
/// `PathBuf` reconstruction so paths round-trip cleanly through the
/// frontend `openFile` IPC call.
pub fn arg_paths_from_iter<I>(iter: I) -> Vec<PathBuf>
where
    I: IntoIterator<Item = OsString>,
{
    let mut out: Vec<PathBuf> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();

    for raw in iter {
        if raw.is_empty() {
            continue;
        }
        if let Some(s) = raw.to_str() {
            if s.starts_with('-') {
                continue;
            }
        }

        let candidate = PathBuf::from(&raw);
        if !candidate.is_file() {
            continue;
        }

        let canonical = std::fs::canonicalize(&candidate).unwrap_or(candidate);
        let normalized = strip_unc_prefix(canonical);

        if seen.insert(normalized.clone()) {
            out.push(normalized);
        }
    }

    out
}

#[cfg(windows)]
fn strip_unc_prefix(path: PathBuf) -> PathBuf {
    const UNC: &str = r"\\?\";
    match path.to_str() {
        Some(s) if s.starts_with(UNC) => PathBuf::from(&s[UNC.len()..]),
        _ => path,
    }
}

#[cfg(not(windows))]
fn strip_unc_prefix(path: PathBuf) -> PathBuf {
    path
}

/// Returns the file name component of `path` as an owned string, or
/// `"untitled"` when the path has no usable file name.
pub fn extract_filename(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("untitled")
        .to_string()
}
