//! Request shape and stream grammar for the chat pane.
//!
//! Sibling of [`crate::polish`] and split the same way: the policy is here and
//! the mechanism — the client, the socket, the keychain — is in the Tauri
//! adapter. Reachability is not decided twice: [`crate::polish::resolve_endpoint`]
//! and [`crate::polish::is_endpoint_allowed`] answer for a chat request exactly
//! as they answer for a rewrite, so a hand-edited `config.toml` is refused by
//! the same guard in both places.
//!
//! Two wire formats reach the same conversation. [`Provider::Anthropic`] speaks
//! the Messages API, where the system prompt is a field beside `messages`
//! rather than a message inside it and a stream carries `content_block_delta`
//! frames. [`Provider::OpenAiCompatible`] speaks `chat/completions`, which is
//! what a local model behind Ollama and most hosted providers answer. Reading
//! one line of either is a pure function over that line ([`parse_delta`]), so
//! both grammars are tested against recorded frames and neither needs a
//! network.
//!
//! Nothing here writes. A reply that asks for a change to a note produces a
//! [`Proposal`], which the user reads beside the current text and applies by
//! hand (ADR-031 rule 4.3). A proposal naming a note that was not attached is
//! dropped: the hash a refusal is judged against is what Writ read when the
//! request was built, and a note nobody attached has none.
//!
//! Split by what each part answers: [`wire`] is the request body and the
//! stream grammar, [`prompt`] what the model is told, [`proposals`] what a
//! finished reply asks for, [`filter`] what the pane may show while a reply
//! arrives, and [`conversation`] the document a conversation is saved as.

pub mod conversation;
pub mod filter;
pub mod prompt;
pub mod proposals;
pub mod wire;

