//! What the conversation store writes, reads and refuses (ADR-040 section 8).

use std::fs;
use std::path::Path;

use serde_json::Value;
use tempfile::TempDir;
use writ_core::chat::{
    AssistantReply, AttachmentRef, Conversation, ProposalStatus, StoredProposal,
    MAX_CONVERSATION_BYTES,
};
use writ_storage::chat_store::{ChatStore, ChatStoreError};

const ID_A: &str = "0b7d6b7a-1111-4b6a-9d5e-000000000001";
const ID_B: &str = "0b7d6b7a-2222-4b6a-9d5e-000000000002";
const ID_C: &str = "0b7d6b7a-3333-4b6a-9d5e-000000000003";

fn conversation(id: &str, updated_at: &str) -> Conversation {
    let mut held = Conversation::new(
        id.to_string(),
        "2026-09-15T10:00:00+00:00".to_string(),
        "anthropic".to_string(),
        "claude-sonnet-5".to_string(),
    );
    held.updated_at = updated_at.to_string();
    held
}

fn chats_dir(root: &Path) -> std::path::PathBuf {
    root.join("chats")
}

/// Every key in the document, at every depth.
fn keys(value: &Value, found: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                found.push(key.clone());
                keys(child, found);
            }
        }
        Value::Array(items) => {
            for item in items {
                keys(item, found);
            }
        }
        _ => {}
    }
}

#[test]
fn a_created_conversation_is_listed() {
    let dir = TempDir::new().unwrap();
    let store = ChatStore::new(dir.path());

    let made = store.create("anthropic", "claude-sonnet-5").unwrap();
    let listed = store.list().unwrap();

    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, made.id);
    assert_eq!(listed[0].title, made.title);
    assert_eq!(listed[0].turns, 0);
    assert_eq!(listed[0].created_at, made.created_at);
}

#[test]
fn a_data_directory_with_no_chats_folder_lists_nothing() {
    let dir = TempDir::new().unwrap();
    let store = ChatStore::new(dir.path());

    assert_eq!(store.list().unwrap().len(), 0);
    assert!(!chats_dir(dir.path()).exists());
}

#[test]
fn the_list_runs_from_the_most_recently_changed() {
    let dir = TempDir::new().unwrap();
    let store = ChatStore::new(dir.path());

    store
        .save(&conversation(ID_A, "2026-09-10T09:00:00+00:00"))
        .unwrap();
    store
        .save(&conversation(ID_B, "2026-09-12T09:00:00+00:00"))
        .unwrap();
    store
        .save(&conversation(ID_C, "2026-09-11T09:00:00+00:00"))
        .unwrap();

    let ids: Vec<String> = store.list().unwrap().into_iter().map(|c| c.id).collect();
    assert_eq!(
        ids,
        vec![ID_B.to_string(), ID_C.to_string(), ID_A.to_string()]
    );
}

#[test]
fn a_saved_conversation_loads_back_as_it_was() {
    let dir = TempDir::new().unwrap();
    let store = ChatStore::new(dir.path());

    let mut held = conversation(ID_A, "2026-09-15T10:00:00+00:00");
    held.push_user(
        "Tighten the opening.".to_string(),
        vec![AttachmentRef {
            path: "Ideas/Launch.md".to_string(),
            bytes: 42,
            hash: "abcd".to_string(),
        }],
        "2026-09-15T10:01:00+00:00".to_string(),
    );
    held.push_assistant(
        "Here is a shorter opening.".to_string(),
        AssistantReply {
            proposals: vec![StoredProposal {
                path: "Ideas/Launch.md".to_string(),
                summary: "Shorter opening".to_string(),
                before_hash: "abcd".to_string(),
                new_content: "Shorter.".to_string(),
                status: ProposalStatus::Pending,
            }],
            ..AssistantReply::default()
        },
        None,
        "2026-09-15T10:02:00+00:00".to_string(),
    );
    store.save(&held).unwrap();

    assert_eq!(store.load(ID_A).unwrap(), held);
    assert_eq!(store.list().unwrap()[0].turns, 2);
}

