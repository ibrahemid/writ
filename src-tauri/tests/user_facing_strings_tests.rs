//! Guards the vocabulary ADR-028 §10 retires from user-visible strings.
//!
//! The scope is the Rust half: strings that reach a person as a message,
//! including the `#[error("…")]` text every `thiserror` enum carries, which is
//! what most IPC failures render. Type names, field names, comments, log
//! lines, SQL and JSON keys stay legal, so the scan reads string literals
//! through a small lexer rather than reading whole files.
//!
//! `src-tauri` carries no `regex` and no `walkdir`, and this guard is not a
//! reason to add one: the walk is `std::fs::read_dir` recursion over declared
//! roots and the match is a `str::find` with its own word boundaries.
//!
//! Known limits. A message assembled from a `const` or a variable escapes the
//! scan, because only the literal is read and the literal is where the word
//! would be. The scan of a file stops at the first line-leading
//! `#[cfg(test)]`, so a declaration written after the test module is invisible.

use std::path::{Path, PathBuf};

/// ADR-028 §10, verbatim. The operator's four (vault, buffer, scratchpad,
/// second brain) are the first four.
const BANNED: &[&str] = &[
    "vault",
    "buffer",
    "scratchpad",
    "second brain",
    "render surface",
    "inbox",
    "reveal",
    "threshold",
    "refuse",
    "debounce",
    "source",
    "dialect",
    "FTS",
    "IPC",
    "sidecar",
    "MiB",
    "syntax highlighting",
    "typography",
];

/// Where the messages are written. The first element is the path from
/// `CARGO_MANIFEST_DIR` (the `src-tauri` crate root even from an integration
/// test, so a sibling crate is reached with `..`); the second is how the file
/// is named in an allowlist record, relative to the workspace root.
const SCANNED_ROOTS: &[(&str, &str)] = &[
    ("src", "src-tauri/src"),
    ("../crates/writ-core/src", "crates/writ-core/src"),
    ("../crates/writ-storage/src", "crates/writ-storage/src"),
    ("../crates/writ-render/src", "crates/writ-render/src"),
    ("../crates/writ-cli/src", "crates/writ-cli/src"),
    ("../crates/writ-plugin/src", "crates/writ-plugin/src"),
    ("../crates/writ-lint/src", "crates/writ-lint/src"),
];

/// Directories inside a scanned root that hold no user message.
const SKIPPED_DIRS: &[&str] = &["tests", "benches", "target", "fixtures"];

/// One string that already says a retired word. The file can only shrink: a
/// record whose string is gone fails the staleness test, so a rename deletes
/// its record in the same change.
struct AllowedString {
    file: &'static str,
    line: usize,
    word: &'static str,
    note: &'static str,
}

const RUST_ALLOWLIST: &[AllowedString] = &[
    AllowedString {
        file: "crates/writ-core/src/default_app.rs",
        line: 67,
        word: "source",
        note: "file-association group label, release 0.6",
    },
    AllowedString {
        file: "crates/writ-core/src/errors.rs",
        line: 12,
        word: "buffer",
        note: "BufferNotFound message, release 0.6",
    },
    AllowedString {
        file: "crates/writ-core/src/errors.rs",
        line: 19,
        word: "buffer",
        note: "BufferAlreadyExists message, release 0.6",
    },
    AllowedString {
        file: "crates/writ-core/src/errors.rs",
        line: 37,
        word: "buffer",
        note: "InvalidTitle message, release 0.6",
    },
    AllowedString {
        file: "crates/writ-core/src/file_ops.rs",
        line: 152,
        word: "MiB",
        note: "size formatter unit, release 0.6",
    },
    AllowedString {
        file: "crates/writ-core/src/startup.rs",
        line: 50,
        word: "buffer",
        note: "startup failure remedy text, release 0.6",
    },
    AllowedString {
        file: "crates/writ-storage/src/database/queries.rs",
        line: 106,
        word: "buffer",
        note: "row lookup error, release 0.6",
    },
];

