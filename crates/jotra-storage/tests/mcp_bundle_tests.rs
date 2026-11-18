//! The Claude Desktop extension bundle: `mcpb pack`'s layout, written into the
//! data folder and nowhere else.

use std::io::Read;

use jotra_core::mcp_clients::{bundle_launcher, bundle_manifest, BUNDLE_LAUNCHER_PATH};
use jotra_storage::mcp_bundle::{bundle_bytes, path, write_bundle, FOLDER};

const SERVER: &str = "/Applications/Jotra.app/Contents/MacOS/jotra";

fn entries(bytes: Vec<u8>) -> Vec<(String, String)> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("a zip");
    (0..archive.len())
        .map(|index| {
            let mut entry = archive.by_index(index).expect("an entry");
            let mut text = String::new();
            entry.read_to_string(&mut text).expect("utf-8");
            (entry.name().expect("a name").into_owned(), text)
        })
        .collect()
}

#[test]
fn the_bundle_holds_the_manifest_and_the_launcher_at_mcpb_pack_paths() {
    let manifest = bundle_manifest(SERVER, "0.7.0");
    let bytes = bundle_bytes(&manifest, &bundle_launcher()).expect("bytes");

    let entries = entries(bytes);
    let names: Vec<&str> = entries.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(names, ["manifest.json", BUNDLE_LAUNCHER_PATH]);
    assert_eq!(entries[0].1, manifest);
    assert_eq!(entries[1].1, bundle_launcher());
}

#[test]
fn the_bundle_is_written_inside_the_data_folder() {
    let dir = tempfile::TempDir::new().expect("temp dir");

    let written = write_bundle(
        dir.path(),
        &bundle_manifest(SERVER, "0.7.0"),
        &bundle_launcher(),
    )
    .expect("written");

    assert_eq!(written, path(dir.path()));
    assert!(written.starts_with(dir.path().join(FOLDER)));
    assert_eq!(
        written.file_name().and_then(|name| name.to_str()),
        Some("Jotra.mcpb")
    );
    let manifest: serde_json::Value =
        serde_json::from_str(&entries(std::fs::read(&written).expect("read"))[0].1).expect("json");
    assert_eq!(
        manifest["server"]["mcp_config"]["env"]["JOTRA_COMMAND"],
        SERVER
    );
}

#[test]
fn a_second_bundle_replaces_the_first() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    write_bundle(
        dir.path(),
        &bundle_manifest("/old/jotra", "0.7.0"),
        &bundle_launcher(),
    )
    .expect("first");

    let written = write_bundle(
        dir.path(),
        &bundle_manifest(SERVER, "0.7.0"),
        &bundle_launcher(),
    )
    .expect("second");

    let manifest = &entries(std::fs::read(&written).expect("read"))[0].1;
    assert!(manifest.contains(SERVER));
    assert!(!manifest.contains("/old/jotra"));
    let left: Vec<_> = std::fs::read_dir(dir.path().join(FOLDER))
        .expect("folder")
        .map(|entry| entry.expect("entry").file_name())
        .collect();
    assert_eq!(left.len(), 1);
}