#[test]
fn a_save_leaves_no_temp_file_and_replaces_the_whole_file() {
    let dir = TempDir::new().unwrap();
    let store = ChatStore::new(dir.path());

    let mut held = conversation(ID_A, "2026-09-15T10:00:00+00:00");
    held.push_user(
        "A long first question that names the conversation.".to_string(),
        Vec::new(),
        "2026-09-15T10:01:00+00:00".to_string(),
    );
    store.save(&held).unwrap();

    let mut shorter = conversation(ID_A, "2026-09-15T11:00:00+00:00");
    shorter.title = "Hi".to_string();
    store.save(&shorter).unwrap();

    let files: Vec<String> = fs::read_dir(chats_dir(dir.path()))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name != "index.json")
        .collect();
    assert_eq!(files, vec![format!("{ID_A}.json")]);

    let loaded = store.load(ID_A).unwrap();
    assert_eq!(loaded, shorter);
    assert!(loaded.turns.is_empty());
}

#[test]
fn a_rename_takes_the_new_title_and_stamps_the_file() {
    let dir = TempDir::new().unwrap();
    let store = ChatStore::new(dir.path());
    store
        .save(&conversation(ID_A, "2026-09-15T10:00:00+00:00"))
        .unwrap();

    let renamed = store.rename(ID_A, "  Launch copy  ").unwrap();

    assert_eq!(renamed.title, "Launch copy");
    assert_ne!(renamed.updated_at, "2026-09-15T10:00:00+00:00");
    assert_eq!(store.load(ID_A).unwrap().title, "Launch copy");
}

#[test]
fn a_rename_to_nothing_keeps_the_title_it_had() {
    let dir = TempDir::new().unwrap();
    let store = ChatStore::new(dir.path());
    let mut held = conversation(ID_A, "2026-09-15T10:00:00+00:00");
    held.title = "Launch copy".to_string();
    store.save(&held).unwrap();

    let renamed = store.rename(ID_A, "   ").unwrap();

    assert_eq!(renamed.title, "Launch copy");
    assert_eq!(store.load(ID_A).unwrap().title, "Launch copy");
}

#[test]
fn a_rename_of_a_conversation_that_is_gone_is_not_found() {
    let dir = TempDir::new().unwrap();
    let store = ChatStore::new(dir.path());

    assert!(matches!(
        store.rename(ID_A, "Launch copy"),
        Err(ChatStoreError::NotFound(_))
    ));
}

#[test]
fn a_deleted_conversation_is_gone_and_deleting_it_again_is_not_found() {
    let dir = TempDir::new().unwrap();
    let store = ChatStore::new(dir.path());
    store
        .save(&conversation(ID_A, "2026-09-15T10:00:00+00:00"))
        .unwrap();

    store.delete(ID_A).unwrap();

    assert!(store.list().unwrap().is_empty());
    assert!(matches!(
        store.delete(ID_A),
        Err(ChatStoreError::NotFound(_))
    ));
    assert!(matches!(store.load(ID_A), Err(ChatStoreError::NotFound(_))));
}

#[test]
fn an_id_that_is_not_a_uuid_names_no_file() {
    let dir = TempDir::new().unwrap();
    let store = ChatStore::new(dir.path());

    for id in ["../config", "..", "", "chat-1", "0b7d6b7a-1111"] {
        assert!(
            matches!(store.load(id), Err(ChatStoreError::InvalidId(_))),
            "{id} loaded"
        );
        assert!(
            matches!(
                store.rename(id, "Launch"),
                Err(ChatStoreError::InvalidId(_))
            ),
            "{id} renamed"
        );
        assert!(
            matches!(store.delete(id), Err(ChatStoreError::InvalidId(_))),
            "{id} deleted"
        );
    }
}

/// A UUID has four spellings and one value. The file is named by the value,
/// so a caller that hands back the braced or URN form of an id it was given
/// reads and writes the conversation it means.
#[test]
fn the_spellings_of_one_id_name_one_file() {
    let dir = TempDir::new().unwrap();
    let store = ChatStore::new(dir.path());
    store
        .save(&conversation(ID_A, "2026-09-15T10:00:00+00:00"))
        .unwrap();

    let braced = format!("{{{ID_A}}}");
    let urn = format!("urn:uuid:{ID_A}");
    let unhyphenated = ID_A.replace('-', "");

    for spelling in [&braced, &urn, &unhyphenated] {
        assert_eq!(store.load(spelling).unwrap().id, ID_A, "{spelling} loaded");
        store.rename(spelling, "Launch").unwrap();
    }

    let files: Vec<String> = fs::read_dir(chats_dir(dir.path()))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name != "index.json")
        .collect();
    assert_eq!(files, vec![format!("{ID_A}.json")]);

    store.delete(&urn).unwrap();
    assert!(matches!(store.load(ID_A), Err(ChatStoreError::NotFound(_))));
}

