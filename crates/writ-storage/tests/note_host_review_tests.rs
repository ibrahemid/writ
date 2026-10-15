//! A program's write held for the person's answer (ADR-053): nothing lands
//! but what they applied, and a file that changed while they looked is
//! refused rather than written over.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tempfile::TempDir;
use writ_core::config::FileExtension;
use writ_core::hash::digest_hex;
use writ_core::notes::host::{Capability, HostError, LastKnown, NoteHost};
use writ_core::notes::WriteOrigin;
use writ_core::write_review::{
    ProposedChange, ReviewAnswer, ReviewUnavailable, WriteProposal, WriteReview,
};
use writ_storage::note_host::NoteHostImpl;

struct Fixture {
    _dir: TempDir,
    notes: PathBuf,
}

fn fixture() -> Fixture {
    let dir = TempDir::new().expect("temp dir");
    let notes = dir.path().join("notes");
    std::fs::create_dir_all(&notes).expect("notes folder");
    Fixture { _dir: dir, notes }
}

fn host(fixture: &Fixture) -> NoteHostImpl<'static> {
    NoteHostImpl::open(
        &fixture.notes,
        None,
        [
            Capability::ReadNote,
            Capability::WriteNote,
            Capability::CreateNote,
            Capability::RenameNote,
        ]
        .into_iter()
        .collect(),
        FileExtension::Md,
    )
    .expect("open the host")
}

fn program() -> WriteOrigin {
    WriteOrigin::Mcp {
        client: "claude-code".to_string(),
    }
}

/// Answers every held write the same way, after running `meanwhile`, and
/// keeps what it was shown.
struct Person {
    answer: Result<ReviewAnswer, ReviewUnavailable>,
    meanwhile: Box<dyn Fn() + Send + Sync>,
    shown: Mutex<Vec<WriteProposal>>,
}

impl Person {
    fn answering(answer: Result<ReviewAnswer, ReviewUnavailable>) -> Self {
        Self {
            answer,
            meanwhile: Box::new(|| {}),
            shown: Mutex::new(Vec::new()),
        }
    }

    fn shown(&self) -> Vec<WriteProposal> {
        self.shown.lock().expect("shown").clone()
    }
}

impl WriteReview for Person {
    fn review(&self, proposal: &WriteProposal) -> Result<ReviewAnswer, ReviewUnavailable> {
        self.shown.lock().expect("shown").push(proposal.clone());
        (self.meanwhile)();
        self.answer
    }
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).expect("read")
}

fn files_in(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(folder)
        .expect("list the folder")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

#[test]
fn an_append_waits_for_apply_and_shows_what_it_would_change() {
    let fixture = fixture();
    let file = fixture.notes.join("Plan.md");
    std::fs::write(&file, "one\n").expect("seed");
    let person = Person::answering(Ok(ReviewAnswer::Apply));

    host(&fixture)
        .with_review(Some(&person), "claude-code")
        .append_note("Plan.md", "two", program())
        .expect("applied");

    assert_eq!(read(&file), "one\ntwo\n");
    let shown = person.shown();
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].program, "claude-code");
    assert!(shown[0].path.ends_with("Plan.md"));
    assert_eq!(
        shown[0].change,
        ProposedChange::Edit {
            before: "one\n".to_string(),
            after: "one\ntwo\n".to_string(),
        }
    );
}

