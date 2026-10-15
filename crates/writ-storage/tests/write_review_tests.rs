use tempfile::TempDir;
use writ_core::write_review::{ProposedChange, ReviewAnswer, WriteProposal};
use writ_storage::write_review::{
    answers_dir, clear_reviews, proposals_dir, read_proposal, take_answer, withdraw, write_answer,
    write_proposal,
};

fn proposal() -> WriteProposal {
    WriteProposal {
        program: "claude-code".to_string(),
        path: "/notes/Plan.md".to_string(),
        change: ProposedChange::Edit {
            before: "one\n".to_string(),
            after: "one\ntwo\n".to_string(),
        },
    }
}

fn count(dir: &std::path::Path) -> usize {
    std::fs::read_dir(dir).map_or(0, |entries| entries.count())
}

#[test]
fn a_proposal_is_read_and_its_answer_taken_once_leaving_nothing_behind() {
    let dir = TempDir::new().expect("temp dir");
    assert_eq!(read_proposal(dir.path(), "rv-1"), None);
    assert_eq!(take_answer(dir.path(), "rv-1"), None);

    write_proposal(dir.path(), "rv-1", &proposal()).expect("proposal");
    assert_eq!(read_proposal(dir.path(), "rv-1"), Some(proposal()));
    write_answer(dir.path(), "rv-1", ReviewAnswer::Apply).expect("answer");

    assert_eq!(take_answer(dir.path(), "rv-1"), Some(ReviewAnswer::Apply));
    assert_eq!(take_answer(dir.path(), "rv-1"), None);
    assert_eq!(count(&proposals_dir(dir.path())), 0);
    assert_eq!(count(&answers_dir(dir.path())), 0);
}

#[test]
fn an_answer_already_there_is_not_replaced() {
    let dir = TempDir::new().expect("temp dir");
    write_answer(dir.path(), "rv-2", ReviewAnswer::Discard).expect("answer");
    assert!(write_answer(dir.path(), "rv-2", ReviewAnswer::Apply).is_err());
    assert_eq!(take_answer(dir.path(), "rv-2"), Some(ReviewAnswer::Discard));
}

#[test]
fn an_answer_that_does_not_parse_writes_nothing() {
    let dir = TempDir::new().expect("temp dir");
    std::fs::create_dir_all(answers_dir(dir.path())).expect("folder");
    std::fs::write(answers_dir(dir.path()).join("bad-1.json"), "\"apply!\"").expect("write");
    assert_eq!(
        take_answer(dir.path(), "bad-1"),
        Some(ReviewAnswer::Discard)
    );
}

#[test]
fn a_token_that_could_name_another_file_is_never_written_or_read() {
    let dir = TempDir::new().expect("temp dir");
    std::fs::write(dir.path().join("config.toml"), "[mcp]\n").expect("settings");
    for token in ["../config", "a/b", "", "a.b"] {
        assert!(
            write_proposal(dir.path(), token, &proposal()).is_err(),
            "{token:?}"
        );
        assert!(
            write_answer(dir.path(), token, ReviewAnswer::Apply).is_err(),
            "{token:?}"
        );
        assert_eq!(read_proposal(dir.path(), token), None, "{token:?}");
        assert_eq!(take_answer(dir.path(), token), None, "{token:?}");
        withdraw(dir.path(), token);
    }
    assert!(dir.path().join("config.toml").exists());
}

#[cfg(unix)]
#[test]
fn a_proposal_and_an_answer_are_readable_by_their_owner_only() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new().expect("temp dir");
    write_proposal(dir.path(), "rv-3", &proposal()).expect("proposal");
    write_answer(dir.path(), "rv-3", ReviewAnswer::Apply).expect("answer");
    for path in [
        proposals_dir(dir.path()).join("rv-3.json"),
        answers_dir(dir.path()).join("rv-3.json"),
    ] {
        let mode = std::fs::metadata(&path).expect("meta").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "{}", path.display());
    }
}

#[test]
fn withdrawn_and_leftover_reviews_are_removed() {
    let dir = TempDir::new().expect("temp dir");
    write_proposal(dir.path(), "old-1", &proposal()).expect("proposal");
    write_proposal(dir.path(), "old-2", &proposal()).expect("proposal");
    write_answer(dir.path(), "old-2", ReviewAnswer::Apply).expect("answer");

    withdraw(dir.path(), "old-1");
    assert_eq!(read_proposal(dir.path(), "old-1"), None);

    clear_reviews(dir.path());
    assert_eq!(count(&proposals_dir(dir.path())), 0);
    assert_eq!(count(&answers_dir(dir.path())), 0);
    clear_reviews(&dir.path().join("nowhere"));
}
