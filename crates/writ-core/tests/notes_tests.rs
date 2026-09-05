use std::collections::HashSet;
use std::path::{Path, PathBuf};

use writ_core::notes::{
    conflict_file_name, date_stem, dedupe_file_name, display_path, recovered_file_name,
    refuse_notes_root, resolve_notes_root, resolve_notes_root_from, sanitize_title,
    sanitize_title_or, NotesRootError, NotesRootRefusal, NotesRootSources, DEFAULT_NOTES_FOLDER,
};

/// Absolute paths in these fixtures are spelled the Unix way; on Windows an
/// absolute path needs a drive, so every one goes through this.
fn abs(path: &str) -> String {
    if cfg!(windows) {
        format!("C:{path}")
    } else {
        path.to_string()
    }
}

fn home() -> PathBuf {
    PathBuf::from(abs("/home/tester"))
}

#[test]
fn resolve_notes_root_defaults_to_home_writ() {
    let resolved = resolve_notes_root(None, Some(&home())).unwrap();
    assert_eq!(resolved, PathBuf::from(abs("/home/tester/Writ")));
}

#[test]
fn resolve_notes_root_expands_leading_tilde() {
    let resolved = resolve_notes_root(Some("~/Documents/Notes"), Some(&home())).unwrap();
    assert_eq!(resolved, PathBuf::from(abs("/home/tester/Documents/Notes")));
}

#[test]
fn resolve_notes_root_rejects_relative_path() {
    let err = resolve_notes_root(Some("Notes"), Some(&home())).unwrap_err();
    assert_eq!(
        err,
        NotesRootError::NotAbsolute {
            path: "Notes".to_string()
        }
    );
}

#[test]
fn resolve_notes_root_treats_blank_as_default() {
    // A hand-edited blank must never stop the app launching.
    assert_eq!(
        resolve_notes_root(Some("   "), Some(&home())).unwrap(),
        PathBuf::from(abs("/home/tester/Writ"))
    );
    assert_eq!(
        resolve_notes_root(Some(""), Some(&home())).unwrap(),
        PathBuf::from(abs("/home/tester/Writ"))
    );
}

#[test]
fn resolve_notes_root_env_override_wins_over_config() {
    let resolved = resolve_notes_root_from(NotesRootSources {
        env_override: Some(abs("/tmp/writ-dev-1431/notes").as_str()),
        configured: Some(abs("/home/tester/Documents/Notes").as_str()),
        data_dir: Some(Path::new(&abs("/tmp/writ-dev-1431"))),
        home: Some(&home()),
    })
    .unwrap();
    assert_eq!(resolved, PathBuf::from(abs("/tmp/writ-dev-1431/notes")));
}

#[test]
fn resolve_notes_root_blank_env_override_falls_through_to_config() {
    let resolved = resolve_notes_root_from(NotesRootSources {
        env_override: Some("  "),
        configured: Some(abs("/home/tester/Documents/Notes").as_str()),
        data_dir: None,
        home: Some(&home()),
    })
    .unwrap();
    assert_eq!(resolved, PathBuf::from(abs("/home/tester/Documents/Notes")));
}

#[test]
fn resolve_notes_root_rejects_a_relative_env_override() {
    let err = resolve_notes_root_from(NotesRootSources {
        env_override: Some("notes"),
        configured: None,
        data_dir: None,
        home: Some(&home()),
    })
    .unwrap_err();
    assert_eq!(
        err,
        NotesRootError::NotAbsolute {
            path: "notes".to_string()
        }
    );
}

#[test]
fn resolve_notes_root_uses_the_data_dir_when_no_root_is_configured() {
    // A dev or recording instance must never create the real home folder one.
    let resolved = resolve_notes_root_from(NotesRootSources {
        env_override: None,
        configured: None,
        data_dir: Some(Path::new(&abs("/tmp/writ-dev-1431"))),
        home: Some(&home()),
    })
    .unwrap();
    assert_eq!(resolved, PathBuf::from(abs("/tmp/writ-dev-1431/Writ")));
}