#[test]
fn a_discarded_or_unanswered_write_leaves_the_file_as_it_was() {
    let fixture = fixture();
    let file = fixture.notes.join("Plan.md");
    std::fs::write(&file, "one\n").expect("seed");

    let discards = Person::answering(Ok(ReviewAnswer::Discard));
    let refused = host(&fixture)
        .with_review(Some(&discards), "claude-code")
        .replace_in_note("Plan.md", "one", "uno", program());
    assert_eq!(
        refused.map(|_| ()),
        Err(HostError::Discarded {
            path: "Plan.md".to_string()
        })
    );

    for (answer, reason) in [
        (Ok(ReviewAnswer::NotShown), ReviewUnavailable::NotShown),
        (
            Err(ReviewUnavailable::Unanswered),
            ReviewUnavailable::Unanswered,
        ),
        (
            Err(ReviewUnavailable::NotRunning),
            ReviewUnavailable::NotRunning,
        ),
    ] {
        let person = Person::answering(answer);
        let refused = host(&fixture)
            .with_review(Some(&person), "claude-code")
            .write_note("Plan.md", "replaced\n", LastKnown::Overwrite, program());
        assert_eq!(
            refused.map(|_| ()),
            Err(HostError::ReviewUnavailable {
                path: "Plan.md".to_string(),
                reason,
            })
        );
    }
    assert_eq!(read(&file), "one\n");
}

#[test]
fn a_file_that_changed_while_the_person_looked_is_refused_and_not_asked_again() {
    let fixture = fixture();
    let file = fixture.notes.join("Plan.md");
    std::fs::write(&file, "one\n").expect("seed");
    let typed = file.clone();
    let person = Person {
        meanwhile: Box::new(move || {
            std::fs::write(&typed, "one\ntyped meanwhile\n").expect("type")
        }),
        ..Person::answering(Ok(ReviewAnswer::Apply))
    };

    let refused = host(&fixture)
        .with_review(Some(&person), "claude-code")
        .append_note("Plan.md", "two", program());

    assert!(
        matches!(refused, Err(HostError::Conflict { .. })),
        "{refused:?}"
    );
    assert_eq!(read(&file), "one\ntyped meanwhile\n");
    assert_eq!(
        person.shown().len(),
        1,
        "a changed file is not built and shown again"
    );
}

#[test]
fn a_write_that_would_change_nothing_or_that_the_guard_refuses_asks_nobody() {
    let fixture = fixture();
    let file = fixture.notes.join("Plan.md");
    std::fs::write(&file, "one\n").expect("seed");
    let person = Person::answering(Ok(ReviewAnswer::Apply));
    let host = host(&fixture);
    let read_back = host.read_note("Plan.md").expect("read");

    host.with_permissions(host.permissions().clone())
        .with_review(Some(&person), "claude-code")
        .write_note("Plan.md", "one\n", LastKnown::LastSeen, program())
        .expect("nothing to change");

    let stale = writ_core::hash::sha256_bytes(b"something else");
    let refused = host
        .with_permissions(host.permissions().clone())
        .with_review(Some(&person), "claude-code")
        .write_note("Plan.md", "two\n", LastKnown::Hash(stale), program());
    assert_eq!(
        refused.map(|_| ()),
        Err(HostError::Conflict {
            path: "Plan.md".to_string(),
            conflict_copy: None,
        })
    );
    assert_eq!(files_in(&fixture.notes), ["Plan.md"]);
    assert_eq!(read(&file), "one\n");

    assert!(person.shown().is_empty());
    assert_eq!(
        read_back.hash,
        digest_hex(writ_core::hash::sha256_bytes(b"one\n"))
    );
}

