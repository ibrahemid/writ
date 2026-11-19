//! Every reader of a value written under the old name still reads it, and
//! every writer uses the new one (ADR-059 decision 5).

use std::collections::HashMap;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use chrono::{TimeZone, Utc};
use jotra_core::activity::{canonical_action, ActivityRecord};
use jotra_core::chat::{parse_proposals, AttachedNote, ProposalFilter};
use jotra_core::jotra_link::{is_jotra_link, link_args, open_url, parse, JotraLink};
use jotra_core::notes::{
    is_minted_title, minted_stem, refuse_notes_root, DataFolderGuard, NotesRootRefusal,
};
use jotra_core::startup::{classify_data_dir, DataDirVerdict, Platform};
use jotra_core::user_variables::{read_user_variable, UserVariable};
use jotra_core::workspace::{
    is_ignored_name, is_notes_ignored_name, is_temp_file_name, TEMP_FILE_PREFIX,
};

const HOME: &str = "/home/u";

fn home() -> PathBuf {
    PathBuf::from(HOME)
}

#[test]
fn a_link_under_the_old_scheme_opens_what_it_names() {
    assert_eq!(
        parse("writ://open?path=Plan.md&line=12"),
        Ok(JotraLink::Open {
            path: "Plan.md".to_string(),
            line: Some(12),
        })
    );
    assert_eq!(
        parse("WRIT://new?name=Ideas"),
        Ok(JotraLink::New {
            name: Some("Ideas".to_string()),
        })
    );
}

#[test]
fn a_link_under_the_old_scheme_is_followed_from_launch_arguments() {
    let args = [
        "--hidden",
        "writ://open?path=Plan.md",
        "jotra://new",
        "writ-preview://x",
        "writing.md",
    ]
    .map(OsString::from);
    assert_eq!(
        link_args(args),
        vec![
            "writ://open?path=Plan.md".to_string(),
            "jotra://new".to_string()
        ]
    );
    assert!(is_jotra_link("writ://open?path=Plan.md"));
    assert!(is_jotra_link("Writ:open"));
    assert!(!is_jotra_link("writ-proposal:x"));
    assert!(!is_jotra_link("writ"));
}

#[test]
fn a_new_link_is_written_under_the_new_scheme() {
    let url = open_url("Plan.md", Some(3)).expect("a link");
    assert!(url.starts_with("jotra://"), "{url}");
}

fn launch_note() -> Vec<AttachedNote> {
    vec![AttachedNote {
        path: "Ideas/Launch.md".to_string(),
        prompt_path: "Ideas/Launch.md".to_string(),
        text: "The old text.\n".to_string(),
        before_hash: "hash".to_string(),
    }]
}

const OLD_FENCE_REPLY: &str = "Before.\n\n```writ-proposal path=\"Ideas/Launch.md\" summary=\"Fold\"\nThe proposed text.\n```\n\nAfter.\n";

#[test]
fn a_proposal_under_the_old_fence_word_is_a_proposal() {
    let parsed = parse_proposals(OLD_FENCE_REPLY, &launch_note(), false);
    assert_eq!(parsed.dropped, Vec::new());
    assert_eq!(parsed.proposals.len(), 1);
    assert_eq!(parsed.proposals[0].path, "Ideas/Launch.md");
    assert_eq!(parsed.proposals[0].new_content, "The proposed text.\n");
    assert_eq!(parsed.proposals[0].summary, "Fold");
}

#[test]
fn a_proposal_under_the_old_fence_word_is_withheld_from_the_stream() {
    let whole = {
        let mut filter = ProposalFilter::new();
        let mut out = filter.push(OLD_FENCE_REPLY);
        out.push_str(&filter.finish());
        out
    };
    let by_char = {
        let mut filter = ProposalFilter::new();
        let mut out = String::new();
        for c in OLD_FENCE_REPLY.chars() {
            out.push_str(&filter.push(&c.to_string()));
        }
        out.push_str(&filter.finish());
        out
    };
    assert_eq!(whole, by_char);
    assert!(
        whole.contains("Before.") && whole.contains("After."),
        "{whole}"
    );
    assert!(!whole.contains("The proposed text."), "{whole}");
    assert!(!whole.contains('`'), "{whole}");
}

#[test]
fn a_stem_minted_under_the_old_name_counts_as_minted() {
    assert!(is_minted_title("writ-260730-1015"));
    assert!(is_minted_title("jotra-260730-1015"));
    assert!(!is_minted_title("writing-260730-1015"));

    let now = Utc.with_ymd_and_hms(2026, 7, 30, 10, 15, 0).unwrap();
    assert!(minted_stem(now).starts_with("jotra-"));
}

#[test]
fn a_temp_file_from_before_the_rename_is_ignored() {
    for name in [".writ-tmp-a1b2c3", ".WRIT-TMP-Plan.md"] {
        assert!(is_temp_file_name(name), "{name}");
        assert!(is_notes_ignored_name(name), "{name}");
        assert!(is_ignored_name(name), "{name}");
    }
    assert!(!is_temp_file_name("writ-tmp-plan.md"));
    assert_eq!(TEMP_FILE_PREFIX, ".jotra-tmp-");
}