#[test]
fn resolve_notes_root_prefers_a_configured_root_over_the_data_dir() {
    let resolved = resolve_notes_root_from(NotesRootSources {
        env_override: None,
        configured: Some(abs("/home/tester/Documents/Notes").as_str()),
        data_dir: Some(Path::new(&abs("/tmp/writ-dev-1431"))),
        home: Some(&home()),
    })
    .unwrap();
    assert_eq!(resolved, PathBuf::from(abs("/home/tester/Documents/Notes")));
}

#[test]
fn resolve_notes_root_falls_back_to_home_without_a_data_dir() {
    let resolved = resolve_notes_root_from(NotesRootSources {
        env_override: None,
        configured: None,
        data_dir: None,
        home: Some(&home()),
    })
    .unwrap();
    assert_eq!(resolved, PathBuf::from(abs("/home/tester/Writ")));
}

#[test]
fn resolve_notes_root_needs_no_home_once_a_data_dir_is_set() {
    let resolved = resolve_notes_root_from(NotesRootSources {
        env_override: None,
        configured: None,
        data_dir: Some(Path::new(&abs("/tmp/writ-dev-1431"))),
        home: None,
    })
    .unwrap();
    assert_eq!(resolved, PathBuf::from(abs("/tmp/writ-dev-1431/Writ")));
}

#[test]
fn resolve_notes_root_errors_without_home() {
    assert_eq!(
        resolve_notes_root(None, None).unwrap_err(),
        NotesRootError::NoHome
    );
    assert_eq!(
        resolve_notes_root(Some("~/Writ"), None).unwrap_err(),
        NotesRootError::NoHome
    );
}

#[test]
fn resolve_notes_root_keeps_an_absolute_configured_path() {
    let resolved = resolve_notes_root(Some(abs("/data/notes").as_str()), Some(&home())).unwrap();
    assert_eq!(resolved, PathBuf::from(abs("/data/notes")));
}

#[test]
fn display_path_collapses_home_to_tilde() {
    let shown = display_path(Path::new(&abs("/home/tester/Writ")), Some(&home()));
    assert_eq!(shown, Path::new("~").join("Writ").to_string_lossy());

    let outside = display_path(Path::new(&abs("/data/notes")), Some(&home()));
    assert_eq!(outside, abs("/data/notes"));

    let no_home = display_path(Path::new(&abs("/home/tester/Writ")), None);
    assert_eq!(no_home, abs("/home/tester/Writ"));
}

#[test]
fn date_stem_is_iso_calendar_day() {
    let now = chrono::Utc::now();
    let stem = date_stem(now);

    let expected = now
        .with_timezone(&chrono::Local)
        .format("%Y-%m-%d")
        .to_string();
    assert_eq!(stem, expected);
    assert_eq!(stem.len(), 10);
    assert!(chrono::NaiveDate::parse_from_str(&stem, "%Y-%m-%d").is_ok());
}

#[test]
fn dedupe_appends_space_two_then_three() {
    let mut taken = HashSet::new();
    assert_eq!(dedupe_file_name("Notes", "md", &taken), "Notes.md");

    taken.insert("notes.md".to_string());
    assert_eq!(dedupe_file_name("Notes", "md", &taken), "Notes 2.md");

    taken.insert("notes 2.md".to_string());
    assert_eq!(dedupe_file_name("Notes", "md", &taken), "Notes 3.md");
}

#[test]
fn dedupe_is_case_insensitive() {
    let mut taken = HashSet::new();
    taken.insert("notes.md".to_string());
    assert_eq!(dedupe_file_name("NOTES", "md", &taken), "NOTES 2.md");
}

#[test]
fn dedupe_without_an_extension_keeps_the_bare_stem() {
    let mut taken = HashSet::new();
    assert_eq!(dedupe_file_name("Notes", "", &taken), "Notes");

    taken.insert("notes".to_string());
    assert_eq!(dedupe_file_name("Notes", "", &taken), "Notes 2");
}

#[test]
fn sanitize_removes_path_separators() {
    let cleaned = sanitize_title("a/b").unwrap();
    assert_eq!(cleaned, "a b");
    assert!(!cleaned.contains('/'));

    let windows = sanitize_title("a\\b").unwrap();
    assert_eq!(windows, "a b");
    assert!(!windows.contains('\\'));
}

#[test]
fn sanitize_removes_colon() {
    let cleaned = sanitize_title("a:b").unwrap();
    assert_eq!(cleaned, "a b");
    assert!(!cleaned.contains(':'));
}

