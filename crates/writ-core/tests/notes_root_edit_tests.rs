use writ_core::config::{NotesConfig, NotesRootEditError, WritConfig};

const BAD: &str = "# Writ settings\n[editor]\nfont_size = \"16\" # too big?\ntab_size = 4\n";

fn root_of(text: &str) -> Option<String> {
    WritConfig::parse_lenient(text).config.notes.root
}

#[test]
fn a_file_without_the_table_gains_it_at_the_end_and_keeps_every_other_byte() {
    let edited = NotesConfig::write_root_into(BAD, "/Users/me/Moved").unwrap();

    assert_eq!(
        edited,
        format!("{BAD}\n[notes]\nroot = \"/Users/me/Moved\"\n")
    );
    let read = WritConfig::parse_lenient(&edited);
    assert_eq!(read.config.notes.root.as_deref(), Some("/Users/me/Moved"));
    assert_eq!(read.diagnostics.len(), 1);
    assert_eq!(read.diagnostics[0].key, "editor.font_size");
    assert_eq!(read.diagnostics[0].line, 3);
}

#[test]
fn a_root_already_there_is_replaced_in_place_with_its_comment() {
    let text = format!(
        "{BAD}\n[notes]\nroot = \"/Users/me/Old\" # picked in Settings\n\n[sidebar]\nwidth = 300\n"
    );

    let edited = NotesConfig::write_root_into(&text, "/Users/me/New").unwrap();

    assert_eq!(
        edited,
        format!("{BAD}\n[notes]\nroot = \"/Users/me/New\" # picked in Settings\n\n[sidebar]\nwidth = 300\n")
    );
}

#[test]
fn a_table_without_a_root_takes_it_and_the_tables_after_it_stay() {
    let text = "[notes]\n\n[sidebar]\nwidth = 300\n";

    let edited = NotesConfig::write_root_into(text, "/srv/files").unwrap();

    let read = WritConfig::parse(&edited).unwrap();
    assert_eq!(read.notes.root.as_deref(), Some("/srv/files"));
    assert_eq!(read.sidebar.width, 300);
    assert!(edited.ends_with("[sidebar]\nwidth = 300\n"));
}

#[test]
fn an_inline_table_and_a_dotted_key_are_written_in_their_own_form() {
    let inline = NotesConfig::write_root_into("notes = { root = \"/a\" }\n", "/b").unwrap();
    assert_eq!(inline, "notes = { root = \"/b\" }\n");

    let dotted = NotesConfig::write_root_into("notes.root = \"/a\"\n", "/b").unwrap();
    assert_eq!(dotted, "notes.root = \"/b\"\n");
}

#[test]
fn a_root_of_the_wrong_kind_is_replaced() {
    let edited = NotesConfig::write_root_into("[notes]\nroot = 5\n", "/b").unwrap();
    assert_eq!(root_of(&edited).as_deref(), Some("/b"));
    assert!(WritConfig::parse(&edited).is_ok());
}

#[test]
fn a_file_without_a_final_newline_still_reads_after_the_write() {
    let edited = NotesConfig::write_root_into("[editor]\ntab_size = 4", "/b").unwrap();
    let read = WritConfig::parse(&edited).unwrap();
    assert_eq!(read.notes.root.as_deref(), Some("/b"));
    assert_eq!(read.editor.tab_size, 4);
}

#[test]
fn a_path_that_needs_escaping_reads_back_as_it_was() {
    for root in [
        "C:\\Users\\me\\My \"Files\"",
        "/Users/me/ملفات/O'Brien's notes",
        "/tmp/tab\there",
    ] {
        let edited = NotesConfig::write_root_into(BAD, root).unwrap();
        assert_eq!(root_of(&edited).as_deref(), Some(root), "{edited}");
    }
}

#[test]
fn an_empty_file_gains_only_the_table() {
    let edited = NotesConfig::write_root_into("", "/b").unwrap();
    let read = WritConfig::parse(&edited).unwrap();
    assert_eq!(read.notes.root.as_deref(), Some("/b"));
}

#[test]
fn a_file_that_is_not_toml_is_left_alone() {
    let error = NotesConfig::write_root_into("[editor\nfont_size = 16\n", "/b").unwrap_err();
    assert!(
        matches!(error, NotesRootEditError::NotToml { .. }),
        "{error:?}"
    );
}

#[test]
fn a_plain_value_where_the_table_belongs_is_left_alone() {
    let error = NotesConfig::write_root_into("notes = 5\n", "/b").unwrap_err();
    assert_eq!(error, NotesRootEditError::NotATable);
}
