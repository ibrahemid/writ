//! The external-link surface holds two invariants that live in configuration
//! rather than in code, so they are asserted against the files themselves.

const CAPABILITIES: &str = include_str!("../capabilities/default.json");
const LIB_RS: &str = include_str!("../src/lib.rs");

#[test]
fn the_opener_plugin_is_not_reachable_from_javascript() {
    let capabilities: serde_json::Value =
        serde_json::from_str(CAPABILITIES).expect("capabilities/default.json parses");
    let granted = capabilities["permissions"]
        .as_array()
        .expect("permissions is an array");

    for permission in granted {
        let name = permission.as_str().unwrap_or_default();
        assert!(
            !name.starts_with("opener:"),
            "granting {name} would let the frontend open a URL without the link policy"
        );
    }
}

#[test]
fn both_link_commands_are_registered() {
    assert!(LIB_RS.contains("commands::link::open_external_url"));
    assert!(LIB_RS.contains("commands::link::classify_external_url"));
}

#[test]
fn the_opener_plugin_is_initialized() {
    assert!(LIB_RS.contains("tauri_plugin_opener::init()"));
}
