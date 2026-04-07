use std::path::Path;
use writ_core::file_ops::{detect_language_from_path, extract_filename, validate_file_for_opening};

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

#[test]
fn validate_file_rejects_nonexistent_path() {
    let result = validate_file_for_opening(Path::new("/tmp/writ-test-nonexistent-file.txt"));
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("not found"), "error should mention not found: {}", err);
}

#[test]
fn validate_file_rejects_directory() {
    let dir = tempfile::TempDir::new().unwrap();
    let result = validate_file_for_opening(dir.path());
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("not a file"), "error should mention not a file: {}", err);
}

#[test]
fn validate_file_rejects_oversized_file() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("huge.txt");
    let f = std::fs::File::create(&path).unwrap();
    f.set_len(11 * 1024 * 1024).unwrap();
    let result = validate_file_for_opening(&path);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("too large"), "error should mention too large: {}", err);
}

#[test]
fn validate_file_rejects_binary_file() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("binary.dat");
    let data: Vec<u8> = vec![0x89, 0x50, 0x4E, 0x47, 0x00, 0x0D, 0x0A, 0x1A];
    std::fs::write(&path, &data).unwrap();
    let result = validate_file_for_opening(&path);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("binary"), "error should mention binary: {}", err);
}

#[test]
fn validate_file_accepts_valid_text_file() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("valid.txt");
    std::fs::write(&path, "Hello, world!\nThis is a text file.").unwrap();
    assert!(validate_file_for_opening(&path).is_ok());
}

#[test]
fn validate_file_accepts_empty_file() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("empty.txt");
    std::fs::write(&path, "").unwrap();
    assert!(validate_file_for_opening(&path).is_ok());
}

#[test]
fn validate_file_accepts_utf8_with_special_chars() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("unicode.md");
    std::fs::write(&path, "# Hello 世界\nCafé résumé naïve").unwrap();
    assert!(validate_file_for_opening(&path).is_ok());
}

#[test]
fn extract_filename_from_path() {
    assert_eq!(extract_filename(Path::new("/home/user/notes/todo.md")), "todo.md");
    assert_eq!(extract_filename(Path::new("relative/file.rs")), "file.rs");
    assert_eq!(extract_filename(Path::new("file.txt")), "file.txt");
}

#[test]
fn extract_filename_fallback_for_root() {
    assert_eq!(extract_filename(Path::new("/")), "untitled");
}

#[test]
fn validate_file_accepts_path_with_spaces() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("my document.txt");
    std::fs::write(&path, "content with spaces in path").unwrap();
    assert!(validate_file_for_opening(&path).is_ok());
}

#[test]
fn detect_language_with_dotfile_path() {
    assert!(detect_language_from_path(Path::new("/home/user/.gitignore")).is_none());
    assert_eq!(
        detect_language_from_path(Path::new("/home/user/.eslintrc.json")).as_deref(),
        Some("json")
    );
}

#[test]
fn extract_filename_with_spaces() {
    assert_eq!(
        extract_filename(Path::new("/home/user/my notes.md")),
        "my notes.md"
    );
}

#[test]
fn validate_file_at_exact_size_limit() {
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("exact.txt");
    let content = "a".repeat(10 * 1024 * 1024);
    std::fs::write(&path, &content).unwrap();
    assert!(validate_file_for_opening(&path).is_ok());
}
