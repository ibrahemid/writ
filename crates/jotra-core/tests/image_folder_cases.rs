//! The other half is src/__tests__/lib/image-folder.fixture.test.ts, reading
//! the same file: a folder the settings field accepts is one the host saves
//! images into.

use jotra_core::config::ImageFolder;
use serde::Deserialize;

#[derive(Deserialize)]
struct Fixture {
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    value: String,
    folder: Option<String>,
    error: Option<String>,
}

fn cases() -> Vec<Case> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/image-folders.json"
    );
    let text = std::fs::read_to_string(path).expect("fixture is readable");
    serde_json::from_str::<Fixture>(&text)
        .expect("fixture parses")
        .cases
}

#[test]
fn every_image_folder_case_reads_as_the_fixture_says() {
    for case in cases() {
        let read = ImageFolder::parse(&case.value);
        match (&case.folder, &case.error) {
            (Some(folder), None) => assert_eq!(
                read.as_ref().map(ImageFolder::as_str),
                Ok(folder.as_str()),
                "{:?}",
                case.value
            ),
            (None, Some(error)) => assert_eq!(
                read.map_err(|refusal| refusal.code()),
                Err(error.as_str()),
                "{:?}",
                case.value
            ),
            _ => panic!("case {:?} names neither a folder nor an error", case.value),
        }
    }
}
