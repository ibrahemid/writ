//! The app webview's Content-Security-Policy as the build merges it: the base
//! `tauri.conf.json`, with a platform file layered on top by RFC 7396 JSON
//! Merge Patch, the rule Tauri applies. Shared by `app_csp_tests.rs` and the
//! engine probe, so both read the policy that ships.

use serde_json::{Map, Value};

/// A config file beside `Cargo.toml`, or `None` when it does not exist.
pub fn read_config(name: &str) -> Option<Value> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(name);
    let raw = std::fs::read_to_string(&path).ok()?;
    Some(serde_json::from_str(&raw).unwrap_or_else(|e| panic!("{name} does not parse: {e}")))
}

/// RFC 7396 JSON Merge Patch.
pub fn merge_patch(target: &mut Value, patch: &Value) {
    let Value::Object(patch) = patch else {
        *target = patch.clone();
        return;
    };
    if !target.is_object() {
        *target = Value::Object(Map::new());
    }
    let target = target.as_object_mut().expect("object");
    for (key, value) in patch {
        if value.is_null() {
            target.remove(key);
        } else {
            merge_patch(target.entry(key.clone()).or_insert(Value::Null), value);
        }
    }
}

/// The app CSP for macOS and Linux (`windows == false`) or Windows.
pub fn app_csp_for(windows: bool) -> String {
    let mut config = read_config("tauri.conf.json").expect("tauri.conf.json");
    if windows {
        let patch = read_config("tauri.windows.conf.json")
            .expect("tauri.windows.conf.json is missing, so Windows ships the base CSP");
        merge_patch(&mut config, &patch);
    }
    config["app"]["security"]["csp"]
        .as_str()
        .expect("app.security.csp is a policy string")
        .to_string()
}