/// Lines whose first token opens a log macro or an attribute, plus the lines a
/// multi-line one runs onto. A log line is not a user message. `#[error(…)]`
/// is the exception: it *is* the user message.
fn skipped_lines(src: &str) -> Vec<bool> {
    const SKIP_PREFIXES: &[&str] = &[
        "tracing::",
        "trace!",
        "debug!",
        "info!",
        "warn!",
        "error!",
        "#[",
        "#![",
    ];
    let lines: Vec<&str> = src.lines().collect();
    let mut skipped = vec![false; lines.len()];
    let mut i = 0;
    while i < lines.len() {
        let head = lines[i].trim_start();
        let is_skip = SKIP_PREFIXES.iter().any(|p| head.starts_with(p))
            && !head.starts_with("#[error(")
            && !head.starts_with("#[doc");
        if is_skip {
            let mut depth = 0i32;
            let mut j = i;
            loop {
                skipped[j] = true;
                depth += delimiter_balance(lines[j]);
                if depth <= 0 || j + 1 >= lines.len() {
                    break;
                }
                j += 1;
            }
            i = j + 1;
        } else {
            i += 1;
        }
    }
    skipped
}

fn delimiter_balance(line: &str) -> i32 {
    let mut depth = 0i32;
    for ch in line.chars() {
        match ch {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            _ => {}
        }
    }
    depth
}

/// The source up to the first line-leading `#[cfg(test)]`. A test module is
/// test code and by convention it closes the file.
fn without_test_module(src: &str) -> &str {
    let mut offset = 0usize;
    for line in src.split_inclusive('\n') {
        if line.trim_start().starts_with("#[cfg(test)]") {
            return &src[..offset];
        }
        offset += line.len();
    }
    src
}

fn line_starts(src: &str) -> Vec<usize> {
    let mut starts = vec![0usize];
    for (idx, byte) in src.bytes().enumerate() {
        if byte == b'\n' {
            starts.push(idx + 1);
        }
    }
    starts
}

fn line_of(starts: &[usize], index: usize) -> usize {
    match starts.binary_search(&index) {
        Ok(pos) => pos + 1,
        Err(pos) => pos,
    }
}

fn is_ident_byte(byte: Option<u8>) -> bool {
    matches!(byte, Some(b) if b.is_ascii_alphanumeric() || b == b'_')
}

/// A string literal, with the line it opens on and whatever its line says
/// before it. The prefix is what tells a message from a log argument.
struct Literal {
    line: usize,
    value: String,
    prefix: String,
}

impl Literal {
    fn new(starts: &[usize], src: &str, open: usize, body_start: usize, body_end: usize) -> Self {
        let line = line_of(starts, open);
        Self {
            line,
            value: src[body_start..body_end].to_string(),
            prefix: src[starts[line - 1]..open].to_string(),
        }
    }

    /// `true` when the literal is an argument to a log macro, wherever on the
    /// line the call sits: `Ok(n) => info!(n, "…")` is as much a log line as
    /// one that opens the line.
    fn is_log_argument(&self) -> bool {
        const MACROS: &[&str] = &[
            "tracing::",
            "log::",
            "trace!",
            "debug!",
            "info!",
            "warn!",
            "error!",
        ];
        MACROS.iter().any(|m| self.prefix.contains(m))
    }
}