#[test]
fn sanitize_removes_the_remaining_illegal_characters() {
    let cleaned = sanitize_title("a<b>c\"d|e?f*g").unwrap();
    assert_eq!(cleaned, "a b c d e f g");
    for illegal in ['<', '>', '"', '|', '?', '*'] {
        assert!(!cleaned.contains(illegal), "{illegal} survived");
    }
}

#[test]
fn sanitize_strips_leading_dots() {
    assert_eq!(sanitize_title(".hidden").unwrap(), "hidden");
    assert_eq!(sanitize_title("..hidden").unwrap(), "hidden");
}

#[test]
fn sanitize_strips_trailing_dot() {
    assert_eq!(sanitize_title("name.").unwrap(), "name");
}

#[test]
fn sanitize_strips_trailing_space() {
    assert_eq!(sanitize_title("name ").unwrap(), "name");
}

#[test]
fn sanitize_collapses_whitespace_runs() {
    assert_eq!(sanitize_title("a   b\t\tc").unwrap(), "a b c");
}

#[test]
fn sanitize_suffixes_reserved_con() {
    assert_eq!(sanitize_title("CON").unwrap(), "CON_");
    assert_eq!(sanitize_title("com4").unwrap(), "com4_");
    assert_eq!(sanitize_title("LPT9").unwrap(), "LPT9_");
}

#[test]
fn sanitize_suffixes_reserved_nul_with_extension() {
    assert_eq!(sanitize_title("NUL.md").unwrap(), "NUL_.md");
}

#[test]
fn sanitize_leaves_a_name_that_only_starts_like_a_reserved_one() {
    assert_eq!(sanitize_title("CONTRACT").unwrap(), "CONTRACT");
    assert_eq!(sanitize_title("COM10").unwrap(), "COM10");
}

#[test]
fn sanitize_removes_control_characters() {
    let cleaned = sanitize_title("a\u{0}b\u{7}c\nd").unwrap();
    assert_eq!(cleaned, "a b c d");
    assert!(!cleaned.chars().any(char::is_control));
}

#[test]
fn sanitize_truncates_arabic_title_at_grapheme_boundary_under_200_bytes() {
    let raw: String = "م".repeat(300);
    assert_eq!(raw.len(), 600);

    let cleaned = sanitize_title(&raw).unwrap();

    assert!(cleaned.len() <= 200, "{} bytes", cleaned.len());
    assert!(std::str::from_utf8(cleaned.as_bytes()).is_ok());
    // Two bytes per character, so the byte cap lands before the 120-grapheme
    // cap and the last character has to be whole.
    assert_eq!(cleaned.chars().count(), 100);
    assert!(raw.starts_with(&cleaned));
}

#[test]
fn sanitize_truncates_a_long_ascii_title_to_120_graphemes() {
    let raw: String = "a".repeat(300);
    let cleaned = sanitize_title(&raw).unwrap();
    assert_eq!(cleaned.chars().count(), 120);
}

#[test]
fn sanitize_returns_none_for_all_illegal_input() {
    assert_eq!(sanitize_title("///"), None);
    assert_eq!(sanitize_title("   "), None);
    assert_eq!(sanitize_title("..."), None);
    assert_eq!(sanitize_title(""), None);
}

#[test]
fn sanitize_title_or_falls_back_when_nothing_survives() {
    assert_eq!(sanitize_title_or("///", "2026-08-28"), "2026-08-28");
    assert_eq!(
        sanitize_title_or("Meeting notes", "2026-08-28"),
        "Meeting notes"
    );
}