#[test]
fn a_conversation_over_the_cap_is_refused_and_the_file_it_would_replace_stands() {
    let dir = TempDir::new().unwrap();
    let store = ChatStore::new(dir.path());
    let held = conversation(ID_A, "2026-09-15T10:00:00+00:00");
    store.save(&held).unwrap();

    let mut oversized = conversation(ID_A, "2026-09-15T11:00:00+00:00");
    oversized.push_user(
        "x".repeat(MAX_CONVERSATION_BYTES + 1),
        Vec::new(),
        "2026-09-15T11:00:00+00:00".to_string(),
    );

    match store.save(&oversized) {
        Err(ChatStoreError::TooLarge { bytes, limit }) => {
            assert!(bytes > MAX_CONVERSATION_BYTES);
            assert_eq!(limit, MAX_CONVERSATION_BYTES);
        }
        other => panic!("a conversation over the cap was not refused: {other:?}"),
    }
    assert_eq!(store.load(ID_A).unwrap(), held);
}

#[test]
fn a_file_the_folder_holds_that_is_not_a_conversation_is_skipped() {
    let dir = TempDir::new().unwrap();
    let store = ChatStore::new(dir.path());
    store
        .save(&conversation(ID_A, "2026-09-15T10:00:00+00:00"))
        .unwrap();
    fs::write(chats_dir(dir.path()).join("notes.txt"), "not json").unwrap();
    fs::write(chats_dir(dir.path()).join("broken.json"), "{ not json").unwrap();
    fs::write(
        chats_dir(dir.path()).join(format!("{ID_B}.json")),
        "{ not json",
    )
    .unwrap();

    let listed = store.list().unwrap();

    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, ID_A);
}

/// The rows `chats/index.json` holds, by id.
fn indexed_ids(root: &Path) -> Vec<String> {
    let index: Value =
        serde_json::from_slice(&fs::read(chats_dir(root).join("index.json")).unwrap()).unwrap();
    index["rows"]
        .as_object()
        .expect("the index keeps its rows by id")
        .keys()
        .cloned()
        .collect()
}

#[test]
fn the_list_reads_no_conversation_bodies() {
    let dir = TempDir::new().unwrap();
    let store = ChatStore::new(dir.path());
    let mut held = conversation(ID_A, "2026-09-15T10:00:00+00:00");
    held.title = "Launch plan".to_string();
    store.save(&held).unwrap();
    store
        .save(&conversation(ID_B, "2026-09-14T10:00:00+00:00"))
        .unwrap();

    // Garbage of the same length with the same time: the file still matches
    // its row, so a list that opened it would skip it as unreadable.
    let file = chats_dir(dir.path()).join(format!("{ID_A}.json"));
    let modified = fs::metadata(&file).unwrap().modified().unwrap();
    let length = fs::metadata(&file).unwrap().len() as usize;
    fs::write(&file, vec![b'#'; length]).unwrap();
    fs::File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(modified)
        .unwrap();

    let listed = store.list().unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].id, ID_A);
    assert_eq!(listed[0].title, "Launch plan");
}

#[test]
fn a_folder_written_before_the_index_is_listed_whole_and_indexed() {
    let dir = TempDir::new().unwrap();
    let store = ChatStore::new(dir.path());
    store
        .save(&conversation(ID_A, "2026-09-15T10:00:00+00:00"))
        .unwrap();
    store
        .save(&conversation(ID_B, "2026-09-14T10:00:00+00:00"))
        .unwrap();
    fs::remove_file(chats_dir(dir.path()).join("index.json")).unwrap();

    let ids: Vec<String> = store.list().unwrap().into_iter().map(|c| c.id).collect();
    assert_eq!(ids, vec![ID_A.to_string(), ID_B.to_string()]);
    assert_eq!(
        indexed_ids(dir.path()),
        vec![ID_A.to_string(), ID_B.to_string()]
    );
}

