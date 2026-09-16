//! The conversation document: what a chat file holds and how a turn changes it.

use writ_core::chat::{
    AttachmentRef, Conversation, Proposal, ProposalStatus, RequestIdentity, Role, StoredProposal,
    StoredTurn, CONVERSATION_SCHEMA_VERSION, TITLE_MAX_CHARS,
};

/// A conversation with nothing in it.
fn fresh() -> Conversation {
    Conversation::new(
        "8f14e45f-ea8f-4b60-9a4c-3b1b1b1b1b1b".to_string(),
        "2026-09-15T10:00:00Z".to_string(),
        "anthropic".to_string(),
        "claude-sonnet-5".to_string(),
    )
}

/// One attachment reference, which never carries the note's text.
fn attachment(path: &str) -> AttachmentRef {
    AttachmentRef {
        path: path.to_string(),
        bytes: 1234,
        hash: "abc".to_string(),
    }
}

#[test]
fn a_new_conversation_is_pinned_to_the_schema_version() {
    let conversation = fresh();
    assert_eq!(conversation.version, CONVERSATION_SCHEMA_VERSION);
    assert_eq!(conversation.title, "New chat");
    assert_eq!(conversation.created_at, conversation.updated_at);
    assert!(conversation.turns.is_empty());
}

#[test]
fn a_title_is_the_first_line_that_holds_something() {
    assert_eq!(
        Conversation::title_from("Fold the intros"),
        "Fold the intros"
    );
    assert_eq!(
        Conversation::title_from("\n\n  Second line wins  \nthird\n"),
        "Second line wins"
    );
}

#[test]
fn a_title_with_nothing_in_it_stays_the_default() {
    assert_eq!(Conversation::title_from(""), "New chat");
    assert_eq!(Conversation::title_from("   \n\t\n"), "New chat");
}

#[test]
fn a_long_title_is_cut_on_a_character_boundary() {
    let text = format!("{}é{}", "a".repeat(TITLE_MAX_CHARS - 1), "b".repeat(20));
    let title = Conversation::title_from(&text);
    assert_eq!(title.chars().count(), TITLE_MAX_CHARS);
    assert!(title.ends_with('é'));
}

#[test]
fn the_first_user_turn_names_the_conversation_and_a_rename_survives() {
    let mut conversation = fresh();
    conversation.push_user(
        "Fold the intros\nand say why".to_string(),
        vec![attachment("Ideas/Launch.md")],
        "2026-09-15T10:01:00Z".to_string(),
    );
    assert_eq!(conversation.title, "Fold the intros");
    assert_eq!(conversation.updated_at, "2026-09-15T10:01:00Z");
    assert_eq!(conversation.turns.len(), 1);
    assert_eq!(conversation.turns[0].role, Role::User);
    assert_eq!(conversation.turns[0].attachments.len(), 1);

    conversation.title = "Launch note".to_string();
    conversation.push_user(
        "Something else".to_string(),
        Vec::new(),
        "2026-09-15T10:02:00Z".to_string(),
    );
    assert_eq!(conversation.title, "Launch note");
}

#[test]
fn an_assistant_turn_carries_its_proposals_and_no_attachments() {
    let mut conversation = fresh();
    conversation.push_assistant(
        "Here is what I would change.".to_string(),
        vec![StoredProposal {
            path: "Ideas/Launch.md".to_string(),
            summary: "Fold the intros".to_string(),
            before_hash: "abc".to_string(),
            new_content: "new text\n".to_string(),
            status: ProposalStatus::Pending,
        }],
        None,
        "2026-09-15T10:03:00Z".to_string(),
    );
    assert_eq!(conversation.turns[0].role, Role::Assistant);
    assert!(conversation.turns[0].attachments.is_empty());
    assert_eq!(conversation.turns[0].proposals.len(), 1);
    assert_eq!(conversation.updated_at, "2026-09-15T10:03:00Z");
}

#[test]
fn truncating_drops_the_turns_from_that_point_on() {
    let mut conversation = fresh();
    for n in 0..4 {
        conversation.push_user(format!("turn {n}"), Vec::new(), "t".to_string());
    }
    conversation.truncate(2, "2026-09-15T10:04:00Z".to_string());
    assert_eq!(conversation.turns.len(), 2);
    assert_eq!(conversation.turns[1].content, "turn 1");
    assert_eq!(conversation.updated_at, "2026-09-15T10:04:00Z");
    conversation.truncate(9, "2026-09-15T10:05:00Z".to_string());
    assert_eq!(conversation.turns.len(), 2);
}

#[test]
fn the_request_carries_roles_and_text_and_nothing_else() {
    let mut conversation = fresh();
    conversation.push_user(
        "question".to_string(),
        vec![attachment("A.md")],
        "t".to_string(),
    );
    conversation.push_assistant(
        "answer".to_string(),
        vec![StoredProposal {
            path: "A.md".to_string(),
            summary: String::new(),
            before_hash: "abc".to_string(),
            new_content: "new\n".to_string(),
            status: ProposalStatus::Pending,
        }],
        None,
        "t".to_string(),
    );
    let turns = conversation.request_turns();
    assert_eq!(turns.len(), 2);
    assert_eq!(turns[0].role, Role::User);
    assert_eq!(turns[0].content, "question");
    assert_eq!(turns[1].role, Role::Assistant);
    assert_eq!(turns[1].content, "answer");
}

