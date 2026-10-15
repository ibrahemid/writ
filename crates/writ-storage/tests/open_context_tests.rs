use tempfile::TempDir;
use writ_core::open_context::{OpenContext, Selected, TextPoint};
use writ_storage::open_context::{
    answer_path, answers_dir, clear_answers, take_answer, withdraw_answer, write_answer,
};

fn context() -> OpenContext {
    OpenContext {
        path: Some("/notes/Plan.md".to_string()),
        selections: vec![Selected {
            text: "launch the site".to_string(),
            from: TextPoint { line: 2, column: 1 },
            to: TextPoint {
                line: 2,
                column: 16,
            },
        }],
        cursor: Some(TextPoint {
            line: 2,
            column: 16,
        }),
        unsaved: true,
        truncated: false,
    }
}

#[test]
fn an_answer_is_taken_once_and_leaves_nothing_behind() {
    let dir = TempDir::new().expect("temp dir");
    assert_eq!(take_answer(dir.path(), "abc-1"), None);

    write_answer(dir.path(), "abc-1", &context()).expect("write");
    assert_eq!(take_answer(dir.path(), "abc-1"), Some(context()));
    assert_eq!(take_answer(dir.path(), "abc-1"), None);
    assert_eq!(
        std::fs::read_dir(answers_dir(dir.path()))
            .expect("the folder")
            .count(),
        0
    );
}

#[test]
fn a_token_that_could_name_another_file_is_never_written_or_read() {
    let dir = TempDir::new().expect("temp dir");
    std::fs::write(dir.path().join("config.toml"), "[mcp]\n").expect("settings");
    for token in ["../config", "a/b", "", "a.b"] {
        assert!(answer_path(dir.path(), token).is_none(), "{token:?}");
        assert!(
            write_answer(dir.path(), token, &context()).is_err(),
            "{token:?}"
        );
        assert_eq!(take_answer(dir.path(), token), None, "{token:?}");
    }
    assert!(dir.path().join("config.toml").exists());
}

#[test]
fn an_answer_already_there_is_not_replaced() {
    let dir = TempDir::new().expect("temp dir");
    write_answer(dir.path(), "abc-2", &context()).expect("write");
    assert!(write_answer(dir.path(), "abc-2", &OpenContext::nothing_open()).is_err());
    assert_eq!(take_answer(dir.path(), "abc-2"), Some(context()));
}

#[cfg(unix)]
#[test]
fn an_answer_is_readable_by_its_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new().expect("temp dir");
    write_answer(dir.path(), "abc-3", &context()).expect("write");
    let path = answer_path(dir.path(), "abc-3").expect("path");
    let mode = std::fs::metadata(path).expect("meta").permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}

#[test]
fn answers_left_from_an_earlier_run_and_withdrawn_ones_are_removed() {
    let dir = TempDir::new().expect("temp dir");
    write_answer(dir.path(), "old-1", &context()).expect("write");
    write_answer(dir.path(), "old-2", &context()).expect("write");
    withdraw_answer(dir.path(), "old-1");
    assert!(!answer_path(dir.path(), "old-1").expect("path").exists());

    clear_answers(dir.path());
    assert_eq!(take_answer(dir.path(), "old-2"), None);
    clear_answers(&dir.path().join("nowhere"));
}

#[test]
fn an_answer_that_does_not_parse_reads_as_no_file_open() {
    let dir = TempDir::new().expect("temp dir");
    std::fs::create_dir_all(answers_dir(dir.path())).expect("folder");
    let path = answer_path(dir.path(), "bad-1").expect("path");
    std::fs::write(&path, "{not json").expect("write");
    assert_eq!(
        take_answer(dir.path(), "bad-1"),
        Some(OpenContext::nothing_open())
    );
    assert!(!path.exists());
}
