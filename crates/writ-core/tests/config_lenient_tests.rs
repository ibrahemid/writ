use writ_core::config::{ConfigDiagnostic, FileExtension, Polarity, WritConfig};

#[test]
fn a_file_that_reads_in_full_reads_as_the_strict_parse_with_nothing_to_say() {
    let text = "[editor]\nfont_size = 18\n\n[appearance]\npolarity = \"dark\"\n";
    let read = WritConfig::parse_lenient(text);
    assert!(read.diagnostics.is_empty());
    assert_eq!(read.config, WritConfig::parse(text).unwrap());
}

#[test]
fn one_invalid_value_keeps_the_valid_tables_and_the_rest_of_its_own() {
    let text = "[editor]\n\
                font_size = \"16\"\n\
                tab_size = 4\n\
                \n\
                [appearance]\n\
                polarity = \"dark\"\n\
                \n\
                [files]\n\
                default_extension = \"md\"\n";
    let read = WritConfig::parse_lenient(text);

    assert_eq!(read.diagnostics.len(), 1, "{:?}", read.diagnostics);
    let ConfigDiagnostic { key, line, message } = &read.diagnostics[0];
    assert_eq!(key, "editor.font_size");
    assert_eq!(*line, 2);
    assert!(!message.is_empty());

    // The bad value reads as a file without it would.
    let absent = WritConfig::parse("[editor]\ntab_size = 4\n").unwrap();
    assert_eq!(read.config.editor.font_size, absent.editor.font_size);
    // Its neighbour and the other tables keep what the file says.
    assert_eq!(read.config.editor.tab_size, 4);
    assert_eq!(read.config.appearance.polarity, Polarity::Dark);
    assert_eq!(read.config.files.default_extension, FileExtension::Md);
}

#[test]
fn every_invalid_value_is_named_with_its_own_line() {
    let text = "[appearance]\n\
                polarity = \"Dark\"\n\
                \n\
                [files]\n\
                default_extension = \"rtf\"\n\
                \n\
                [keybindings]\n\
                \"tab.new\" = \"CmdOrCtrl+T\"\n\
                \"tab.close\" = 5\n";
    let read = WritConfig::parse_lenient(text);

    let mut named: Vec<(String, u32)> = read
        .diagnostics
        .iter()
        .map(|d| (d.key.clone(), d.line))
        .collect();
    named.sort();
    assert_eq!(
        named,
        vec![
            ("appearance.polarity".to_string(), 2),
            ("files.default_extension".to_string(), 5),
            ("keybindings.tab.close".to_string(), 9),
        ]
    );
    assert_eq!(
        read.config.keybindings.get("tab.new").map(String::as_str),
        Some("CmdOrCtrl+T")
    );
    assert!(!read.config.keybindings.contains_key("tab.close"));
    assert_eq!(read.config.files.default_extension, FileExtension::Txt);
}

#[test]
fn a_file_that_is_not_toml_runs_on_the_defaults_and_names_the_line() {
    let text = "[editor]\nfont_size = 18\n[appearance\npolarity = \"dark\"\n";
    let read = WritConfig::parse_lenient(text);

    assert_eq!(read.config, WritConfig::default());
    assert_eq!(read.diagnostics.len(), 1);
    assert_eq!(read.diagnostics[0].key, "");
    assert_eq!(read.diagnostics[0].line, 3);
}

#[test]
fn a_top_level_value_of_the_wrong_kind_is_named_by_its_table() {
    let read = WritConfig::parse_lenient("hotkey = 5\n\n[editor]\nfont_size = 20\n");
    assert_eq!(read.diagnostics.len(), 1);
    assert_eq!(read.diagnostics[0].key, "hotkey");
    assert_eq!(read.diagnostics[0].line, 1);
    assert_eq!(read.config.editor.font_size, 20);
    assert_eq!(read.config.hotkey, WritConfig::default().hotkey);
}

#[test]
fn a_dropped_value_still_settles_the_apps() {
    // A hidden tags section moves onto the Tags switch whatever else fails.
    let read = WritConfig::parse_lenient(
        "[sidebar]\nhidden = [\"tags\"]\nwidth = \"wide\"\n\n[apps]\ntags = true\n",
    );
    assert_eq!(read.diagnostics.len(), 1);
    assert_eq!(read.diagnostics[0].key, "sidebar.width");
    assert!(!read.config.apps.tags);
    assert!(read.config.sidebar.hidden.is_empty());
}