/// The body of every string literal in a Rust source, with its line. Comments,
/// char literals and lifetimes are stepped over; escapes are left as written,
/// which is enough for a word match.
fn string_literals(src: &str) -> Vec<Literal> {
    let starts = line_starts(src);
    let bytes = src.as_bytes();
    let n = bytes.len();
    let mut out: Vec<Literal> = Vec::new();
    let mut i = 0usize;

    while i < n {
        let byte = bytes[i];

        if byte == b'/' && i + 1 < n && bytes[i + 1] == b'/' {
            while i < n && bytes[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if byte == b'/' && i + 1 < n && bytes[i + 1] == b'*' {
            let mut depth = 1usize;
            i += 2;
            while i < n && depth > 0 {
                if bytes[i] == b'/' && i + 1 < n && bytes[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                } else if bytes[i] == b'*' && i + 1 < n && bytes[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }

        // A raw string: r"…", r#"…"#, and any hash count above that.
        if byte == b'r' && !is_ident_byte(i.checked_sub(1).map(|p| bytes[p])) {
            let mut hashes = 0usize;
            let mut j = i + 1;
            while j < n && bytes[j] == b'#' {
                hashes += 1;
                j += 1;
            }
            if j < n && bytes[j] == b'"' {
                let body_start = j + 1;
                let mut k = body_start;
                let end = loop {
                    if k >= n {
                        break n;
                    }
                    if bytes[k] == b'"' && bytes[k + 1..].iter().take(hashes).all(|b| *b == b'#') {
                        break k;
                    }
                    k += 1;
                };
                out.push(Literal::new(&starts, src, i, body_start, end));
                i = (end + 1 + hashes).min(n);
                continue;
            }
        }

        // A char literal, not a lifetime: 'x' or '\n'.
        if byte == b'\'' {
            if i + 1 < n && bytes[i + 1] == b'\\' {
                let mut j = i + 2;
                while j < n && bytes[j] != b'\'' {
                    j += 1;
                }
                i = (j + 1).min(n);
                continue;
            }
            if i + 2 < n && bytes[i + 2] == b'\'' {
                i += 3;
                continue;
            }
            i += 1;
            continue;
        }

        if byte == b'"' {
            let body_start = i + 1;
            let mut j = body_start;
            while j < n {
                if bytes[j] == b'\\' {
                    j += 2;
                    continue;
                }
                if bytes[j] == b'"' {
                    break;
                }
                j += 1;
            }
            let end = j.min(n);
            out.push(Literal::new(&starts, src, i, body_start, end));
            i = (end + 1).min(n);
            continue;
        }

        i += 1;
    }

    out
}

/// SQL is written in upper case here, and a message never is.
fn looks_like_sql(value: &str) -> bool {
    const SQL_WORDS: &[&str] = &[
        "SELECT", "INSERT", "UPDATE", "DELETE", "CREATE", "REPLACE", "PRAGMA", "ALTER", "DROP",
        "WHERE", "FROM", "INTO", "VALUES", "JOIN", "TABLE", "INDEX", "VACUUM", "ATTACH",
    ];
    value
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .any(|token| SQL_WORDS.contains(&token))
}

/// A message a person reads: several words, or one word that opens with a
/// capital the way a label does. A lower-case literal with no whitespace is a
/// key, an id, a path or a column name.
fn is_user_message(value: &str) -> bool {
    if !value.chars().any(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    let has_space = value.chars().any(char::is_whitespace);
    let opens_with_capital = value
        .trim_start()
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_uppercase());
    if !has_space && !opens_with_capital {
        return false;
    }
    !looks_like_sql(value)
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80
}

/// Every spelling of a banned word this guard answers to. A hyphen reads as a
/// space, so `syntax-highlighting` is the same violation as the phrase, and a
/// word ending in `e` inflects off its stem, so "refusing" and "debouncing"
/// count as much as "refused" and "debounced".
fn word_forms(word: &str) -> Vec<String> {
    let mut bases = vec![word.to_lowercase()];
    if word.eq_ignore_ascii_case("typography") {
        for extra in [
            "typographies",
            "typographic",
            "typographical",
            "typographically",
        ] {
            bases.push(extra.to_string());
        }
    }

    let mut forms = Vec::new();
    for base in &bases {
        let (head, last) = match base.rfind(' ') {
            Some(at) => (&base[..=at], &base[at + 1..]),
            None => ("", base.as_str()),
        };
        if let Some(stem) = last.strip_suffix('e') {
            for suffix in ["e", "es", "ed", "ing"] {
                forms.push(format!("{head}{stem}{suffix}"));
            }
        } else {
            for suffix in ["", "s", "es", "ed", "ing", "d"] {
                forms.push(format!("{head}{last}{suffix}"));
            }
        }
    }
    forms
}

/// A case-insensitive whole-word match against every form of the word:
/// "Search buffers…", "refusing" and "syntax-highlighting" are the same
/// violations as "buffer", "refuse" and "syntax highlighting".
fn contains_banned_word(text: &str, word: &str) -> bool {
    let hay: String = text
        .to_lowercase()
        .chars()
        .map(|c| if c == '-' { ' ' } else { c })
        .collect();
    let bytes = hay.as_bytes();

    for needle in word_forms(word) {
        if needle.is_empty() {
            continue;
        }
        let mut from = 0usize;
        while let Some(rel) = hay[from..].find(&needle) {
            let start = from + rel;
            let end = start + needle.len();
            let before_ok = start == 0 || !is_word_byte(bytes[start - 1]);
            let after_ok = end == hay.len() || !is_word_byte(bytes[end]);
            if before_ok && after_ok {
                return true;
            }
            from = start + 1;
        }
    }
    false
}

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn collect_rust_files(root: &Path, display: &str, out: &mut Vec<(String, PathBuf)>) {
    if root.is_file() {
        out.push((display.to_string(), root.to_path_buf()));
        return;
    }
    let entries = std::fs::read_dir(root)
        .unwrap_or_else(|err| panic!("read_dir {}: {err}", root.display()))
        .filter_map(Result::ok);
    let mut found: Vec<(String, PathBuf)> = Vec::new();
    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();
        let path = entry.path();
        if path.is_dir() {
            if SKIPPED_DIRS.contains(&name.as_str()) || name.starts_with('.') {
                continue;
            }
            collect_rust_files(&path, &format!("{display}/{name}"), &mut found);
        } else if name.ends_with(".rs") {
            found.push((format!("{display}/{name}"), path));
        }
    }
    found.sort();
    out.extend(found);
}

fn scanned_files() -> Vec<(String, PathBuf)> {
    let mut files = Vec::new();
    for (relative, display) in SCANNED_ROOTS {
        let mut path = manifest_dir();
        for part in relative.split('/') {
            path.push(part);
        }
        collect_rust_files(&path, display, &mut files);
    }
    files
}

fn collect_offenders() -> Vec<(String, usize, &'static str)> {
    let mut offenders: Vec<(String, usize, &'static str)> = Vec::new();
    for (display, path) in scanned_files() {
        let src = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
        let src = without_test_module(&src);
        let skipped = skipped_lines(src);
        for literal in string_literals(src) {
            if skipped.get(literal.line - 1).copied().unwrap_or(false) {
                continue;
            }
            if literal.is_log_argument() || !is_user_message(&literal.value) {
                continue;
            }
            for word in BANNED {
                let found = (display.clone(), literal.line, *word);
                if contains_banned_word(&literal.value, word) && !offenders.contains(&found) {
                    offenders.push(found);
                }
            }
        }
    }
    offenders.sort();
    offenders
}

fn key(file: &str, line: usize, word: &str) -> String {
    format!("{file}:{line}:{word}")
}

#[test]
fn banned_words_have_no_new_violations_in_rust_user_messages() {
    let allowed: Vec<String> = RUST_ALLOWLIST
        .iter()
        .map(|entry| key(entry.file, entry.line, entry.word))
        .collect();
    let new_violations: Vec<String> = collect_offenders()
        .into_iter()
        .map(|(file, line, word)| key(&file, line, word))
        .filter(|found| !allowed.contains(found))
        .collect();
    assert!(
        new_violations.is_empty(),
        "ADR-028 §10 retires this vocabulary from user-visible messages: {}",
        new_violations.join("\n")
    );
}

#[test]
fn rust_banned_words_allowlist_has_no_stale_entries() {
    let live: Vec<String> = collect_offenders()
        .into_iter()
        .map(|(file, line, word)| key(&file, line, word))
        .collect();
    let stale: Vec<String> = RUST_ALLOWLIST
        .iter()
        .filter(|entry| !live.contains(&key(entry.file, entry.line, entry.word)))
        .map(|entry| {
            format!(
                "{} ({})",
                key(entry.file, entry.line, entry.word),
                entry.note
            )
        })
        .collect();
    assert!(
        stale.is_empty(),
        "these allowlist records no longer match a live string; delete them in the same change: {}",
        stale.join(", ")
    );
}
