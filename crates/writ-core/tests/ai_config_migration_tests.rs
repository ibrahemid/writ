//! The eight ADR-040 section 1 migration rows, asserted from outside the crate
//! through a whole `config.toml`.
//!
//! The in-module tests parse the `[ai]` table alone. These feed a full file,
//! other sections included, so the rows are pinned at the seam a real upgrade
//! crosses, and the serialised result is checked for every field the old shape
//! carried.

use writ_core::ai::providers::PROVIDERS;
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
fn row_3_the_chat_alone_on_anthropic() {
    let c = parse(&whole_file(
        "enabled = false\nconsented_hosts = [\"api.anthropic.com\"]\n\n[ai.chat]\nenabled = true\nprovider = \"anthropic\"\nmodel = \"claude-opus-5\"\n",
    ));
    assert_eq!(c.ai.provider, "anthropic");
    assert_eq!(c.ai.model, "claude-opus-5");
    assert!(c.ai.chat.enabled);
    assert!(!c.ai.rewrite.enabled);
    assert!(c.ai.chat.model.is_empty());
    assert_eq!(c.ai.chat_model(), "claude-opus-5");
    assert!(
        c.ai.chat.model_provider.is_empty(),
        "a chat that names no model of its own carries no qualifier"
    );
    assert_eq!(c.ai.consented_hosts, vec!["api.anthropic.com".to_string()]);
}

#[test]
fn row_4_matches_on_every_base_url_the_table_carries() {
    // One case per row that has a base URL, not only the two the unit tests pin.
    for row in PROVIDERS.iter().filter(|p| !p.base_url.is_empty()) {
        for typed in [row.base_url.to_string(), format!("{}/", row.base_url)] {
            let c = parse(&whole_file(&format!(
                "enabled = false\n\n[ai.chat]\nenabled = true\nprovider = \"openai_compatible\"\nbase_url = \"{typed}\"\nmodel = \"m-1\"\n"
            )));
            assert_eq!(
                c.ai.provider, row.id,
                "{typed} should be recognised as {}",
                row.id
            );
            assert!(
                c.ai.base_url.is_empty(),
                "{} keeps no typed url of its own",
                row.id
            );
            assert_eq!(c.ai.model, "m-1");
            assert_eq!(c.ai.effective_base_url(), row.base_url);
        }
    }

    // The DeepSeek preset shipped with a `/v1` the table dropped.
    let legacy = parse(&whole_file(
        "enabled = false\n\n[ai.chat]\nenabled = true\nprovider = \"openai_compatible\"\nbase_url = \"https://api.deepseek.com/v1\"\nmodel = \"deepseek-chat\"\n",
    ));
    assert_eq!(legacy.ai.provider, "deepseek");
    assert!(legacy.ai.base_url.is_empty());
    assert_eq!(legacy.ai.effective_base_url(), "https://api.deepseek.com");
}

#[test]
fn row_5_an_unknown_base_url_becomes_custom() {
    let c = parse(&whole_file(
        "enabled = false\n\n[ai.chat]\nenabled = true\nprovider = \"openai_compatible\"\nbase_url = \"https://llm.example/v1\"\nmodel = \"house-model\"\n",
    ));
    assert_eq!(c.ai.provider, "custom");
    assert_eq!(c.ai.base_url, "https://llm.example/v1");
    assert_eq!(c.ai.model, "house-model");
    assert_eq!(c.ai.effective_base_url(), "https://llm.example/v1");
}

#[test]
fn row_6_both_on_against_the_same_provider() {
    let same = parse(&whole_file(
        "enabled = true\npreset = \"groq\"\nbase_url = \"https://api.groq.com/openai/v1\"\nmodel = \"llama-3.3-70b-versatile\"\n\n[ai.chat]\nenabled = true\nprovider = \"openai_compatible\"\nbase_url = \"https://api.groq.com/openai/v1\"\nmodel = \"llama-3.3-70b-versatile\"\n",
    ));
    assert_eq!(same.ai.provider, "groq");
    assert!(same.ai.rewrite.enabled && same.ai.chat.enabled);
    assert!(
        same.ai.chat.model.is_empty(),
        "one model is not written down twice"
    );
    assert_eq!(same.ai.chat_model(), "llama-3.3-70b-versatile");

    let differs = parse(&whole_file(
        "enabled = true\npreset = \"groq\"\nbase_url = \"https://api.groq.com/openai/v1\"\nmodel = \"llama-3.1-8b-instant\"\n\n[ai.chat]\nenabled = true\nprovider = \"openai_compatible\"\nbase_url = \"https://api.groq.com/openai/v1\"\nmodel = \"llama-3.3-70b-versatile\"\n",
    ));
    assert_eq!(differs.ai.model, "llama-3.1-8b-instant");
    assert_eq!(differs.ai.chat.model, "llama-3.3-70b-versatile");
    assert_eq!(
        differs.ai.chat.model_provider, "groq",
        "the override is qualified to the connection it was read under"
    );
    assert_eq!(differs.ai.chat_model(), "llama-3.3-70b-versatile");

    // The same file after the connection moves: the override names a Groq
    // model and is not sent to another server.
    let moved = differs.ai.with_provider("deepseek", "deepseek-chat");
    assert_eq!(moved.chat_model(), "deepseek-chat");
}

#[test]
fn row_7_both_on_against_different_providers() {
    let c = parse(&whole_file(
        "enabled = true\npreset = \"groq\"\nbase_url = \"https://api.groq.com/openai/v1\"\nmodel = \"llama-3.3-70b-versatile\"\n\n[ai.chat]\nenabled = true\nprovider = \"anthropic\"\nmodel = \"claude-opus-5\"\n",
    ));
    assert_eq!(c.ai.provider, "groq", "the rewrite side is the connection");
    assert_eq!(c.ai.model, "llama-3.3-70b-versatile");
    assert!(c.ai.chat.enabled, "the switch is kept");
    assert!(
        c.ai.chat.model.is_empty(),
        "the chat's model went with its endpoint"
    );
    assert_eq!(c.ai.chat_model(), "llama-3.3-70b-versatile");
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
