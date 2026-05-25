use std::fs;
use std::path::PathBuf;

const UNWIRED_MARKER: &str = "STATUS: UNWIRED INFRASTRUCTURE";

fn recovery_mod_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("recovery")
        .join("mod.rs")
}

fn workspace_root() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.pop();
    path
}

#[test]
fn recovery_module_documents_unwired_status() {
    let path = recovery_mod_path();
    let contents = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));

    assert!(
        contents.contains(UNWIRED_MARKER),
        "crates/writ-storage/src/recovery/mod.rs must retain the \
         `{UNWIRED_MARKER}` docstring marker until the recovery flow is \
         wired into the running app. If you are wiring it, follow the \
         resurrection plan in that docstring and remove this contract test \
         in the same PR that adds the wiring and a real end-to-end test.",
    );
}

#[test]
fn recovery_apis_remain_uncalled_by_app_shell() {
    let src_tauri = workspace_root().join("src-tauri").join("src");
    assert!(
        src_tauri.exists(),
        "expected src-tauri/src at {}",
        src_tauri.display(),
    );

    let mut offenders: Vec<String> = Vec::new();
    let needles = [
        "check_dirty_shutdown",
        "SnapshotManager",
        "write_snapshot",
        "ConsistencyChecker",
    ];

    visit_rust_files(&src_tauri, &mut |file, contents| {
        for needle in &needles {
            if contents.contains(needle) {
                offenders.push(format!("{}: references `{needle}`", file.display()));
            }
        }
    });

    assert!(
        offenders.is_empty(),
        "src-tauri now references recovery APIs without removing the \
         UNWIRED contract. If recovery is being wired, update \
         `crates/writ-storage/src/recovery/mod.rs` (drop the UNWIRED marker), \
         update every public surface (README, CHANGELOG, docs/ARCHITECTURE, \
         docs/adr/004, site changelog), and delete this test in the same \
         PR. Offenders:\n  {}",
        offenders.join("\n  "),
    );
}

fn visit_rust_files(dir: &PathBuf, visit: &mut dyn FnMut(&PathBuf, &str)) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            visit_rust_files(&path, visit);
        } else if path.extension().and_then(|s| s.to_str()) == Some("rs") {
            if let Ok(contents) = fs::read_to_string(&path) {
                visit(&path, &contents);
            }
        }
    }
}
