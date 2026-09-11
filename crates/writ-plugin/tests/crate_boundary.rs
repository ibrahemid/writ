//! What this crate is allowed to reach.
//!
//! `writ-plugin` declares an extension boundary, and a boundary that pulls in
//! storage or the app is not one: a consumer targeting this crate would be
//! taking the whole tree with it. CLAUDE.md states the rule and this test is
//! what holds it, by reading the manifest rather than trusting the import list.
//!
//! The manifest is read line by line rather than parsed by a TOML crate. The
//! one that was here existed for the plugin-manifest stub, which ADR-032
//! removed, and a dependency added back to read four section headers would be
//! the stub's last trace.

use std::path::PathBuf;

/// The crates `[dependencies]` may name.
const ALLOWED: &[&str] = &["writ-core", "serde", "serde_json", "thiserror"];

/// The only workspace crate this one may point at.
const ONLY_PATH_DEPENDENCY: &str = "writ-core";

fn manifest() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    std::fs::read_to_string(path).expect("Cargo.toml")
}

/// Every `name = …` line under `[dependencies]`, in declaration order.
///
/// Stops at the next section header, so `[dev-dependencies]` and the bench
/// table below it are another question.
fn dependency_lines(manifest: &str) -> Vec<String> {
    manifest
        .lines()
        .map(str::trim)
        .skip_while(|line| *line != "[dependencies]")
        .skip(1)
        .take_while(|line| !line.starts_with('['))
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.to_string())
        .collect()
}

/// The name a dependency line declares.
fn declared_name(line: &str) -> &str {
    line.split('=').next().unwrap_or_default().trim()
}

#[test]
fn the_dependencies_are_the_four_the_boundary_needs() {
    let manifest = manifest();
    let declared: Vec<String> = dependency_lines(&manifest)
        .iter()
        .map(|line| declared_name(line).to_string())
        .collect();
    let unexpected: Vec<&String> = declared
        .iter()
        .filter(|name| !ALLOWED.contains(&name.as_str()))
        .collect();

    assert!(
        unexpected.is_empty(),
        "writ-plugin declares the extension boundary and takes {ALLOWED:?}; it also declares \
         {unexpected:?}"
    );
    assert!(
        !declared.is_empty(),
        "the walk found no dependency section, so it read the wrong file"
    );
}

#[test]
fn the_only_workspace_crate_reached_is_writ_core() {
    let manifest = manifest();
    let pointed_at: Vec<String> = manifest
        .lines()
        .map(str::trim)
        .filter(|line| line.contains("path = "))
        .map(|line| declared_name(line).to_string())
        .collect();

    assert_eq!(
        pointed_at,
        vec![ONLY_PATH_DEPENDENCY.to_string()],
        "a second path dependency points this crate at a sibling; the trait it declares is \
         `writ-core`'s so that neither side has to"
    );
}

#[test]
fn the_host_surface_is_reachable_under_this_crate() {
    // The re-export is the boundary's whole shape: a consumer finds the trait,
    // the capabilities and the error in one crate.
    let held: writ_plugin::host::PermissionSet = [writ_plugin::host::Capability::ReadNote]
        .into_iter()
        .collect();
    assert!(held.is_read_only());
}
