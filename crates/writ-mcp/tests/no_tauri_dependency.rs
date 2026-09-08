//! What this crate is allowed to reach.
//!
//! Two assertions. The manifest names no tauri dependency, which is the
//! `writ-core` rule CLAUDE.md states, applied to the second crate that has it.
//! And the resolved tree carries no HTTP client, which is the automated check
//! behind the threat model's third row: the MCP path makes no outbound request
//! (ADR-031 rule 2.3).
//!
//! Both read the tree as cargo resolved it, so a transitive edge fails the test
//! the same way a direct one does.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Crate names no part of this crate's tree may carry. Each matches the name
/// itself and anything hyphenated under it, so `hyper-util` fails on `hyper`.
const FORBIDDEN: &[&str] = &["tauri", "reqwest", "hyper", "ureq", "wry"];

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Whether `name` is one of [`FORBIDDEN`] or a crate hyphenated under one.
fn is_forbidden(name: &str) -> bool {
    FORBIDDEN
        .iter()
        .any(|banned| name == *banned || name.starts_with(&format!("{banned}-")))
}

#[test]
fn the_manifest_names_no_tauri_dependency() {
    let manifest = std::fs::read_to_string(manifest_dir().join("Cargo.toml")).expect("Cargo.toml");
    let parsed: toml::Table = toml::from_str(&manifest).expect("parse Cargo.toml");

    let mut named = Vec::new();
    for table in ["dependencies", "dev-dependencies", "build-dependencies"] {
        let Some(section) = parsed.get(table).and_then(|value| value.as_table()) else {
            continue;
        };
        for name in section.keys() {
            if name.starts_with("tauri") {
                named.push(format!("{table}.{name}"));
            }
        }
    }

    assert!(
        named.is_empty(),
        "writ-mcp imports no tauri: {}",
        named.join(", ")
    );
}

#[test]
fn the_resolved_tree_carries_no_http_client() {
    let reachable = reachable_crates(&manifest_dir());

    let found: Vec<&String> = reachable.iter().filter(|name| is_forbidden(name)).collect();

    assert!(
        found.is_empty(),
        "the MCP path makes no outbound request, so its tree carries none of {FORBIDDEN:?}; found {found:?}"
    );
    // A tree that resolved to nothing would pass the assertion above for the
    // wrong reason.
    assert!(
        reachable.contains("rmcp"),
        "the walk reached {} crates and not rmcp, so it walked the wrong node",
        reachable.len()
    );
}

/// Every crate reachable from `writ-mcp` over normal dependency edges.
///
/// Dev-dependencies are skipped: they are the test binary's, not the shipped
/// crate's. A failure to read the metadata is fatal rather than skipped, so
/// this test cannot pass by not running.
fn reachable_crates(manifest_dir: &Path) -> BTreeSet<String> {
    let output = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--format-version",
            "1",
            "--offline",
            "--locked",
            "--manifest-path",
        ])
        .arg(manifest_dir.join("Cargo.toml"))
        .output()
        .expect("cargo metadata ran");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("cargo metadata is json");
    let nodes = metadata["resolve"]["nodes"]
        .as_array()
        .expect("a resolve graph");

    let mut by_id = std::collections::HashMap::new();
    for node in nodes {
        by_id.insert(node["id"].as_str().expect("a package id"), node);
    }

    let mut name_of = std::collections::HashMap::new();
    for package in metadata["packages"].as_array().expect("packages") {
        name_of.insert(
            package["id"].as_str().expect("a package id"),
            package["name"]
                .as_str()
                .expect("a package name")
                .to_string(),
        );
    }

    let root = *name_of
        .iter()
        .find(|(_, name)| name.as_str() == "writ-mcp")
        .map(|(id, _)| id)
        .expect("writ-mcp is in the metadata");

    let mut reached = BTreeSet::new();
    let mut queue = vec![root];
    let mut seen = std::collections::HashSet::new();
    while let Some(id) = queue.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Some(node) = by_id.get(id) else { continue };
        for edge in node["deps"].as_array().expect("dependency edges") {
            let normal = edge["dep_kinds"]
                .as_array()
                .expect("dependency kinds")
                .iter()
                .any(|kind| kind["kind"].is_null());
            if !normal {
                continue;
            }
            let next = edge["pkg"].as_str().expect("a package id");
            if let Some(name) = name_of.get(next) {
                reached.insert(name.clone());
            }
            queue.push(next);
        }
    }
    reached
}

#[test]
fn the_matcher_catches_a_crate_and_the_crates_hyphenated_under_it() {
    for name in [
        "tauri",
        "tauri-build",
        "tauri-plugin-shell",
        "reqwest",
        "hyper",
        "hyper-util",
        "ureq",
        "wry",
    ] {
        assert!(is_forbidden(name), "{name}");
    }
    for name in ["tokio", "rmcp", "serde", "hyperlink", "wryneck"] {
        assert!(!is_forbidden(name), "{name}");
    }
}
