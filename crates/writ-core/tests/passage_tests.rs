//! One passage replaced, found exactly once.

use writ_core::notes::passage::{replace_passage, PassageError, Replaced};

fn replaced(text: &str, line: u32) -> Result<Replaced, PassageError> {
    Ok(Replaced {
        text: text.to_string(),
        line,
    })
}

#[test]
fn the_one_place_holding_the_text_is_replaced_and_nothing_else() {
    assert_eq!(
        replace_passage("one\ntwo\nthree\n", "two", "2"),
        replaced("one\n2\nthree\n", 2)
    );
    assert_eq!(
        replace_passage("# Plan\n\nShip it Friday.\n", "Friday", "Monday"),
        replaced("# Plan\n\nShip it Monday.\n", 3)
    );
}

#[test]
fn text_the_note_does_not_hold_is_refused() {
    assert_eq!(
        replace_passage("one\ntwo\n", "three", "3"),
        Err(PassageError::Absent)
    );
    assert_eq!(
        replace_passage("One\n", "one", "1"),
        Err(PassageError::Absent),
        "the match is exact, case included"
    );
}

#[test]
fn text_the_note_holds_more_than_once_is_refused_with_the_count() {
    assert_eq!(
        replace_passage("a b a b a", "a", "x"),
        Err(PassageError::Repeated { count: 3 })
    );
    assert_eq!(
        replace_passage("aaa", "aa", "x"),
        Err(PassageError::Repeated { count: 2 }),
        "overlapping places count"
    );
}

#[test]
fn empty_text_to_replace_is_refused() {
    assert_eq!(replace_passage("one", "", "x"), Err(PassageError::Empty));
}

#[test]
fn a_crlf_note_takes_lf_lines_and_keeps_its_own() {
    assert_eq!(
        replace_passage("one\r\ntwo\r\nthree\r\n", "one\ntwo", "1\n2"),
        replaced("1\r\n2\r\nthree\r\n", 1)
    );
    assert_eq!(
        replace_passage("one\r\ntwo\r\n", "two", "2\nand a half"),
        replaced("one\r\n2\r\nand a half\r\n", 2)
    );
}

#[test]
fn a_replacement_with_the_same_text_leaves_the_text_as_it_was() {
    assert_eq!(
        replace_passage("one\ntwo\n", "two", "two"),
        replaced("one\ntwo\n", 2)
    );
}

#[test]
fn text_past_a_multibyte_character_is_found() {
    assert_eq!(
        replace_passage("café ☕ then tea", "tea", "more tea"),
        replaced("café ☕ then more tea", 1)
    );
}
