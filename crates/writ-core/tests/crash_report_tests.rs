//! The panic report keeps where and what kind, never the message (ADR-045).

use writ_core::crash::{
    crash_report_file_name, format_crash_report, AddressAnchor, CrashFiles, PanicSummary,
    PayloadKind, DIGEST_PREFIX_LEN, MAX_CRASH_FILES,
};
use writ_core::hash::sha256_hex;

const NOTE_TEXT: &str = "meeting notes: the password is hunter2";

fn anchor() -> AddressAnchor {
    AddressAnchor {
        symbol: "writ_tauri_lib::logging::panic_handler::install_panic_handler",
        address: 0x1_0000_4f20,
    }
}

fn summary_of(payload: Box<dyn std::any::Any + Send>) -> PanicSummary {
    PanicSummary::new(
        payload.as_ref(),
        Some("crates/writ-lint/src/utf16.rs:42:17".to_string()),
        Some("main".to_string()),
    )
}

#[test]
fn a_string_message_is_described_by_its_length_and_digest_prefix() {
    let summary = summary_of(Box::new(NOTE_TEXT.to_string()));

    assert_eq!(summary.payload_kind, PayloadKind::String);
    assert_eq!(summary.payload_len, Some(NOTE_TEXT.len()));
    assert_eq!(
        summary.payload_digest.as_deref(),
        Some(&sha256_hex(NOTE_TEXT.as_bytes())[..DIGEST_PREFIX_LEN])
    );
}

#[test]
fn a_static_str_message_is_described_the_same_way() {
    let summary = summary_of(Box::new(NOTE_TEXT));

    assert_eq!(summary.payload_kind, PayloadKind::StaticStr);
    assert_eq!(summary.payload_len, Some(NOTE_TEXT.len()));
    assert_eq!(summary.payload_digest.as_ref().map(String::len), Some(16));
}

#[test]
fn a_payload_that_is_not_text_has_no_length_or_digest() {
    let summary = summary_of(Box::new(42_u32));

    assert_eq!(summary.payload_kind, PayloadKind::Other);
    assert_eq!(summary.payload_len, None);
    assert_eq!(summary.payload_digest, None);
    assert!(summary.log_line().contains("other payload"));
}

#[test]
fn neither_the_log_line_nor_the_report_carries_the_message() {
    let summary = summary_of(Box::new(format!(
        "byte index 1 is not a char boundary; `{NOTE_TEXT}`"
    )));
    let report = format_crash_report(
        &summary,
        "20261014-101500",
        anchor(),
        "   0: 0x1000 - <unknown>\n",
    );

    for text in [summary.log_line(), report] {
        assert!(!text.contains("hunter2"), "{text}");
        assert!(!text.contains("meeting notes"), "{text}");
    }
}

#[test]
fn the_report_names_the_location_thread_payload_anchor_and_backtrace() {
    let summary = summary_of(Box::new(NOTE_TEXT.to_string()));
    let report = format_crash_report(
        &summary,
        "20261014-101500",
        anchor(),
        "   0: 0x1000 - <unknown>",
    );

    assert!(report.contains("Time: 20261014-101500\n"));
    assert!(report.contains("Thread: main\n"));
    assert!(report.contains("Location: crates/writ-lint/src/utf16.rs:42:17\n"));
    assert!(report.contains(&format!(
        "Payload: String message, {} bytes, sha256 {}\n",
        NOTE_TEXT.len(),
        summary.payload_digest.as_deref().unwrap()
    )));
    assert!(report.contains(
        "Anchor: writ_tauri_lib::logging::panic_handler::install_panic_handler at 0x100004f20\n"
    ));
    assert!(report.ends_with("Backtrace:\n   0: 0x1000 - <unknown>\n"));
}

#[test]
fn a_panic_without_a_location_or_thread_name_still_reports() {
    let summary = PanicSummary::new(&"boom", None, None);

    assert_eq!(
        summary.log_line(),
        format!(
            "panic at an unknown location on thread <unnamed>: &str message, 4 bytes, sha256 {}",
            &sha256_hex(b"boom")[..DIGEST_PREFIX_LEN]
        )
    );
}

#[test]
fn the_crash_file_name_carries_the_timestamp_and_sequence() {
    assert_eq!(
        crash_report_file_name("20261014-101500", 2),
        "writ-crash-20261014-101500-2.log"
    );
}

fn panic_at(location: &str, message: &str) -> PanicSummary {
    PanicSummary::new(
        &message.to_string(),
        Some(location.to_string()),
        Some("main".to_string()),
    )
}

#[test]
fn a_repeated_panic_gets_one_file_and_a_different_one_gets_its_own() {
    let mut files = CrashFiles::default();
    let render = panic_at("src/markdown.rs:10:5", "slice error");

    assert_eq!(files.admit(&render), Some(1));
    assert_eq!(files.admit(&render), None);
    assert_eq!(
        files.admit(&panic_at("src/markdown.rs:10:5", "other text")),
        Some(2)
    );
    assert_eq!(
        files.admit(&panic_at("src/lib.rs:99:1", "slice error")),
        Some(3)
    );
}

#[test]
fn a_process_writes_at_most_the_cap() {
    let mut files = CrashFiles::default();
    let admitted = (0..20)
        .filter_map(|line| files.admit(&panic_at(&format!("src/x.rs:{line}:1"), "m")))
        .count();

    assert_eq!(admitted, MAX_CRASH_FILES);
}
