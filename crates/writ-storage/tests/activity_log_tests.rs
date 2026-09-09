//! The activity log: appending, rotation, malformed lines and two appenders.

use std::io::Write;

use writ_core::activity::{ActivityRecord, Actor, Decision};
use writ_storage::activity_log::{self, current_path, previous_path, read_recent, ROTATE_AT_BYTES};

fn record(action: &str) -> ActivityRecord {
    ActivityRecord::now(
        Actor::Client {
            name: "Claude Code".to_string(),
            version: Some("1.2.3".to_string()),
        },
        action,
        Decision::Allow,
    )
    .with_path("Ideas/Tessera.md")
    .with_bytes(412)
}

#[test]
fn a_record_appended_comes_back() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    activity_log::append(dir.path(), &record("read_note")).expect("append");

    let recent = read_recent(dir.path(), 10);
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].action, "read_note");
    assert_eq!(recent[0].decision, Decision::Allow);
}

#[test]
fn a_folder_that_does_not_exist_yet_is_created() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let nested = dir.path().join("data").join("writ");
    activity_log::append(&nested, &record("read_note")).expect("append");

    assert!(current_path(&nested).is_file());
}

#[test]
fn records_come_back_newest_first_and_held_to_the_limit() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    for index in 0..10 {
        activity_log::append(dir.path(), &record(&format!("call_{index}"))).expect("append");
    }

    let recent = read_recent(dir.path(), 3);
    let actions: Vec<&str> = recent.iter().map(|r| r.action.as_str()).collect();
    assert_eq!(actions, ["call_9", "call_8", "call_7"]);
}

#[test]
fn a_limit_of_zero_reads_nothing() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    activity_log::append(dir.path(), &record("read_note")).expect("append");

    assert!(read_recent(dir.path(), 0).is_empty());
}

#[test]
fn an_empty_folder_reads_as_no_activity() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    assert!(read_recent(dir.path(), 50).is_empty());
}

#[test]
fn a_log_over_the_cap_rotates_and_the_newest_records_still_read_back() {
    let dir = tempfile::TempDir::new().expect("temp dir");

    // Seed past the cap in one write rather than six megabytes of appends: the
    // file is plain JSONL and the rotation decision is made on its length.
    let line = format!(
        "{}\n",
        serde_json::to_string(&record("seeded")).expect("serialise")
    );
    let repeats = (ROTATE_AT_BYTES as usize / line.len()) + 1_000;
    std::fs::write(current_path(dir.path()), line.repeat(repeats)).expect("seed");
    assert!(
        std::fs::metadata(current_path(dir.path()))
            .expect("meta")
            .len()
            > ROTATE_AT_BYTES
    );

    activity_log::append(dir.path(), &record("after_rotation")).expect("append");

    assert!(previous_path(dir.path()).is_file(), "the generation kept");
    let current_len = std::fs::metadata(current_path(dir.path()))
        .expect("meta")
        .len();
    assert!(current_len < ROTATE_AT_BYTES, "{current_len}");

    let recent = read_recent(dir.path(), 3);
    assert_eq!(recent[0].action, "after_rotation");
    // The rest come from the generation behind it, so nothing is lost yet.
    assert_eq!(recent[1].action, "seeded");
    assert_eq!(recent.len(), 3);
}

#[test]
fn only_one_generation_is_kept() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    std::fs::write(previous_path(dir.path()), "").expect("seed old");
    let line = format!(
        "{}\n",
        serde_json::to_string(&record("seeded")).expect("serialise")
    );
    let repeats = (ROTATE_AT_BYTES as usize / line.len()) + 1;
    std::fs::write(current_path(dir.path()), line.repeat(repeats)).expect("seed");

    activity_log::append(dir.path(), &record("after_rotation")).expect("append");

    let generations: Vec<_> = std::fs::read_dir(dir.path())
        .expect("list")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .filter(|name| name.ends_with(".jsonl"))
        .collect();
    assert_eq!(generations.len(), 2, "{generations:?}");
}

#[test]
fn a_malformed_line_is_skipped_and_the_rest_parse() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    activity_log::append(dir.path(), &record("first")).expect("append");

    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(current_path(dir.path()))
        .expect("open");
    file.write_all(b"{\"at\": not json at all\n")
        .expect("write");
    file.write_all(b"\n").expect("write");
    drop(file);

    activity_log::append(dir.path(), &record("second")).expect("append");

    let recent = read_recent(dir.path(), 10);
    let actions: Vec<&str> = recent.iter().map(|r| r.action.as_str()).collect();
    assert_eq!(actions, ["second", "first"]);
}

#[test]
fn clearing_forgets_both_generations() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    activity_log::append(dir.path(), &record("read_note")).expect("append");
    std::fs::write(previous_path(dir.path()), "").expect("seed old");

    activity_log::clear(dir.path()).expect("clear");

    assert!(!current_path(dir.path()).exists());
    assert!(!previous_path(dir.path()).exists());
    assert!(read_recent(dir.path(), 10).is_empty());
}

