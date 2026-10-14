//! The ADR-040 section 1 migration rows a released file can reach (1, 2 and
//! 8), asserted from outside the crate through a whole `config.toml`. Rows 3
//! to 7 read a chat endpoint only unreleased builds wrote and were deleted.
//!
//! The in-module tests parse the `[ai]` table alone. These feed a full file,
//! other sections included, so the rows are pinned at the seam a real upgrade
//! crosses, and the serialised result is checked for every field the old shape
//! carried.

use writ_core::config::WritConfig;

fn parse(file: &str) -> WritConfig {
    toml::from_str(file).expect("a config of any age parses")
}

/// A file with the sections a real one carries, plus the `[ai]` body given.
fn whole_file(ai: &str) -> String {
    format!(
        "[hotkey]\ntoggle = \"CmdOrCtrl+Shift+Space\"\n\n\
         [editor]\nfont_size = 18\nword_wrap = true\n\n\
         [window]\nwidth = 1400\nheight = 900\n\n\
         [storage]\npath = \"~/.writ\"\n\n\
         [ai]\n{ai}"
    )
}

#[test]
fn row_1_a_pre_chat_file_keeps_its_preset_model_and_switch() {
    let c = parse(&whole_file(
        "enabled = true\npreset = \"groq\"\nbase_url = \"https://api.groq.com/openai/v1\"\nmodel = \"llama-3.3-70b-versatile\"\nconsented_hosts = [\"api.groq.com\"]\n",
    ));
    assert_eq!(c.ai.provider, "groq");
    assert!(c.ai.rewrite.enabled);
    assert_eq!(c.ai.model, "llama-3.3-70b-versatile");
    assert!(
        c.ai.base_url.is_empty(),
        "a preset takes its base url from the table"
    );
    assert_eq!(c.ai.effective_base_url(), "https://api.groq.com/openai/v1");
    assert_eq!(c.ai.consented_hosts, vec!["api.groq.com".to_string()]);
    // The rest of the file is unharmed by the migration.
    assert_eq!(c.editor.font_size, 18);
    assert_eq!(c.window.width, 1400);

    // The same row with `custom` keeps the typed URL.
    let custom = parse(&whole_file(
        "enabled = true\npreset = \"custom\"\nbase_url = \"https://llm.example/v1\"\nmodel = \"house\"\n",
    ));
    assert_eq!(custom.ai.provider, "custom");
    assert_eq!(custom.ai.base_url, "https://llm.example/v1");
}

#[test]
fn row_2_rewrite_on_with_the_chat_off_or_absent() {
    let absent = parse(&whole_file(
        "enabled = true\npreset = \"openai\"\nmodel = \"gpt-4o-mini\"\n",
    ));
    let off = parse(&whole_file(
        "enabled = true\npreset = \"openai\"\nmodel = \"gpt-4o-mini\"\n\n[ai.chat]\nenabled = false\nprovider = \"anthropic\"\nmodel = \"claude-opus-5\"\n",
    ));
    for c in [&absent, &off] {
        assert_eq!(c.ai.provider, "openai");
        assert!(c.ai.rewrite.enabled);
        assert!(!c.ai.chat.enabled);
        assert!(
            c.ai.chat.model.is_empty(),
            "a chat that is off carries no model over"
        );
    }
}

#[test]
fn row_8_an_empty_table_is_the_default_connection() {
    let empty = parse(&whole_file(""));
    let missing = parse("[editor]\nfont_size = 18\n");
    for c in [&empty, &missing] {
        assert_eq!(c.ai.provider, "ollama");
        assert!(!c.ai.rewrite.enabled);
        assert!(!c.ai.chat.enabled);
        assert!(c.ai.model.is_empty());
        assert!(c.ai.base_url.is_empty());
        assert!(c.ai.consented_hosts.is_empty());
        assert_eq!(c.ai.effective_base_url(), "http://localhost:11434/v1");
    }
}

#[test]
fn a_migrated_file_is_written_back_without_one_old_field() {
    let c = parse(&whole_file(
        "enabled = true\npreset = \"custom\"\nbase_url = \"https://llm.example/v1\"\nmodel = \"house\"\nconsented_hosts = [\"llm.example\"]\n\n[ai.chat]\nenabled = true\nprovider = \"openai_compatible\"\nbase_url = \"https://other.example/v1\"\nmodel = \"other\"\n",
    ));
    let written = toml::to_string(&c).expect("a config serialises");
    let ai = written
        .split("[ai]")
        .nth(1)
        .expect("the file carries an [ai] table");
    // The whole `[ai]` tree, which runs to the end of the file or the next
    // top-level table.
    let ai_tree: String = ai
        .lines()
        .take_while(|line| {
            let line = line.trim();
            !(line.starts_with('[') && !line.starts_with("[ai."))
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(!ai_tree.contains("preset"), "{ai_tree}");
    // `[ai]` itself, before its sub-tables: the old master switch lived here.
    let ai_body = ai_tree.split("[ai.").next().unwrap_or_default();
    assert!(
        !ai_body.contains("enabled"),
        "the old master switch is gone from [ai] itself: {ai_body}"
    );
    let chat = ai_tree
        .split("[ai.chat]")
        .nth(1)
        .expect("the chat table is written");
    // The chat's second endpoint is gone; `model_provider`, which qualifies
    // its own model, is written.
    assert!(
        !chat.lines().any(|line| line.trim().starts_with("provider")),
        "{ai_tree}"
    );
    assert!(!chat.contains("base_url"), "{ai_tree}");
    assert!(chat.contains("model_provider ="), "{ai_tree}");

    // And it reads back as itself.
    let back: WritConfig = toml::from_str(&written).expect("the new shape re-reads");
    assert_eq!(back.ai, c.ai);
    assert_eq!(back.ai.provider, "custom");
    assert_eq!(back.ai.base_url, "https://llm.example/v1");
    assert_eq!(back.ai.consented_hosts, vec!["llm.example".to_string()]);
}
