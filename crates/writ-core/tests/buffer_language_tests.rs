use writ_core::buffer::language::{validate_language_choice, LANGUAGE_CHOICE_MAX_CHARS};
use writ_core::errors::WritError;

fn reason_for(choice: &str) -> String {
    match validate_language_choice(Some(choice)) {
        Err(WritError::InvalidLanguage { reason }) => reason,
        other => panic!("expected InvalidLanguage for {choice:?}, got {other:?}"),
    }
}

#[test]
fn clearing_the_choice_always_passes() {
    assert!(validate_language_choice(None).is_ok());
}

#[test]
fn every_kind_of_name_the_editor_offers_passes() {
    for id in [
        "python",
        "plaintext",
        "c++",
        "c#",
        "objective-c++",
        "closure stylesheets (gss)",
        "asn.1",
        "ttcn_cfg",
        "xù",
    ] {
        assert!(
            validate_language_choice(Some(id)).is_ok(),
            "{id} was refused"
        );
    }
}

#[test]
fn an_empty_id_is_refused() {
    assert_eq!(reason_for(""), "the language id is empty");
}

#[test]
fn an_id_with_surrounding_whitespace_is_refused() {
    assert_eq!(
        reason_for(" python"),
        "the language id starts or ends with whitespace"
    );
    assert_eq!(
        reason_for("python\n"),
        "the language id starts or ends with whitespace"
    );
}

#[test]
fn an_id_with_a_control_character_is_refused() {
    assert_eq!(
        reason_for("py\u{0}thon"),
        "the language id holds a control character"
    );
}

#[test]
fn the_length_limit_counts_characters() {
    let longest = "ù".repeat(LANGUAGE_CHOICE_MAX_CHARS);
    assert!(validate_language_choice(Some(&longest)).is_ok());
    let over = "a".repeat(LANGUAGE_CHOICE_MAX_CHARS + 1);
    assert_eq!(
        reason_for(&over),
        format!("the language id is longer than {LANGUAGE_CHOICE_MAX_CHARS} characters")
    );
}
