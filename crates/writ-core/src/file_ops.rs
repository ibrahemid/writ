//! Pure file helpers: validation, language detection, filename extraction.

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::errors::{WritError, WritResult};

const MAX_FILE_SIZE: u64 = 10 * 1024 * 1024;

const BINARY_CHECK_BYTES: usize = 8192;

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

/// Validates that `path` refers to a regular, non-binary file small enough
/// to open.
///
/// Fails with [`WritError::Io`] if any of the following hold:
///
/// - the path does not exist,
/// - the path is not a regular file,
/// - the file exceeds the internal 10 MiB size limit,
/// - the first 8 KiB contain a NUL byte, which Writ treats as a binary
///   heuristic.
pub fn validate_file_for_opening(path: &Path) -> WritResult<()> {
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

    let metadata = std::fs::metadata(path)?;
    if metadata.len() > MAX_FILE_SIZE {
        return Err(WritError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "file too large: {} bytes (max {} bytes)",
                metadata.len(),
                MAX_FILE_SIZE
            ),
        )));
    }

    if metadata.len() > 0 {
        let file = std::fs::File::open(path)?;
        let mut reader = std::io::BufReader::new(file);
        let mut buf = vec![0u8; BINARY_CHECK_BYTES.min(metadata.len() as usize)];
        std::io::Read::read_exact(&mut reader, &mut buf)?;
        if buf.contains(&0) {
            return Err(WritError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("file appears to be binary: {}", path.display()),
            )));
        }
    }

    Ok(())
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