#[test]
fn clearing_a_log_that_was_never_written_is_not_an_error() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    activity_log::clear(dir.path()).expect("clear");
}

#[test]
fn two_appenders_writing_at_once_leave_one_line_per_record() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let root = dir.path().to_path_buf();

    let mut threads = Vec::new();
    for side in ["a", "b"] {
        let root = root.clone();
        threads.push(std::thread::spawn(move || {
            for index in 0..500 {
                activity_log::append(&root, &record(&format!("{side}_{index}"))).expect("append");
            }
        }));
    }
    for thread in threads {
        thread.join().expect("thread");
    }

    let text = std::fs::read_to_string(current_path(&root)).expect("read");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 1_000);
    for line in &lines {
        serde_json::from_str::<ActivityRecord>(line).expect("every line parses whole");
    }

    let mut actions: Vec<String> = lines
        .iter()
        .map(|line| {
            serde_json::from_str::<ActivityRecord>(line)
                .expect("parse")
                .action
        })
        .collect();
    actions.sort();
    actions.dedup();
    assert_eq!(actions.len(), 1_000, "every record survived exactly once");
}

#[test]
fn a_file_longer_than_one_read_window_still_answers_with_the_newest() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let mut seeded = String::new();
    for index in 0..2_000 {
        let line = serde_json::to_string(&record(&format!("call_{index}"))).expect("serialise");
        seeded.push_str(&line);
        seeded.push('\n');
    }
    assert!(
        seeded.len() > 256 * 1024,
        "the seed must be several read windows long"
    );
    std::fs::create_dir_all(dir.path()).expect("dir");
    std::fs::write(current_path(dir.path()), &seeded).expect("seed");

    let recent = read_recent(dir.path(), 10);
    let actions: Vec<&str> = recent.iter().map(|r| r.action.as_str()).collect();
    assert_eq!(actions[0], "call_1999");
    assert_eq!(actions[9], "call_1990");
}

#[test]
fn a_window_widens_until_it_holds_the_whole_limit() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let mut seeded = String::new();
    let mut index = 0;
    while seeded.len() < 200 * 1024 {
        let line = serde_json::to_string(&record(&format!("call_{index}"))).expect("serialise");
        seeded.push_str(&line);
        seeded.push('\n');
        index += 1;
    }
    std::fs::create_dir_all(dir.path()).expect("dir");
    std::fs::write(current_path(dir.path()), &seeded).expect("seed");

    // More records than one window holds, so the window is widened rather than
    // answering short.
    let recent = read_recent(dir.path(), 900);
    assert_eq!(recent.len(), 900);
    for (offset, entry) in recent.iter().enumerate() {
        assert_eq!(entry.action, format!("call_{}", index - 1 - offset));
    }
}

#[test]
fn a_log_read_whole_still_stops_at_the_limit() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    for index in 0..5 {
        activity_log::append(dir.path(), &record(&format!("call_{index}"))).expect("append");
    }

    let recent = read_recent(dir.path(), 3);
    let actions: Vec<&str> = recent.iter().map(|r| r.action.as_str()).collect();
    assert_eq!(actions, ["call_4", "call_3", "call_2"]);
}

/// Fills the current file to just under the cap with lines that parse, so the
/// next few appends are the ones that cross it.
fn fill_to_just_under_the_cap(dir: &std::path::Path) {
    std::fs::create_dir_all(dir).expect("dir");
    let mut line = serde_json::to_vec(&record("filler")).expect("serialise");
    line.push(b'\n');

    let mut file = std::fs::File::create(current_path(dir)).expect("create");
    let mut written = 0u64;
    while written + line.len() as u64 + 4_096 < ROTATE_AT_BYTES {
        file.write_all(&line).expect("write");
        written += line.len() as u64;
    }
    file.flush().expect("flush");
}

/// Two writers reaching the cap together used to both rename the current file.
/// The loser either found the source gone and failed the append, or renamed a
/// nearly empty current file over the generation the winner had just filled.
#[test]
fn a_rotation_under_two_appenders_loses_no_record() {
    let dir = tempfile::TempDir::new().expect("temp dir");
    let root = dir.path().to_path_buf();
    fill_to_just_under_the_cap(&root);

    let failures: Vec<String> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..2)
            .map(|writer| {
                let root = root.clone();
                scope.spawn(move || {
                    let mut failed = Vec::new();
                    for index in 0..20 {
                        let entry = record(&format!("w{writer}_{index}"));
                        if let Err(error) = activity_log::append(&root, &entry) {
                            failed.push(format!("{error}"));
                        }
                    }
                    failed
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|handle| handle.join().expect("writer"))
            .collect()
    });

    assert!(failures.is_empty(), "appends failed: {failures:?}");

    let recent = read_recent(&root, 10_000);
    let written: std::collections::HashSet<&str> = recent
        .iter()
        .map(|entry| entry.action.as_str())
        .filter(|action| action.starts_with('w'))
        .collect();
    assert_eq!(
        written.len(),
        40,
        "records went missing across the rotation: {} of 40",
        written.len()
    );
}