#[test]
fn a_proposal_status_is_set_by_turn_and_path() {
    let mut conversation = fresh();
    conversation.push_user("q".to_string(), Vec::new(), "t".to_string());
    conversation.push_assistant(
        "a".to_string(),
        vec![StoredProposal {
            path: "A.md".to_string(),
            summary: String::new(),
            before_hash: "abc".to_string(),
            new_content: "new\n".to_string(),
            status: ProposalStatus::Pending,
        }],
        None,
        "t".to_string(),
    );
    assert!(conversation.set_proposal_status(
        1,
        "A.md",
        ProposalStatus::Applied,
        "2026-09-15T11:00:00Z".to_string()
    ));
    assert_eq!(
        conversation.turns[1].proposals[0].status,
        ProposalStatus::Applied
    );
    assert_eq!(conversation.updated_at, "2026-09-15T11:00:00Z");
    assert!(!conversation.set_proposal_status(
        9,
        "A.md",
        ProposalStatus::Discarded,
        "later".to_string()
    ));
    assert!(!conversation.set_proposal_status(
        1,
        "B.md",
        ProposalStatus::Discarded,
        "later".to_string()
    ));
    assert!(!conversation.set_proposal_status(
        0,
        "A.md",
        ProposalStatus::Discarded,
        "later".to_string()
    ));
    assert_eq!(conversation.updated_at, "2026-09-15T11:00:00Z");
}

#[test]
fn a_parsed_proposal_is_stored_as_pending_without_its_hunks() {
    let proposal = Proposal {
        path: "A.md".to_string(),
        before_hash: "abc".to_string(),
        new_content: "new\n".to_string(),
        summary: "tidy".to_string(),
        hunks: writ_core::diff::line_diff("old\n", "new\n").expect("under the limit"),
    };
    assert!(!proposal.hunks.is_empty());
    let stored = StoredProposal::from(&proposal);
    assert_eq!(stored.path, "A.md");
    assert_eq!(stored.summary, "tidy");
    assert_eq!(stored.before_hash, "abc");
    assert_eq!(stored.new_content, "new\n");
    assert_eq!(stored.status, ProposalStatus::Pending);
    let json = serde_json::to_string(&stored).expect("a stored proposal serialises");
    assert!(!json.contains("hunks"), "{json}");
}

/// The document shape the decision record writes out.
const RECORDED: &str = r#"{
  "version": 1,
  "id": "8f14e45f-ea8f-4b60-9a4c-3b1b1b1b1b1b",
  "title": "Fold the intros",
  "created_at": "2026-09-15T10:00:00Z",
  "updated_at": "2026-09-15T10:03:00Z",
  "provider": "anthropic",
  "model": "claude-sonnet-5",
  "turns": [
    { "role": "user", "content": "Fold the intros",
      "attachments": [{ "path": "Ideas/Launch.md", "bytes": 1234, "hash": "abc" }] },
    { "role": "assistant", "content": "Here is what I would change.",
      "proposals": [{ "path": "Ideas/Launch.md", "summary": "Fold the intros",
                      "before_hash": "abc", "new_content": "new text\n",
                      "status": "applied" }] }
  ]
}"#;

#[test]
fn the_recorded_document_round_trips() {
    let conversation: Conversation = serde_json::from_str(RECORDED).expect("the record parses");
    assert_eq!(conversation.version, 1);
    assert_eq!(conversation.title, "Fold the intros");
    assert_eq!(conversation.turns.len(), 2);
    assert_eq!(conversation.turns[0].attachments[0].bytes, 1234);
    assert!(conversation.turns[0].proposals.is_empty());
    assert!(conversation.turns[1].attachments.is_empty());
    assert_eq!(
        conversation.turns[1].proposals[0].status,
        ProposalStatus::Applied
    );

    let json = serde_json::to_string(&conversation).expect("it serialises");
    let back: Conversation = serde_json::from_str(&json).expect("it parses again");
    assert_eq!(back, conversation);
    assert!(json.contains("\"status\":\"applied\""), "{json}");
}

#[test]
fn a_turn_holds_no_note_text() {
    let turn = StoredTurn {
        role: Role::User,
        content: "q".to_string(),
        attachments: vec![attachment("A.md")],
        proposals: Vec::new(),
        identity: None,
    };
    let value: serde_json::Value = serde_json::to_value(&turn).expect("a turn serialises");
    let mut keys: Vec<&str> = value["attachments"][0]
        .as_object()
        .expect("an attachment is an object")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(keys, vec!["bytes", "hash", "path"]);
}

#[test]
fn a_turn_written_before_the_identity_field_still_opens() {
    // Every conversation on disk was written without it, and a file that
    // stops opening is a conversation the person has lost.
    let old = r#"{
      "version": 1,
      "id": "0b7d6b7a-1111-4b6a-9d5e-000000000001",
      "title": "A chat",
      "created_at": "2026-09-15T10:00:00Z",
      "updated_at": "2026-09-15T10:01:00Z",
      "provider": "ollama",
      "model": "qwen3:4b",
      "turns": [
        { "role": "user", "content": "q" },
        { "role": "assistant", "content": "a" }
      ]
    }"#;
    let conversation: Conversation = serde_json::from_str(old).expect("an older file opens");
    assert_eq!(conversation.turns.len(), 2);
    assert!(conversation.turns[1].identity.is_none());

    // And a turn that carries one round-trips.
    let mut fresh = conversation.clone();
    fresh.push_assistant(
        "a".to_string(),
        Vec::new(),
        Some(RequestIdentity {
            provider: "deepseek".to_string(),
            model: "deepseek-chat".to_string(),
            host: "api.deepseek.com".to_string(),
        }),
        "2026-09-15T10:02:00Z".to_string(),
    );
    let written = serde_json::to_string(&fresh).expect("write");
    let back: Conversation = serde_json::from_str(&written).expect("read");
    assert_eq!(
        back.turns[2].identity.as_ref().unwrap().model,
        "deepseek-chat"
    );
}
