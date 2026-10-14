use writ_core::command::usage::{usage_from_config_text, CommandUsage};

#[test]
fn the_usage_table_of_an_older_config_is_read_for_the_move() {
    let text = "[editor]\nfont_size = 18\n\n\
                [commands.usage.\"palette.open\"]\ncount = 17\nlast_used_ms = 1715500000000\n\n\
                [commands.usage.\"tab.new\"]\ncount = 2\n";
    let usage = usage_from_config_text(text);
    assert_eq!(usage.len(), 2);
    assert_eq!(
        usage["palette.open"],
        CommandUsage {
            count: 17,
            last_used_ms: 1_715_500_000_000
        }
    );
    assert_eq!(
        usage["tab.new"],
        CommandUsage {
            count: 2,
            last_used_ms: 0
        }
    );
}

#[test]
fn a_config_without_the_table_or_that_does_not_parse_gives_nothing() {
    assert!(usage_from_config_text("[editor]\nfont_size = 18\n").is_empty());
    assert!(usage_from_config_text("").is_empty());
    assert!(usage_from_config_text("[commands\nusage =").is_empty());
}

#[test]
fn an_entry_of_the_wrong_shape_is_skipped_and_the_rest_move() {
    let text = "[commands.usage.\"a\"]\ncount = \"many\"\n\n\
                [commands.usage.\"b\"]\ncount = 1\nlast_used_ms = 5\n";
    let usage = usage_from_config_text(text);
    assert_eq!(usage.len(), 1);
    assert_eq!(usage["b"].count, 1);
}
