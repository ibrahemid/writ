use std::path::Path;
use writ_core::file_ops::{
    classify_file, classify_path, detect_language_from_path, extract_filename, generate_hex_dump,
    FileOpenMode, THRESHOLD_LARGE_BYTES, THRESHOLD_MAX_BYTES, THRESHOLD_NORMAL_BYTES,
};

// ── language detection ────────────────────────────────────────────────────────

#[test]
fn detect_language_common_extensions() {
    let cases = vec![
        ("main.rs", "rust"),
        ("app.js", "javascript"),
        ("app.jsx", "javascript"),
        ("index.ts", "typescript"),
        ("page.tsx", "typescript"),
        ("script.py", "python"),
        ("main.go", "go"),
        ("App.java", "java"),
        ("style.css", "css"),
        ("index.html", "html"),
        ("page.htm", "html"),
        ("data.json", "json"),
        ("config.yaml", "yaml"),
        ("config.yml", "yaml"),
        ("Cargo.toml", "toml"),
        ("README.md", "markdown"),
        ("notes.markdown", "markdown"),
        ("query.sql", "sql"),
        ("deploy.sh", "shell"),
        ("run.bash", "shell"),
        ("file.txt", "plaintext"),
    ];

    for (filename, expected) in cases {
        let result = detect_language_from_path(Path::new(filename));
        assert_eq!(
            result.as_deref(),
            Some(expected),
            "expected {} for {}",
            expected,
            filename
        );
    }
}

#[test]
fn detect_language_systems_languages() {
    let cases = vec![
        ("lib.c", "c"),
        ("header.h", "c"),
        ("main.cpp", "cpp"),
        ("util.cc", "cpp"),
        ("header.hpp", "cpp"),
        ("Program.cs", "csharp"),
        ("main.swift", "swift"),
        ("main.kt", "kotlin"),
        ("build.kts", "kotlin"),
        ("main.zig", "zig"),
        ("main.dart", "dart"),
    ];

    for (filename, expected) in cases {
        let result = detect_language_from_path(Path::new(filename));
        assert_eq!(
            result.as_deref(),
            Some(expected),
            "expected {} for {}",
            expected,
            filename
        );
    }
}

#[test]
fn detect_language_web_and_config() {
    let cases = vec![
        ("style.scss", "scss"),
        ("style.less", "less"),
        ("schema.graphql", "graphql"),
        ("schema.gql", "graphql"),
        ("app.vue", "vue"),
        ("App.svelte", "svelte"),
        ("main.tf", "hcl"),
        ("config.ini", "ini"),
        ("data.csv", "csv"),
        ("message.proto", "protobuf"),
    ];

    for (filename, expected) in cases {
        let result = detect_language_from_path(Path::new(filename));
        assert_eq!(
            result.as_deref(),
            Some(expected),
            "expected {} for {}",
            expected,
            filename
        );
    }
}

#[test]
fn detect_language_returns_none_for_unknown_extension() {
    assert!(detect_language_from_path(Path::new("file.xyz")).is_none());
    assert!(detect_language_from_path(Path::new("file.abc123")).is_none());
}

#[test]
fn detect_language_returns_none_for_no_extension() {
    assert!(detect_language_from_path(Path::new("Makefile")).is_none());
    assert!(detect_language_from_path(Path::new("LICENSE")).is_none());
}

#[test]
fn detect_language_is_case_insensitive() {
    assert_eq!(
        detect_language_from_path(Path::new("Main.RS")).as_deref(),
        Some("rust")
    );
    assert_eq!(
        detect_language_from_path(Path::new("App.TSX")).as_deref(),
        Some("typescript")
    );
}

// ── classify_file (pure, no I/O) ─────────────────────────────────────────────

#[test]
fn classify_file_normal_below_threshold() {
    assert_eq!(classify_file(0, false), FileOpenMode::Normal);
    assert_eq!(classify_file(1024, false), FileOpenMode::Normal);
    assert_eq!(
        classify_file(THRESHOLD_NORMAL_BYTES, false),
        FileOpenMode::Normal
    );
}

#[test]
fn classify_file_large_above_normal_threshold() {
    assert_eq!(
        classify_file(THRESHOLD_NORMAL_BYTES + 1, false),
        FileOpenMode::LargeFile
    );
    assert_eq!(
        classify_file(THRESHOLD_LARGE_BYTES, false),
        FileOpenMode::LargeFile
    );
}

#[test]
fn classify_file_confirm_above_large_threshold() {
    assert_eq!(
        classify_file(THRESHOLD_LARGE_BYTES + 1, false),
        FileOpenMode::LargeFileConfirm
    );
    assert_eq!(
        classify_file(THRESHOLD_MAX_BYTES, false),
        FileOpenMode::LargeFileConfirm
    );
}