#[test]
fn a_write_over_a_file_changed_since_the_read_sets_nothing_beside_it() {
    let fixture = fixture();
    let file = fixture.notes.join("Plan.md");
    std::fs::write(&file, "one\n").expect("seed");
    let host = host(&fixture);
    let read_back = host.read_note("Plan.md").expect("read");
    std::fs::write(&file, "one\ntyped since\n").expect("type");
    let person = Person::answering(Err(ReviewUnavailable::NotRunning));
    let reviewed = host
        .with_permissions(host.permissions().clone())
        .with_review(Some(&person), "claude-code");
    let conflict = Err(HostError::Conflict {
        path: "Plan.md".to_string(),
        conflict_copy: None,
    });

    let last_seen =
        reviewed.write_note("Plan.md", "agent rewrite\n", LastKnown::LastSeen, program());
    assert_eq!(last_seen.map(|_| ()), conflict);

    let read_hash = writ_core::hash::sha256_bytes(b"one\n");
    assert_eq!(read_back.hash, digest_hex(read_hash));
    let stale = reviewed.write_note(
        "Plan.md",
        "agent rewrite\n",
        LastKnown::Hash(read_hash),
        program(),
    );
    assert_eq!(stale.map(|_| ()), conflict);

    let replaced = reviewed.replace_note(
        "Plan.md",
        "agent rewrite\n",
        LastKnown::Hash(read_hash),
        program(),
    );
    assert_eq!(replaced.map(|_| ()), conflict);

    assert!(person.shown().is_empty());
    assert_eq!(files_in(&fixture.notes), ["Plan.md"]);
    assert_eq!(read(&file), "one\ntyped since\n");
}

#[test]
fn an_applied_write_over_a_file_that_changed_meanwhile_sets_nothing_beside_it() {
    let fixture = fixture();
    let file = fixture.notes.join("Plan.md");
    let conflict = Err(HostError::Conflict {
        path: "Plan.md".to_string(),
        conflict_copy: None,
    });

    for replace in [false, true] {
        std::fs::write(&file, "one\n").expect("seed");
        let typed = file.clone();
        let person = Person {
            meanwhile: Box::new(move || {
                std::fs::write(&typed, "one\ntyped meanwhile\n").expect("type")
            }),
            ..Person::answering(Ok(ReviewAnswer::Apply))
        };
        let reviewed = host(&fixture).with_review(Some(&person), "claude-code");

        let refused = if replace {
            reviewed.replace_note(
                "Plan.md",
                "agent rewrite\n",
                LastKnown::Overwrite,
                program(),
            )
        } else {
            reviewed.write_note(
                "Plan.md",
                "agent rewrite\n",
                LastKnown::Overwrite,
                program(),
            )
        };

        assert_eq!(refused.map(|_| ()), conflict, "replace_note: {replace}");
        assert_eq!(person.shown().len(), 1);
        assert_eq!(files_in(&fixture.notes), ["Plan.md"]);
        assert_eq!(read(&file), "one\ntyped meanwhile\n");
    }
}

#[test]
fn a_new_file_and_a_rename_are_held_too() {
    let fixture = fixture();
    std::fs::write(fixture.notes.join("Plan.md"), "one\n").expect("seed");
    let person = Person::answering(Ok(ReviewAnswer::Discard));

    let created = host(&fixture)
        .with_review(Some(&person), "claude-code")
        .create_note("Launch", "hello\n", program());
    assert!(
        matches!(created, Err(HostError::Discarded { .. })),
        "{created:?}"
    );
    assert!(!fixture.notes.join("Launch.md").exists());

    let renamed = host(&fixture)
        .with_review(Some(&person), "claude-code")
        .rename_note("Plan.md", "Roadmap", program());
    assert!(
        matches!(renamed, Err(HostError::Discarded { .. })),
        "{renamed:?}"
    );
    assert!(fixture.notes.join("Plan.md").exists());

    let shown = person.shown();
    assert_eq!(
        shown[0].change,
        ProposedChange::Create {
            after: "hello\n".to_string()
        }
    );
    assert!(shown[0].path.ends_with("Launch.md"), "{}", shown[0].path);
    assert_eq!(
        shown[1].change,
        ProposedChange::Rename {
            to: "Roadmap.md".to_string()
        }
    );
}

#[test]
fn a_handle_with_no_reviewer_writes_without_asking() {
    let fixture = fixture();
    let file = fixture.notes.join("Plan.md");
    std::fs::write(&file, "one\n").expect("seed");
    host(&fixture)
        .with_review(None, "claude-code")
        .append_note("Plan.md", "two", program())
        .expect("written");
    assert_eq!(read(&file), "one\ntwo\n");
}
