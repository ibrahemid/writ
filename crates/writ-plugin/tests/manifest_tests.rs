use writ_plugin::manifest::PluginManifest;

#[test]
fn manifest_parses_from_toml() {
    let toml_str = r#"
        name = "my-plugin"
        version = "1.0.0"
        description = "A test plugin"
        author = "Alice"
        entry = "main.wasm"
    "#;

    let manifest: PluginManifest = toml::from_str(toml_str).expect("failed to parse manifest");

    assert_eq!(manifest.name, "my-plugin");
    assert_eq!(manifest.version, "1.0.0");
    assert_eq!(manifest.description, "A test plugin");
    assert_eq!(manifest.author, "Alice");
    assert_eq!(manifest.entry, "main.wasm");
}

#[test]
fn manifest_serializes_to_toml() {
    let manifest = PluginManifest {
        name: "serializer-plugin".to_string(),
        version: "0.2.0".to_string(),
        description: "Serialization test".to_string(),
        author: "Bob".to_string(),
        entry: "plugin.wasm".to_string(),
    };

    let output = toml::to_string(&manifest).expect("failed to serialize manifest");

    assert!(output.contains("name = \"serializer-plugin\""));
    assert!(output.contains("version = \"0.2.0\""));
    assert!(output.contains("description = \"Serialization test\""));
    assert!(output.contains("author = \"Bob\""));
    assert!(output.contains("entry = \"plugin.wasm\""));
}