#[test]
fn a_file_changed_behind_the_index_is_read_again() {
    let dir = TempDir::new().unwrap();
    let store = ChatStore::new(dir.path());
    store
        .save(&conversation(ID_A, "2026-09-15T10:00:00+00:00"))
        .unwrap();

    let mut changed = conversation(ID_A, "2026-09-16T10:00:00+00:00");
    changed.title = "Renamed elsewhere".to_string();
    fs::write(
        chats_dir(dir.path()).join(format!("{ID_A}.json")),
        serde_json::to_vec_pretty(&changed).unwrap(),
    )
    .unwrap();

    let listed = store.list().unwrap();
    assert_eq!(listed[0].title, "Renamed elsewhere");
    assert_eq!(listed[0].updated_at, "2026-09-16T10:00:00+00:00");
}

#[test]
fn a_file_removed_behind_the_index_leaves_the_list_and_the_index() {
    let dir = TempDir::new().unwrap();
    let store = ChatStore::new(dir.path());
    store
        .save(&conversation(ID_A, "2026-09-15T10:00:00+00:00"))
        .unwrap();
    store
        .save(&conversation(ID_B, "2026-09-14T10:00:00+00:00"))
        .unwrap();
    fs::remove_file(chats_dir(dir.path()).join(format!("{ID_B}.json"))).unwrap();

    let ids: Vec<String> = store.list().unwrap().into_iter().map(|c| c.id).collect();
    assert_eq!(ids, vec![ID_A.to_string()]);
    assert_eq!(indexed_ids(dir.path()), vec![ID_A.to_string()]);
}

#[test]
fn a_deleted_conversation_leaves_the_index() {
    let dir = TempDir::new().unwrap();
    let store = ChatStore::new(dir.path());
    store
        .save(&conversation(ID_A, "2026-09-15T10:00:00+00:00"))
        .unwrap();
    store
        .save(&conversation(ID_B, "2026-09-14T10:00:00+00:00"))
        .unwrap();

    store.delete(ID_B).unwrap();

    assert_eq!(indexed_ids(dir.path()), vec![ID_A.to_string()]);
}

#[test]
fn saves_from_many_threads_each_keep_their_row() {
    let dir = TempDir::new().unwrap();
    let ids: Vec<String> = (0..16)
        .map(|n| format!("0b7d6b7a-4444-4b6a-9d5e-{n:012}"))
        .collect();
    std::thread::scope(|scope| {
        for id in &ids {
            let root = dir.path().to_path_buf();
            scope.spawn(move || {
                // A store per caller, as each command builds its own.
                ChatStore::new(&root)
                    .save(&conversation(id, "2026-09-15T10:00:00+00:00"))
                    .unwrap();
            });
        }
    });

    let mut indexed = indexed_ids(dir.path());
    indexed.sort();
    assert_eq!(indexed, ids);
}

#[test]
fn the_file_holds_no_note_text_and_no_key() {
    let dir = TempDir::new().unwrap();
    let store = ChatStore::new(dir.path());
    let mut held = conversation(ID_A, "2026-09-15T10:00:00+00:00");
    held.push_user(
        "Tighten the opening.".to_string(),
        vec![AttachmentRef {
            path: "Ideas/Launch.md".to_string(),
            bytes: 42,
            hash: "abcd".to_string(),
        }],
        "2026-09-15T10:01:00+00:00".to_string(),
    );
    held.push_assistant(
        "Here is a shorter opening.".to_string(),
        AssistantReply {
            proposals: vec![StoredProposal {
                path: "Ideas/Launch.md".to_string(),
                summary: "Shorter opening".to_string(),
                before_hash: "abcd".to_string(),
                new_content: "Shorter.".to_string(),
                status: ProposalStatus::Applied,
            }],
            ..AssistantReply::default()
        },
        None,
        "2026-09-15T10:02:00+00:00".to_string(),
    );
    store.save(&held).unwrap();

    // Read back off disk: what the store wrote is the claim, not what it was
    // handed.
    let written: Value = serde_json::from_slice(
        &fs::read(chats_dir(dir.path()).join(format!("{ID_A}.json"))).unwrap(),
    )
    .unwrap();
    let mut found = Vec::new();
    keys(&written, &mut found);

    assert!(!found.iter().any(|key| key == "text"), "{found:?}");
    assert!(!found.iter().any(|key| key == "api_key"), "{found:?}");

    let attachment = written["turns"][0]["attachments"][0].as_object().unwrap();
    let mut attachment_keys: Vec<&String> = attachment.keys().collect();
    attachment_keys.sort();
    assert_eq!(attachment_keys, vec!["bytes", "hash", "path"]);
    assert_eq!(written["version"], 1);
}
