use std::process::Command;

#[test]
fn version_flag_reports_the_workspace_version() {
    let output = Command::new(env!("CARGO_BIN_EXE_writ"))
        .arg("--version")
        .output()
        .expect("failed to run the writ binary");

    assert!(
        output.status.success(),
        "writ --version exited with {:?}",
        output.status.code()
    );

    let reported = String::from_utf8(output.stdout).expect("writ --version wrote non-UTF-8 output");
    assert_eq!(
        reported.trim(),
        format!("writ {}", env!("CARGO_PKG_VERSION"))
    );

    // The crate once pinned its own version, so `writ --version` reported 0.1.0
    // from a 0.2.0 install. Fail if the pin comes back.
    assert_eq!(
        env!("CARGO_PKG_VERSION"),
        workspace_version(),
        "writ-cli must inherit the workspace version"
    );
}

fn workspace_version() -> String {
    let manifest_path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.toml");
    let manifest = std::fs::read_to_string(manifest_path)
        .unwrap_or_else(|e| panic!("cannot read {manifest_path}: {e}"));

    let mut in_workspace_package = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_workspace_package = line == "[workspace.package]";
            continue;
        }
        if !in_workspace_package {
            continue;
        }
        if let Some(rest) = line.strip_prefix("version") {
            if let Some(value) = rest.trim_start().strip_prefix('=') {
                return value.trim().trim_matches('"').to_string();
            }
        }
    }

    panic!("no version under [workspace.package] in {manifest_path}");
}
