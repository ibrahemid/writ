//! What this crate is allowed to reach (ADR-005, amended 2026-10-14).
//!
//! The connection lives here so another binary can use it without linking
//! Tauri, which holds only while the resolved tree carries no part of the
//! desktop shell. Read from the tree as cargo resolved it, so a transitive
//! edge fails the test the same way a direct one does.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::process::Command;

/// The shell's crates. Each matches the name itself and anything hyphenated
/// under it, so `tauri-plugin-dialog` fails on `tauri`.
const SHELL: &[&str] = &["tauri", "wry", "tao"];

fn is_shell(name: &str) -> bool {
    SHELL
        .iter()
        .any(|shell| name == *shell || name.starts_with(&format!("{shell}-")))
}

/// Every crate reachable from `writ-ai` over normal dependency edges, on any
/// target. A failure to read the metadata is fatal, so the test cannot pass by
/// not running.
fn reachable_crates() -> BTreeSet<String> {
    let output = Command::new(env!("CARGO"))
        .args([
            "metadata",
            "--format-version",
            "1",
            "--locked",
            "--manifest-path",
        ])
        .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .output()
        .expect("cargo metadata ran");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("cargo metadata is json");

    let name_of: HashMap<&str, &str> = metadata["packages"]
        .as_array()
        .expect("packages")
        .iter()
        .map(|package| {
            (
                package["id"].as_str().expect("a package id"),
                package["name"].as_str().expect("a package name"),
            )
        })
        .collect();
    let by_id: HashMap<&str, &serde_json::Value> = metadata["resolve"]["nodes"]
        .as_array()
        .expect("a resolve graph")
        .iter()
        .map(|node| (node["id"].as_str().expect("a package id"), node))
        .collect();
    let root = name_of
        .iter()
        .find(|(_, name)| **name == "writ-ai")
        .map(|(id, _)| *id)
        .expect("writ-ai is in the metadata");

    let mut reached = BTreeSet::new();
    let mut queue = vec![root];
    let mut seen = HashSet::new();
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
                reached.insert((*name).to_string());
            }
            queue.push(next);
        }
    }
    reached
}

#[test]
fn the_connection_links_none_of_the_desktop_shell() {
    let reachable = reachable_crates();
    let found: Vec<&String> = reachable.iter().filter(|name| is_shell(name)).collect();
    assert!(found.is_empty(), "writ-ai reaches {found:?}");
    // A walk that started from the wrong node would pass the assertion above
    // for the wrong reason.
    for needed in ["writ-core", "writ-storage", "reqwest", "keyring"] {
        assert!(
            reachable.contains(needed),
            "the walk did not reach {needed}"
        );
    }
}

#[test]
fn the_matcher_catches_a_shell_crate_and_the_crates_under_it() {
    for name in ["tauri", "tauri-plugin-dialog", "wry", "tao"] {
        assert!(is_shell(name), "{name}");
    }
    for name in ["tokio", "reqwest", "taoist", "wryneck"] {
        assert!(!is_shell(name), "{name}");
    }
}