pub use conversation::*;
pub use filter::*;
pub use prompt::*;
pub use proposals::*;
pub use wire::*;

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;
    use crate::ai::providers::BodyRules;

    /// Recorded frames, not a live call: the streams and bodies below are
    /// files in the repository, so both grammars and both request shapes are
    /// asserted without a network.
    const ANTHROPIC_STREAM: &str = include_str!("../../tests/fixtures/chat/anthropic-stream.sse");
    const OPENAI_STREAM: &str = include_str!("../../tests/fixtures/chat/openai-stream.sse");
    const OPENAI_ERROR_STREAM: &str = include_str!("../../tests/fixtures/chat/openai-error.sse");
    const ANTHROPIC_REQUEST: &str =
        include_str!("../../tests/fixtures/chat/anthropic-request.json");
    const OPENAI_REQUEST: &str = include_str!("../../tests/fixtures/chat/openai-request.json");

    /// The reply text both recorded streams spell out.
    const RECORDED_REPLY: &str = "The note argues one thing.";

    fn fixture_turns() -> Vec<ChatTurn> {
        vec![
            ChatTurn {
                role: Role::User,
                content: "What does this note argue?".to_string(),
            },
            ChatTurn {
                role: Role::Assistant,
                content: "That March is late.".to_string(),
            },
            ChatTurn {
                role: Role::User,
                content: "Propose a fix.".to_string(),
            },
        ]
    }

    /// Every text delta of a recorded stream, joined, and where it stopped.
    fn replay(provider: Provider, stream: &str) -> (String, Vec<Delta>) {
        let mut text = String::new();
        let mut terminals = Vec::new();
        for line in stream.lines() {
            match parse_delta(provider, line.trim()) {
                Delta::Text(chunk) => text.push_str(&chunk),
                Delta::Ignore => {}
                other => terminals.push(other),
            }
        }
        (text, terminals)
    }

    #[test]
    fn the_recorded_anthropic_body_is_the_one_built() {
        let expected: Value = serde_json::from_str(ANTHROPIC_REQUEST).expect("fixture");
        let built = build_request_body(
            Provider::Anthropic,
            BodyRules::default(),
            "claude-opus-5",
            "SYSTEM",
            &fixture_turns(),
            &[note("Ideas/Launch.md", "The launch is in March.")],
        );
        assert_eq!(built, expected);
    }

    #[test]
    fn the_recorded_openai_body_is_the_one_built() {
        let expected: Value = serde_json::from_str(OPENAI_REQUEST).expect("fixture");
        let built = build_request_body(
            Provider::OpenAiCompatible,
            BodyRules::default(),
            "llama3",
            "SYSTEM",
            &fixture_turns(),
            &[note("Ideas/Launch.md", "The launch is in March.")],
        );
        assert_eq!(built, expected);
    }

    #[test]
    fn openai_s_host_reads_the_ceiling_as_max_completion_tokens() {
        let built = build_request_body(
            Provider::OpenAiCompatible,
            crate::ai::providers::body_rules("api.openai.com"),
            "gpt-5-mini",
            "SYSTEM",
            &fixture_turns(),
            &[],
        );
        assert_eq!(built["max_completion_tokens"], MAX_REPLY_TOKENS);
        assert!(built.get("max_tokens").is_none(), "{built}");
        assert!(built.get("temperature").is_none(), "{built}");

        // Everything else in the body is what any other host is sent.
        let mut elsewhere = build_request_body(
            Provider::OpenAiCompatible,
            BodyRules::default(),
            "gpt-5-mini",
            "SYSTEM",
            &fixture_turns(),
            &[],
        );
        let ceiling = elsewhere
            .as_object_mut()
            .and_then(|body| body.remove("max_tokens"))
            .expect("a ceiling");
        let mut built = built;
        let renamed = built
            .as_object_mut()
            .and_then(|body| body.remove("max_completion_tokens"))
            .expect("a ceiling");
        assert_eq!(ceiling, renamed);
        assert_eq!(built, elsewhere);
    }

    #[test]
    fn the_recorded_anthropic_stream_reads_as_its_reply() {
        let (text, terminals) = replay(Provider::Anthropic, ANTHROPIC_STREAM);
        assert_eq!(text, RECORDED_REPLY);
        assert_eq!(terminals, vec![Delta::Done]);
    }

    #[test]
    fn the_recorded_openai_stream_reads_as_the_same_reply() {
        let (text, terminals) = replay(Provider::OpenAiCompatible, OPENAI_STREAM);
        assert_eq!(text, RECORDED_REPLY);
        assert_eq!(terminals, vec![Delta::Done]);
    }

    fn note(path: &str, text: &str) -> AttachedNote {
        AttachedNote {
            path: path.to_string(),
            prompt_path: path.to_string(),
            text: text.to_string(),
            before_hash: crate::hash::sha256_hex(text.as_bytes()),
        }
    }

    fn turns() -> Vec<ChatTurn> {
        vec![ChatTurn {
            role: Role::User,
            content: "What does this note argue?".to_string(),
        }]
    }

    #[test]
    fn a_provider_id_round_trips() {
        assert_eq!(Provider::parse("anthropic").unwrap(), Provider::Anthropic);
        assert_eq!(
            Provider::parse(" openai_compatible ").unwrap(),
            Provider::OpenAiCompatible
        );
        assert_eq!(
            Provider::parse("mystery"),
            Err(ChatError::UnknownProvider("mystery".to_string()))
        );
        assert_eq!(Provider::Anthropic.as_str(), "anthropic");
    }

    #[test]
    fn every_table_row_reaches_a_wire_this_module_speaks() {
        use crate::ai::providers::PROVIDERS;

        for row in PROVIDERS {
            let provider = Provider::from_wire(row.wire);
            match row.id {
                "anthropic" => assert_eq!(provider, Provider::Anthropic),
                _ => assert_eq!(provider, Provider::OpenAiCompatible, "row {}", row.id),
            }
        }
    }

    #[test]
    fn both_spellings_of_the_anthropic_base_reach_one_endpoint() {
        assert_eq!(
            endpoint(Provider::Anthropic, "https://api.anthropic.com"),
            "https://api.anthropic.com/v1/messages"
        );
        assert_eq!(
            endpoint(Provider::Anthropic, "https://api.anthropic.com/v1/"),
            "https://api.anthropic.com/v1/messages"
        );
        assert_eq!(
            endpoint(Provider::OpenAiCompatible, "http://localhost:11434/v1"),
            "http://localhost:11434/v1/chat/completions"
        );
    }

    #[test]
    fn the_anthropic_body_keeps_the_system_prompt_outside_messages() {
        let body = build_request_body(
            Provider::Anthropic,
            BodyRules::default(),
            " claude-opus-5 ",
            &system_prompt(&[note("Ideas/Launch.md", "the note text")]),
            &turns(),
            &[note("Ideas/Launch.md", "the note text")],
        );
        assert_eq!(body["model"], "claude-opus-5");
        assert_eq!(body["stream"], true);
        assert_eq!(body["max_tokens"], MAX_REPLY_TOKENS);
        assert_eq!(
            body["system"],
            system_prompt(&[note("Ideas/Launch.md", "the note text")])
        );
        let messages = body["messages"].as_array().expect("messages");
        assert_eq!(messages.len(), 2);
        assert!(messages
            .iter()
            .all(|m| m["role"].as_str() != Some("system")));
        assert_eq!(messages[0]["role"], "user");
        assert!(messages[0]["content"]
            .as_str()
            .expect("text")
            .contains("Ideas/Launch.md"));
        assert_eq!(messages[1]["content"], "What does this note argue?");
    }

    #[test]
    fn an_outside_file_reaches_the_model_under_its_name_alone() {
        let mut attached = note("/Users/someone/work/client-repo/README.md", "the readme");
        attached.prompt_path = "README.md".to_string();
        let context = [attached];
        let body = build_request_body(
            Provider::Anthropic,
            BodyRules::default(),
            "claude-opus-5",
            &system_prompt(&context),
            &turns(),
            &context,
        );
        let sent = serde_json::to_string(&body).expect("body");
        assert!(
            sent.contains("<note path=\\\"README.md\\\">"),
            "the model is told the file name it can name back: {sent}"
        );
        assert!(
            !sent.contains("/Users/someone/work/client-repo"),
            "where the file sits on this machine is not the model's to read: {sent}"
        );
    }

    #[test]
    fn an_openai_body_caps_its_tokens() {
        let body = build_request_body(
            Provider::OpenAiCompatible,
            BodyRules::default(),
            "llama3",
            &system_prompt(&[]),
            &turns(),
            &[],
        );
        assert_eq!(
            body["max_tokens"], MAX_REPLY_TOKENS,
            "an uncapped body takes the server's default, which cuts a whole-note reply in half"
        );
    }

    #[test]
    fn a_length_finish_reason_ends_the_reply_as_truncated() {
        let frame = "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"length\"}]}";
        assert_eq!(
            parse_delta(Provider::OpenAiCompatible, frame),
            Delta::Truncated
        );
        let stopped = "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}";
        assert_eq!(
            parse_delta(Provider::OpenAiCompatible, stopped),
            Delta::Ignore
        );
    }

    #[test]
    fn an_anthropic_max_tokens_stop_ends_the_reply_as_truncated() {
        let frame = "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"max_tokens\"}}";
        assert_eq!(parse_delta(Provider::Anthropic, frame), Delta::Truncated);
        let ended = "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"}}";
        assert_eq!(parse_delta(Provider::Anthropic, ended), Delta::Ignore);
    }

    #[test]
    fn the_openai_body_keeps_the_shape_a_rewrite_already_sends() {
        let body = build_request_body(
            Provider::OpenAiCompatible,
            BodyRules::default(),
            "llama3",
            &system_prompt(&[note("Ideas/Launch.md", "the note text")]),
            &turns(),
            &[note("Ideas/Launch.md", "the note text")],
        );
        assert_eq!(body["model"], "llama3");
        assert_eq!(body["stream"], true);
        assert!(body.get("system").is_none());
        let messages = body["messages"].as_array().expect("messages");
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(
            messages[0]["content"],
            system_prompt(&[note("Ideas/Launch.md", "the note text")])
        );
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(messages[2]["content"], "What does this note argue?");
    }

    #[test]
    fn nothing_attached_sends_no_context_message() {
        let body = build_request_body(
            Provider::Anthropic,
            BodyRules::default(),
            "claude-opus-5",
            &system_prompt(&[]),
            &turns(),
            &[],
        );
        let messages = body["messages"].as_array().expect("messages");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["content"], "What does this note argue?");
    }

    #[test]
    fn the_body_holds_the_attached_notes_and_no_others() {
        let body = build_request_body(
            Provider::Anthropic,
            BodyRules::default(),
            "claude-opus-5",
            &system_prompt(&[note("A.md", "the first note")]),
            &turns(),
            &[note("A.md", "the first note")],
        );
        let sent = serde_json::to_string(&body).expect("serialise");
        assert!(sent.contains("the first note"));
        assert!(!sent.contains("the second note"));
    }

    #[test]
    fn a_malformed_line_yields_no_text() {
        for line in [
            "",
            ":ping",
            "event: content_block_delta",
            "data:",
            "data: {not json",
            "data: {\"type\":\"content_block_delta\"}",
        ] {
            assert_eq!(parse_delta(Provider::Anthropic, line), Delta::Ignore);
            assert_eq!(parse_delta(Provider::OpenAiCompatible, line), Delta::Ignore);
        }
    }

    #[test]
    fn a_thinking_delta_is_not_reply_text() {
        assert_eq!(
            parse_delta(
                Provider::Anthropic,
                "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"hm\"}}"
            ),
            Delta::Ignore
        );
    }

    #[test]
    fn a_recorded_openai_error_frame_ends_the_stream_rather_than_completing_it() {
        // Recorded from a server that ran out of memory mid-reply. The frame
        // is read for the fact of the failure; its wording stays in the frame.
        let deltas: Vec<Delta> = OPENAI_ERROR_STREAM
            .lines()
            .map(|line| parse_delta(Provider::OpenAiCompatible, line))
            .filter(|delta| !matches!(delta, Delta::Ignore))
            .collect();
        // Two frames, because servers that copy the spelling do not all copy
        // the shape: the object the OpenAI schema describes, and the bare
        // string several local servers send instead. Both end the stream.
        assert_eq!(deltas, vec![Delta::Failed, Delta::Failed]);
        // `null` is what a server writes when nothing went wrong.
        assert_eq!(
            parse_delta(Provider::OpenAiCompatible, "data: {\"error\":null}"),
            Delta::Ignore
        );
        // And a chunk that carries the field beside real content still
        // delivers the content.
        assert_eq!(
            parse_delta(
                Provider::OpenAiCompatible,
                "data: {\"id\":\"c1\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hello\"}}],\"error\":null}"
            ),
            Delta::Text("hello".to_string())
        );
        // A frame reporting a real failure ends the stream even when it
        // carries an empty delta beside it.
        assert_eq!(
            parse_delta(
                Provider::OpenAiCompatible,
                "data: {\"choices\":[{\"index\":0,\"delta\":{}}],\"error\":{\"message\":\"out of memory\"}}"
            ),
            Delta::Failed
        );
        assert_eq!(
            parse_delta(
                Provider::OpenAiCompatible,
                "data: {\"choices\":[],\"error\":\"out of memory\"}"
            ),
            Delta::Failed
        );
    }

    #[test]
    fn a_mid_stream_error_carries_nothing_the_server_wrote() {
        // Both fields are the host's own text. Neither comes back, so neither
        // can reach a log line or the pane by way of this parser.
        assert_eq!(
            parse_delta(
                Provider::Anthropic,
                "data: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}"
            ),
            Delta::Failed
        );
    }

    #[test]
    fn a_proposal_names_an_attached_note() {
        let context = vec![note("Ideas/Launch.md", "old text\n")];
        let reply = "Here is one change.\n\n\
    ```writ-proposal path=\"Ideas/Launch.md\" summary=\"Fold the intros\"\n\
    the new text\n\
    ```\n";
        let proposals = parse_proposals(reply, &context, false).proposals;
        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0].path, "Ideas/Launch.md");
        assert_eq!(proposals[0].new_content, "the new text\n");
        assert_eq!(proposals[0].summary, "Fold the intros");
        assert_eq!(proposals[0].before_hash, context[0].before_hash);
    }

    #[test]
    fn a_proposal_carries_the_diff_against_the_note_it_was_read_from() {
        let context = vec![note("Ideas/Launch.md", "one\ntwo\nthree\n")];
        let reply = "```writ-proposal path=\"Ideas/Launch.md\"\n\
    one\n\
    two changed\n\
    three\n\
    ```\n";
        let proposals = parse_proposals(reply, &context, false).proposals;
        let lines = &proposals[0].hunks[0].lines;
        assert_eq!(
            lines
                .iter()
                .filter(|line| line.kind == crate::diff::LineKind::Removed)
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            vec!["two"]
        );
        assert_eq!(
            lines
                .iter()
                .filter(|line| line.kind == crate::diff::LineKind::Added)
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            vec!["two changed"]
        );
    }

    #[test]
    fn a_note_too_large_to_compare_leaves_the_hunks_empty() {
        let huge = "x\n".repeat(crate::diff::MAX_DIFF_BYTES);
        let context = vec![note("Big.md", &huge)];
        let reply = "```writ-proposal path=\"Big.md\" summary=\"trim it\"\nsmall\n```\n";
        let proposals = parse_proposals(reply, &context, false).proposals;
        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0].summary, "trim it");
        assert!(proposals[0].hunks.is_empty());
    }

    #[test]
    fn a_proposal_for_a_note_nobody_attached_is_dropped() {
        let context = vec![note("Ideas/Launch.md", "old text")];
        let reply = "```writ-proposal path=\"../../.ssh/config\" summary=\"nothing good\"\n\
    owned\n\
    ```\n";
        assert!(parse_proposals(reply, &context, false).proposals.is_empty());
        let unattached = "```writ-proposal path=\"Ideas/Other.md\"\ntext\n```\n";
        assert!(parse_proposals(unattached, &context, false)
            .proposals
            .is_empty());
    }

    #[test]
    fn a_block_the_reply_never_closed_ends_where_the_reply_does() {
        let context = vec![note("Ideas/Launch.md", "old text\n")];
        let reply = "```writ-proposal path=\"Ideas/Launch.md\"\nthe whole note";
        let proposals = parse_proposals(reply, &context, false).proposals;
        assert_eq!(proposals.len(), 1);
        assert_eq!(proposals[0].new_content, "the whole note\n");
    }

    #[test]
    fn a_reply_can_carry_more_than_one_proposal() {
        let context = vec![note("A.md", "a\n"), note("B.md", "b\n")];
        let reply = "```writ-proposal path=\"A.md\"\nnew a\n```\n\
    between\n\
    ```writ-proposal path=\"B.md\" summary=\"tidy\"\nnew b\n```\n";
        let proposals = parse_proposals(reply, &context, false).proposals;
        assert_eq!(proposals.len(), 2);
        assert_eq!(proposals[0].new_content, "new a\n");
        assert_eq!(proposals[1].summary, "tidy");
    }

    #[test]
    fn a_reply_with_no_fence_proposes_nothing() {
        let context = vec![note("A.md", "a")];
        assert!(parse_proposals("It argues two things.", &context, false)
            .proposals
            .is_empty());
    }
}