#[test]
fn conflict_file_name_is_the_stem_a_date_and_a_dotted_clock() {
    let now = chrono::DateTime::parse_from_rfc3339("2026-08-29T09:41:07Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let name = conflict_file_name("Meeting notes", "md", now);

    let local = now.with_timezone(&chrono::Local);
    assert_eq!(
        name,
        format!(
            "Meeting notes (conflict {}).md",
            local.format("%Y-%m-%d %H.%M.%S")
        )
    );
    assert!(!name.contains(':'), "{name}");
}

#[test]
fn conflict_file_name_without_an_extension_has_no_trailing_dot() {
    let now = chrono::Utc::now();
    let name = conflict_file_name("Makefile", "", now);
    assert!(name.starts_with("Makefile (conflict "), "{name}");
    assert!(name.ends_with(')'), "{name}");
}

#[test]
fn recovered_file_name_carries_the_same_dated_shape() {
    let now = chrono::DateTime::parse_from_rfc3339("2026-08-29T09:41:07Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let name = recovered_file_name("Meeting notes", "md", now);

    let local = now.with_timezone(&chrono::Local);
    assert_eq!(
        name,
        format!(
            "Meeting notes (recovered {}).md",
            local.format("%Y-%m-%d %H.%M.%S")
        )
    );
    assert_ne!(name, conflict_file_name("Meeting notes", "md", now));
}

/// Writ's data folder in the tests below, with the notes folder beside it.
fn writ_dir() -> PathBuf {
    PathBuf::from(abs("/home/tester/.local/share/writ"))
}

fn notes_root() -> PathBuf {
    PathBuf::from(abs("/home/tester/Writ"))
}

#[test]
fn the_data_folder_itself_cannot_be_the_notes_folder() {
    assert_eq!(
        refuse_notes_root(&writ_dir(), &notes_root(), &writ_dir()),
        Some(NotesRootRefusal::HoldsWritData)
    );
}

#[test]
fn a_folder_above_the_data_folder_cannot_be_the_notes_folder() {
    let above = PathBuf::from(abs("/home/tester/.local/share"));
    assert_eq!(
        refuse_notes_root(&above, &notes_root(), &writ_dir()),
        Some(NotesRootRefusal::HoldsWritData)
    );
}

#[test]
fn a_folder_inside_the_data_folder_cannot_be_the_notes_folder() {
    let archive = writ_dir().join("archive");
    assert_eq!(
        refuse_notes_root(&archive, &notes_root(), &writ_dir()),
        Some(NotesRootRefusal::HoldsWritData),
        "the archive folder would become its own destination"
    );
}

#[test]
fn a_folder_beside_the_data_folder_is_accepted() {
    let beside = PathBuf::from(abs("/home/tester/.local/share/writing"));
    assert_eq!(refuse_notes_root(&beside, &notes_root(), &writ_dir()), None);
    assert_eq!(
        refuse_notes_root(
            &PathBuf::from(abs("/home/tester/Dropbox/Notes")),
            &notes_root(),
            &writ_dir()
        ),
        None
    );
}

#[test]
fn a_folder_inside_the_notes_folder_is_refused_and_the_notes_folder_itself_is_not() {
    assert_eq!(
        refuse_notes_root(&notes_root().join("deeper"), &notes_root(), &writ_dir()),
        Some(NotesRootRefusal::InsideNotesFolder)
    );
    assert_eq!(
        refuse_notes_root(&notes_root(), &notes_root(), &writ_dir()),
        None,
        "picking the folder it is already in has nothing to do"
    );
}

#[test]
fn a_folder_that_contains_the_notes_folder_is_an_ordinary_move() {
    let current = PathBuf::from(abs("/volumes/sync/Notes/Writ"));
    assert_eq!(
        refuse_notes_root(current.parent().unwrap(), &current, &writ_dir()),
        None
    );
}

#[test]
fn the_default_folder_under_the_data_folder_is_accepted() {
    let default_under_data = writ_dir().join(DEFAULT_NOTES_FOLDER);
    assert_eq!(
        refuse_notes_root(&default_under_data, &notes_root(), &writ_dir()),
        None,
        "an instance running against its own data folder keeps its notes there"
    );
    assert_eq!(
        resolve_notes_root_from(NotesRootSources {
            data_dir: Some(&writ_dir()),
            home: Some(&home()),
            ..NotesRootSources::default()
        })
        .unwrap(),
        default_under_data,
        "and that is the folder the resolver picks"
    );
    assert_eq!(
        refuse_notes_root(&writ_dir().join("archive"), &notes_root(), &writ_dir()),
        Some(NotesRootRefusal::HoldsWritData),
        "every other folder inside the data folder stays out"
    );
    assert_eq!(
        refuse_notes_root(
            &writ_dir().join(DEFAULT_NOTES_FOLDER).join("deeper"),
            &notes_root(),
            &writ_dir()
        ),
        Some(NotesRootRefusal::HoldsWritData),
        "and so does a folder under the accepted one"
    );
}