#[test]
fn an_activity_line_under_the_old_tool_name_reads_as_the_new_one() {
    assert_eq!(canonical_action("open_in_writ"), "open_in_jotra");
    assert_eq!(canonical_action("open_in_jotra"), "open_in_jotra");
    assert_eq!(canonical_action("save"), "save");

    let line = r#"{"at":"2026-09-09T10:30:00.000Z","actor":{"kind":"app"},"action":"open_in_writ","path":null,"decision":"allow","bytes":null}"#;
    let record: ActivityRecord = serde_json::from_str(line).expect("parse");
    assert_eq!(record.action, "open_in_jotra");
    let written = serde_json::to_string(&record).expect("serialize");
    assert!(written.contains("\"open_in_jotra\""), "{written}");
}

fn lookup<'a>(env: &'a HashMap<&str, &str>) -> impl Fn(&str) -> Option<OsString> + 'a {
    move |name| env.get(name).map(OsString::from)
}

#[test]
fn a_variable_set_under_the_old_name_is_read() {
    let env = HashMap::from([("WRIT_DATA_DIR", "/data/old")]);
    assert_eq!(
        read_user_variable(UserVariable::DataDir, lookup(&env)).as_deref(),
        Some(OsStr::new("/data/old"))
    );

    let env = HashMap::from([("WRIT_NOTES_DIR", "/notes/old")]);
    assert_eq!(
        read_user_variable(UserVariable::NotesDir, lookup(&env)).as_deref(),
        Some(OsStr::new("/notes/old"))
    );

    let env = HashMap::from([("WRIT_GUI_BIN", "/bin/old")]);
    assert_eq!(
        read_user_variable(UserVariable::GuiBin, lookup(&env)).as_deref(),
        Some(OsStr::new("/bin/old"))
    );
}

#[test]
fn the_new_name_wins_and_a_blank_one_falls_through() {
    let env = HashMap::from([
        ("JOTRA_DATA_DIR", "/data/new"),
        ("WRIT_DATA_DIR", "/data/old"),
    ]);
    assert_eq!(
        read_user_variable(UserVariable::DataDir, lookup(&env)).as_deref(),
        Some(OsStr::new("/data/new"))
    );

    let env = HashMap::from([("JOTRA_DATA_DIR", "  "), ("WRIT_DATA_DIR", "/data/old")]);
    assert_eq!(
        read_user_variable(UserVariable::DataDir, lookup(&env)).as_deref(),
        Some(OsStr::new("/data/old"))
    );

    let env = HashMap::from([("JOTRA_DATA_DIR", ""), ("WRIT_DATA_DIR", " ")]);
    assert_eq!(
        read_user_variable(UserVariable::DataDir, lookup(&env)),
        None
    );
}

#[test]
fn both_variable_names_are_listed_new_first() {
    assert_eq!(
        UserVariable::DataDir.names(),
        ["JOTRA_DATA_DIR", "WRIT_DATA_DIR"]
    );
    assert_eq!(
        UserVariable::NotesDir.names(),
        ["JOTRA_NOTES_DIR", "WRIT_NOTES_DIR"]
    );
    assert_eq!(
        UserVariable::GuiBin.names(),
        ["JOTRA_GUI_BIN", "WRIT_GUI_BIN"]
    );
}

fn guard(in_force: &Path) -> DataFolderGuard<'_> {
    DataFolderGuard {
        in_force,
        home: Some(Path::new(HOME)),
    }
}

#[test]
fn a_notes_folder_may_hold_neither_home_data_folder() {
    let current = home().join(".jotra");
    let old = home().join(".writ");
    let notes = home().join("Writ");

    for in_force in [&current, &old] {
        for refused in [
            old.clone(),
            old.join("archive"),
            current.clone(),
            current.join("archive"),
            home(),
        ] {
            assert_eq!(
                refuse_notes_root(&refused, &notes, guard(in_force)),
                Some(NotesRootRefusal::HoldsJotraData),
                "{} with {} in force",
                refused.display(),
                in_force.display()
            );
        }
        assert_eq!(
            refuse_notes_root(&home().join("Documents"), &notes, guard(in_force)),
            None
        );
    }
}

#[test]
fn the_old_notes_default_beside_an_overridden_data_folder_is_allowed() {
    let data = home().join(".writ-dev");
    let notes = home().join("Notes");
    assert_eq!(
        refuse_notes_root(&data.join("Writ"), &notes, guard(&data)),
        None
    );
    assert_eq!(
        refuse_notes_root(&data.join("Writ").join("deeper"), &notes, guard(&data)),
        Some(NotesRootRefusal::HoldsJotraData)
    );
}

#[test]
fn the_old_notes_default_inside_the_data_directory_passes_the_startup_guard() {
    let notes = home().join(".writ-dev").join("Writ");
    let verdict = classify_data_dir(
        Platform::Linux,
        &home().join(".writ-dev"),
        Some(&home()),
        Some(&notes),
        &[],
    );
    assert_eq!(verdict, DataDirVerdict::Ok);

    let notes = home().join(".writ").join("notes");
    let verdict = classify_data_dir(
        Platform::Linux,
        &home().join(".writ"),
        Some(&home()),
        Some(&notes),
        &[],
    );
    assert_eq!(
        verdict,
        DataDirVerdict::InsideNotesFolder { notes_root: notes }
    );
}
