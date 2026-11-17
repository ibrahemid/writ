//! Tests that build an app on Tauri's mock runtime live under `tests/`.
//!
//! On Windows the mock runtime needs the Common Controls v6 manifest, which
//! build.rs (`embed_manifest_for_tests`) links into integration test binaries
//! only: cargo has no link argument for a library's unit-test binary that
//! spares the app binary. A unit test under `src/` that names the mock runtime
//! keeps that whole binary from loading there, and every unit test in it with
//! it.

use std::path::{Path, PathBuf};

/// What a source file names when it reaches the mock runtime.
const MOCK_RUNTIME: &[&str] = &["tauri::test", "MockRuntime", "mock_builder"];

fn names_mock_runtime(text: &str) -> bool {
    MOCK_RUNTIME.iter().any(|name| text.contains(name))
}

/// Every Rust file under `src/`, relative to it.
fn sources(src: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut folders = vec![src.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).expect("read the source folder") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                folders.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                found.push(path.strip_prefix(src).expect("inside src").to_path_buf());
            }
        }
    }
    found.sort();
    found
}

#[test]
fn no_source_file_names_the_mock_runtime() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let files = sources(&src);
    assert!(
        files.contains(&PathBuf::from("lib.rs")),
        "the scan did not reach src/lib.rs"
    );
    let naming: Vec<PathBuf> = files
        .into_iter()
        .filter(|file| {
            names_mock_runtime(&std::fs::read_to_string(src.join(file)).expect("read the source"))
        })
        .collect();
    assert!(
        naming.is_empty(),
        "{naming:?} under src-tauri/src name Tauri's mock runtime. A test that builds a mock app \
         goes in an integration test under src-tauri/tests, with what it needs made public: \
         on Windows only those binaries carry the Common Controls manifest the mock runtime \
         needs (build.rs, embed_manifest_for_tests), and a library unit-test binary that links \
         it fails to load with STATUS_ENTRYPOINT_NOT_FOUND, so none of its tests run there"
    );
}

#[test]
fn every_way_of_reaching_the_mock_runtime_is_caught() {
    for (i, text) in [
        "use tauri::test::{mock_builder, mock_context, noop_assets};",
        "use tauri::{test::mock_builder, Manager};",
        "fn app() -> tauri::App<tauri::test::MockRuntime> { todo!() }",
        "let app: App<MockRuntime> = build();",
    ]
    .into_iter()
    .enumerate()
    {
        assert!(names_mock_runtime(text), "source #{i} is not caught");
    }
    assert!(!names_mock_runtime(
        "use tauri::{AppHandle, Manager, Runtime};\n#[cfg(test)]\nmod tests {}"
    ));
}