#[test]
fn classify_file_binary_overrides_size_tier() {
    // Binary regardless of size.
    assert_eq!(classify_file(0, true), FileOpenMode::Binary);
    assert_eq!(classify_file(1024, true), FileOpenMode::Binary);
    assert_eq!(
        classify_file(THRESHOLD_NORMAL_BYTES + 1, true),
        FileOpenMode::Binary
    );
    assert_eq!(
        classify_file(THRESHOLD_LARGE_BYTES + 1, true),
        FileOpenMode::Binary
    );
}

// ── classify_path (with disk I/O) ────────────────────────────────────────────

#[test]
fn classify_path_rejects_nonexistent() {
    let result = classify_path(Path::new("/tmp/writ-test-nonexistent-file.txt"));
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("not found"), "expected 'not found' in: {}", msg);
}

#[test]
fn classify_path_rejects_directory() {
    let dir = tempfile::TempDir::new().unwrap();
    let result = classify_path(dir.path());
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("not a file"),
        "expected 'not a file' in: {}",
        msg
    );
}

#[test]
fn classify_path_normal_for_small_text_file() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("small.txt");
    std::fs::write(&path, "Hello, world!").unwrap();
    let c = classify_path(&path).unwrap();
    assert_eq!(c.mode, FileOpenMode::Normal);
    assert_eq!(c.size_bytes, 13);
}

#[test]
fn classify_path_normal_for_empty_file() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("empty.txt");
    std::fs::write(&path, "").unwrap();
    let c = classify_path(&path).unwrap();
    assert_eq!(c.mode, FileOpenMode::Normal);
    assert_eq!(c.size_bytes, 0);
}

/// Creates a file of exactly `target_size` bytes filled with non-NUL ASCII
/// text so the binary sniff does not trigger.
fn make_text_file_of_size(path: &std::path::Path, target_size: u64) {
    use std::io::Write;
    let chunk = b"abcdefghijklmnopqrstuvwxyz0123456789\n";
    let mut f = std::fs::File::create(path).unwrap();
    let mut written: u64 = 0;
    while written < target_size {
        let remaining = (target_size - written) as usize;
        let to_write = chunk.len().min(remaining);
        f.write_all(&chunk[..to_write]).unwrap();
        written += to_write as u64;
    }
}

#[test]
fn classify_path_normal_at_exact_normal_threshold() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("exact.txt");
    make_text_file_of_size(&path, THRESHOLD_NORMAL_BYTES);
    let c = classify_path(&path).unwrap();
    assert_eq!(c.mode, FileOpenMode::Normal);
}

#[test]
fn classify_path_large_file_above_normal_threshold() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("large.txt");
    make_text_file_of_size(&path, THRESHOLD_NORMAL_BYTES + 1);
    let c = classify_path(&path).unwrap();
    assert_eq!(c.mode, FileOpenMode::LargeFile);
}

#[test]
fn classify_path_large_at_exact_large_threshold() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("biglarge.txt");
    make_text_file_of_size(&path, THRESHOLD_LARGE_BYTES);
    let c = classify_path(&path).unwrap();
    assert_eq!(c.mode, FileOpenMode::LargeFile);
}

#[test]
fn classify_path_confirm_above_large_threshold() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("huge.txt");
    make_text_file_of_size(&path, THRESHOLD_LARGE_BYTES + 1);
    let c = classify_path(&path).unwrap();
    assert_eq!(c.mode, FileOpenMode::LargeFileConfirm);
}

#[test]
fn classify_path_refused_above_max_threshold() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("massive.txt");
    let f = std::fs::File::create(&path).unwrap();
    f.set_len(THRESHOLD_MAX_BYTES + 1).unwrap();
    let c = classify_path(&path).unwrap();
    assert!(
        matches!(c.mode, FileOpenMode::Refused { .. }),
        "expected Refused, got {:?}",
        c.mode
    );
    if let FileOpenMode::Refused { reason } = &c.mode {
        assert!(
            reason.contains("cannot be opened safely"),
            "reason should mention safety: {}",
            reason
        );
    }
}

#[test]
fn classify_path_binary_file_returns_binary_mode() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("binary.dat");
    // PNG magic bytes contain NUL.
    let data: Vec<u8> = vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00];
    std::fs::write(&path, &data).unwrap();
    let c = classify_path(&path).unwrap();
    assert_eq!(c.mode, FileOpenMode::Binary);
}

#[test]
fn classify_path_utf8_special_chars_not_binary() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("unicode.md");
    std::fs::write(&path, "# Hello 世界\nCafé résumé naïve").unwrap();
    let c = classify_path(&path).unwrap();
    assert_eq!(c.mode, FileOpenMode::Normal);
}

