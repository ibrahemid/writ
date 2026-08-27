//! Guards the vocabulary ADR-028 §10 retires from user-visible strings.
//!
//! The scope is the Rust half: strings that reach the frontend as a message a
//! person reads. Type names, field names, comments, log lines, SQL and JSON
//! keys stay legal, so the scan reads string literals through a small lexer
//! rather than reading whole files.
//!
//! `src-tauri` carries no `regex` and no `walkdir`, and this guard is not a
//! reason to add one: the walk is `std::fs::read_to_string` over a declared
//! file list and the match is a `str::find` with its own word boundaries.

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

/// Every file that builds a string the frontend shows. Paths resolve against
/// `CARGO_MANIFEST_DIR`, which is the `src-tauri` crate root even from an
/// integration test, so a sibling crate is reached with `..`.
const USER_FACING_RUST_FILES: &[&str] = &[
    "src/commands/file.rs",
    "src/commands/buffer.rs",
    "src/commands/workspace.rs",
    "src/commands/history.rs",
    "src/commands/cli.rs",
    "src/commands/storage.rs",
    "src/commands/inbox.rs",
    "src/commands/link.rs",
    "src/commands/default_app.rs",
    "src/commands/notices.rs",
    "src/commands/update.rs",
    "src/commands/preview.rs",
    "src/startup_failure.rs",
    "../crates/writ-core/src/startup.rs",
];

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
        file: "src/commands/buffer.rs",
        line: 94,
        word: "buffer",
        note: "read-only save error, release 0.6",
    },
    AllowedString {
        file: "../crates/writ-core/src/startup.rs",
        line: 41,
        word: "buffer",
        note: "startup failure remedy text, release 0.6",
    },
];

/// Lines whose first token opens a log macro or an attribute, plus the lines a
/// multi-line one runs onto. A log line is not a user message.
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
        if SKIP_PREFIXES.iter().any(|p| head.starts_with(p)) {
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

/// The body of every string literal in a Rust source, with its line. Comments,
/// char literals and lifetimes are stepped over; escapes are left as written,
/// which is enough for a word match.
fn string_literals(src: &str) -> Vec<(usize, String)> {
    let starts = line_starts(src);
    let bytes = src.as_bytes();
    let n = bytes.len();
    let mut out = Vec::new();
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
                out.push((line_of(&starts, i), src[body_start..end].to_string()));
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
            out.push((line_of(&starts, i), src[body_start..end].to_string()));
            i = (end + 1).min(n);
            continue;
        }

        i += 1;
    }

    out
}

fn is_ident_byte(byte: Option<u8>) -> bool {
    matches!(byte, Some(b) if b.is_ascii_alphanumeric() || b == b'_')
}

/// A message a person reads holds several words. A literal with no whitespace
/// is a key, an id, a path or a column name, and SQL is not a message either.
fn is_user_message(value: &str) -> bool {
    const SQL_HEADS: &[&str] = &[
        "SELECT ", "INSERT ", "UPDATE ", "DELETE ", "CREATE ", "REPLACE ", "PRAGMA ", "ALTER ",
        "DROP ", "WITH ", "BEGIN ",
    ];
    if !value.chars().any(char::is_whitespace) {
        return false;
    }
    if !value.chars().any(|c| c.is_ascii_alphabetic()) {
        return false;
    }
    let head = value.trim_start();
    !SQL_HEADS.iter().any(|kw| head.starts_with(kw))
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80
}

/// A case-insensitive whole-word match that also accepts a plural: "Search
/// buffers…" is the same violation as "buffer".
fn contains_banned_word(text: &str, word: &str) -> bool {
    let hay = text.to_lowercase();
    let needle = word.to_lowercase();
    if needle.is_empty() {
        return false;
    }
    let bytes = hay.as_bytes();
    let mut from = 0usize;
    while let Some(rel) = hay[from..].find(&needle) {
        let start = from + rel;
        let end = start + needle.len();
        let before_ok = start == 0 || !is_word_byte(bytes[start - 1]);
        if before_ok {
            for suffix in ["es", "s", ""] {
                let stop = end + suffix.len();
                if stop <= hay.len()
                    && hay.is_char_boundary(stop)
                    && &hay[end..stop] == suffix
                    && (stop == hay.len() || !is_word_byte(bytes[stop]))
                {
                    return true;
                }
            }
        }
        from = start + 1;
    }
    false
}

fn read_scanned_file(relative: &str) -> String {
    let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for part in relative.split('/') {
        path.push(part);
    }
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {relative}: {err}"))
}

fn collect_offenders() -> Vec<(&'static str, usize, &'static str)> {
    let mut offenders: Vec<(&'static str, usize, &'static str)> = Vec::new();
    for file in USER_FACING_RUST_FILES {
        let src = read_scanned_file(file);
        // A `#[cfg(test)]` module is test code, and by convention it closes the
        // file, so the scan stops where it opens.
        let src = match src.find("#[cfg(test)]") {
            Some(at) => &src[..at],
            None => &src[..],
        };
        let skipped = skipped_lines(src);
        for (line, value) in string_literals(src) {
            if skipped.get(line - 1).copied().unwrap_or(false) {
                continue;
            }
            if !is_user_message(&value) {
                continue;
            }
            for word in BANNED {
                if contains_banned_word(&value, word) && !offenders.contains(&(file, line, word)) {
                    offenders.push((file, line, word));
                }
            }
        }
    }
    offenders.sort_unstable();
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
        .map(|(file, line, word)| key(file, line, word))
        .filter(|found| !allowed.contains(found))
        .collect();
    assert!(
        new_violations.is_empty(),
        "ADR-028 §10 retires this vocabulary from user-visible messages: {}",
        new_violations.join(", ")
    );
}

#[test]
fn rust_banned_words_allowlist_has_no_stale_entries() {
    let live: Vec<String> = collect_offenders()
        .into_iter()
        .map(|(file, line, word)| key(file, line, word))
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
