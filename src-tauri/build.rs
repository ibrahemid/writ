use std::path::PathBuf;
use std::process::Command;

fn main() {
    check_sidecar_version();
    tauri_build::build()
}

/// Fails the build when the bundled `writ` command is not this version's.
///
/// The app ships the sidecar in `binaries/` as it finds it, so a stale one
/// would serve an older tool set to every connected program without a word.
/// A sidecar this host cannot run (another architecture's) is reported and
/// let through: the version cannot be read, and the release workflow builds
/// every sidecar from the same commit.
fn check_sidecar_version() {
    let Ok(target) = std::env::var("TARGET") else {
        return;
    };
    let extension = if target.contains("windows") {
        ".exe"
    } else {
        ""
    };
    let sidecar = PathBuf::from(format!("binaries/writ-{target}{extension}"));
    println!("cargo:rerun-if-changed={}", sidecar.display());
    if !sidecar.is_file() {
        // tauri-build names the missing sidecar itself.
        return;
    }
    let expected = format!(
        "writ {}",
        std::env::var("CARGO_PKG_VERSION").unwrap_or_default()
    );
    let output = match Command::new(&sidecar).arg("--version").output() {
        Ok(output) if output.status.success() => output,
        Ok(output) => {
            println!(
                "cargo:warning={} --version exited with {}; its version was not checked",
                sidecar.display(),
                output.status
            );
            return;
        }
        Err(error) => {
            println!(
                "cargo:warning={} could not be run here ({error}); its version was not checked",
                sidecar.display()
            );
            return;
        }
    };
    let reported = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if reported != expected {
        panic!(
            "src-tauri/{sidecar} reports `{reported}`, and this build is `{expected}`. \
             Rebuild it: cargo build -p writ-cli && cp target/debug/writ{extension} src-tauri/{sidecar}",
            sidecar = sidecar.display()
        );
    }
}