#[test]
fn classify_path_accepts_path_with_spaces() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("my document.txt");
    std::fs::write(&path, "content").unwrap();
    let c = classify_path(&path).unwrap();
    assert_eq!(c.mode, FileOpenMode::Normal);
}

// ── generate_hex_dump ─────────────────────────────────────────────────────────

#[test]
fn hex_dump_empty_input_produces_empty_output() {
    let out = generate_hex_dump(&[], 0);
    assert_eq!(out, "");
}

#[test]
fn hex_dump_single_byte_alignment() {
    let out = generate_hex_dump(&[0x41], 1);
    // Should contain the offset, the hex byte, and the ASCII char.
    assert!(out.contains("00000000"), "missing offset: {}", out);
    assert!(out.contains("41"), "missing hex byte: {}", out);
    assert!(out.contains("|A|"), "missing ascii gutter: {}", out);
}

#[test]
fn hex_dump_16_bytes_single_row() {
    let data: Vec<u8> = (0x41..=0x50).collect(); // 'A' .. 'P'
    let out = generate_hex_dump(&data, 16);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 1, "expected 1 row for 16 bytes");
    assert!(lines[0].starts_with("00000000"), "offset wrong: {}", lines[0]);
    // All 16 ASCII chars present in gutter.
    assert!(lines[0].contains("|ABCDEFGHIJKLMNOP|"), "gutter wrong: {}", lines[0]);
}

#[test]
fn hex_dump_17_bytes_two_rows() {
    let data: Vec<u8> = (0..17).collect();
    let out = generate_hex_dump(&data, 17);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 2, "expected 2 rows for 17 bytes");
    assert!(lines[1].starts_with("00000010"), "second offset wrong: {}", lines[1]);
}

#[test]
fn hex_dump_non_printable_replaced_with_dot() {
    let data: Vec<u8> = vec![0x00, 0x01, 0x41, 0x7f];
    let out = generate_hex_dump(&data, 4);
    // Byte 0x00 and 0x01 and 0x7f → '.', 0x41 → 'A'
    assert!(out.contains("|..A.|"), "gutter wrong: {}", out);
}

#[test]
fn hex_dump_mid_group_separator() {
    // 16 bytes: the hex section should have a gap after byte 8.
    let data = vec![0u8; 16];
    let out = generate_hex_dump(&data, 16);
    // 8 groups of "00 " then space then 8 more.
    let line = out.lines().next().unwrap();
    // Bytes 0-7, gap, bytes 8-15.
    let hex_section = &line[10..58]; // rough index range
    assert!(
        hex_section.contains("  "),
        "missing mid-group gap in: {}",
        line
    );
}

#[test]
fn hex_dump_truncation_notice_when_clamped() {
    use writ_core::file_ops::HEX_DUMP_MAX_BYTES;
    // Pass data larger than HEX_DUMP_MAX_BYTES.
    let big: Vec<u8> = vec![0x41; HEX_DUMP_MAX_BYTES + 1];
    let out = generate_hex_dump(&big, HEX_DUMP_MAX_BYTES + 1);
    assert!(
        out.contains("truncated"),
        "missing truncation notice: {}",
        &out[out.len().saturating_sub(200)..]
    );
}

#[test]
fn hex_dump_no_truncation_notice_at_exact_limit() {
    use writ_core::file_ops::HEX_DUMP_MAX_BYTES;
    let data = vec![0x41; HEX_DUMP_MAX_BYTES];
    let out = generate_hex_dump(&data, HEX_DUMP_MAX_BYTES);
    assert!(
        !out.contains("truncated"),
        "unexpected truncation notice for exact limit"
    );
}

// ── extract_filename ──────────────────────────────────────────────────────────

#[test]
fn extract_filename_from_path() {
    assert_eq!(
        extract_filename(Path::new("/home/user/notes/todo.md")),
        "todo.md"
    );
    assert_eq!(extract_filename(Path::new("relative/file.rs")), "file.rs");
    assert_eq!(extract_filename(Path::new("file.txt")), "file.txt");
}

#[test]
fn extract_filename_fallback_for_root() {
    assert_eq!(extract_filename(Path::new("/")), "untitled");
}

#[test]
fn extract_filename_with_spaces() {
    assert_eq!(
        extract_filename(Path::new("/home/user/my notes.md")),
        "my notes.md"
    );
}

#[test]
fn detect_language_with_dotfile_path() {
    assert!(detect_language_from_path(Path::new("/home/user/.gitignore")).is_none());
    assert_eq!(
        detect_language_from_path(Path::new("/home/user/.eslintrc.json")).as_deref(),
        Some("json")
    );
}
