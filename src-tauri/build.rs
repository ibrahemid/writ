use std::path::PathBuf;
use std::process::Command;

fn main() {
    check_sidecar_version();
    build_on_device_bridge();
    tauri_build::build()
}

/// Compiles the Swift bridge to Apple's on-device model
/// (`swift/WritOnDevice`) into a static library and links it, for a macOS
/// target, and sets `writ_on_device_bridge` so `src/on_device.rs` calls it.
///
/// Where no Swift compiler answers, the app is built without the bridge and
/// says so; `src/on_device.rs` then reports the model as unsupported, and the
/// on-device row is never offered. An SDK older than macOS 26 compiles the
/// bridge without the framework, to the same effect.
fn build_on_device_bridge() {
    println!("cargo::rustc-check-cfg=cfg(writ_on_device_bridge)");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let source = "swift/WritOnDevice/Sources/WritOnDevice/WritOnDevice.swift";
    println!("cargo:rerun-if-changed={source}");
    println!("cargo:rerun-if-env-changed=MACOSX_DEPLOYMENT_TARGET");
    println!("cargo:rerun-if-env-changed=DEVELOPER_DIR");

    let arch = match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("aarch64") => "arm64",
        Ok("x86_64") => "x86_64",
        other => {
            println!(
                "cargo:warning=no Swift target for {other:?}; Apple's on-device model is left out"
            );
            return;
        }
    };
    let deployment =
        std::env::var("MACOSX_DEPLOYMENT_TARGET").unwrap_or_else(|_| "12.0".to_string());
    let target = format!("{arch}-apple-macos{deployment}");

    let Some(sdk) = xcrun(&["--sdk", "macosx", "--show-sdk-path"]) else {
        println!("cargo:warning=xcrun found no macOS SDK; Apple's on-device model is left out");
        return;
    };
    let Some(swiftc) = xcrun(&["--sdk", "macosx", "--find", "swiftc"]) else {
        println!("cargo:warning=xcrun found no swiftc; Apple's on-device model is left out");
        return;
    };
    if !PathBuf::from(&sdk)
        .join("System/Library/Frameworks/FoundationModels.framework")
        .exists()
    {
        println!(
            "cargo:warning=the macOS SDK at {sdk} predates macOS 26; \
             Apple's on-device model is left out of this build"
        );
    }

    let out = PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    let library = out.join("libwrit_on_device.a");
    let compiled = Command::new(&swiftc)
        .args(["-sdk", &sdk, "-target", &target])
        .args(["-parse-as-library", "-emit-library", "-static", "-O"])
        .args(["-swift-version", "5", "-module-name", "WritOnDevice"])
        .arg("-module-cache-path")
        .arg(out.join("swift-module-cache"))
        .arg(source)
        .arg("-o")
        .arg(&library)
        .status();
    match compiled {
        Ok(status) if status.success() => {}
        Ok(status) => panic!("swiftc exited with {status} compiling {source}"),
        Err(error) => {
            println!(
                "cargo:warning=swiftc could not be run ({error}); Apple's on-device model is left out"
            );
            return;
        }
    }

    // The Swift runtime ships with macOS; the compatibility archives the
    // object names for an older deployment target ship with the toolchain.
    let toolchain_lib = PathBuf::from(&swiftc)
        .parent()
        .and_then(|bin| bin.parent())
        .map(|usr| usr.join("lib/swift/macosx"));
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=writ_on_device");
    println!("cargo:rustc-link-search=native={sdk}/usr/lib/swift");
    if let Some(toolchain_lib) = toolchain_lib {
        println!("cargo:rustc-link-search=native={}", toolchain_lib.display());
    }
    println!("cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift");
    println!("cargo:rustc-cfg=writ_on_device_bridge");
}

/// One line `xcrun` prints, or `None` when it cannot be run or fails.
fn xcrun(args: &[&str]) -> Option<String> {
    let output = Command::new("xcrun").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let line = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (!line.is_empty()).then_some(line)
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
