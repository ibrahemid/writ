use std::time::{Duration, Instant};
use writ_core::watcher::ignore::{IgnoreStamps, SuppressDecision};

const TTL: Duration = Duration::from_secs(5);

#[test]
fn internal_write_without_external_change_suppresses() {
    let mut stamps = IgnoreStamps::new();
    let now = Instant::now();
    let content = b"hello internal";

    stamps.record(String::from("writ-123"), content, now);

    let decision = stamps.decide("writ-123", Some(content), now, TTL);
    assert_eq!(decision, SuppressDecision::Suppress);
}

#[test]
fn internal_write_coalesced_with_external_write_emits() {
    let mut stamps = IgnoreStamps::new();
    let now = Instant::now();
    let internal_bytes = b"writ wrote this";
    let external_bytes = b"someone else wrote this";

    stamps.record(String::from("writ-123"), internal_bytes, now);

    let decision = stamps.decide("writ-123", Some(external_bytes), now, TTL);
    assert_eq!(decision, SuppressDecision::Emit);
}

#[test]
fn stamp_older_than_ttl_emits() {
    let mut stamps = IgnoreStamps::new();
    let earlier = Instant::now();
    let content = b"stale content";

    stamps.record(String::from("writ-123"), content, earlier);

    let later = earlier + TTL + Duration::from_millis(1);
    let decision = stamps.decide("writ-123", Some(content), later, TTL);
    assert_eq!(decision, SuppressDecision::Emit);
}

#[test]
fn no_stamp_recorded_emits() {
    let mut stamps = IgnoreStamps::new();
    let now = Instant::now();
    let decision = stamps.decide("writ-unknown", Some(b"any content"), now, TTL);
    assert_eq!(decision, SuppressDecision::Emit);
}

#[test]
fn missing_file_on_disk_treated_as_external_and_emits() {
    let mut stamps = IgnoreStamps::new();
    let now = Instant::now();
    stamps.record(String::from("writ-123"), b"content", now);

    let decision = stamps.decide("writ-123", None, now, TTL);
    assert_eq!(decision, SuppressDecision::Emit);
}

#[test]
fn suppression_consumes_stamp_so_second_event_emits() {
    let mut stamps = IgnoreStamps::new();
    let now = Instant::now();
    let content = b"same bytes";

    stamps.record(String::from("writ-123"), content, now);
    let first = stamps.decide("writ-123", Some(content), now, TTL);
    let second = stamps.decide("writ-123", Some(content), now, TTL);

    assert_eq!(first, SuppressDecision::Suppress);
    assert_eq!(second, SuppressDecision::Emit);
}

#[test]
fn remove_clears_stamp() {
    let mut stamps = IgnoreStamps::new();
    let now = Instant::now();
    let content = b"x";
    stamps.record(String::from("writ-123"), content, now);
    stamps.remove("writ-123");

    let decision = stamps.decide("writ-123", Some(content), now, TTL);
    assert_eq!(decision, SuppressDecision::Emit);
}

#[test]
fn record_evicts_expired_stamps_for_other_files() {
    let mut stamps = IgnoreStamps::new();
    let t0 = Instant::now();
    stamps.record(String::from("writ-old"), b"a", t0);
    stamps.record(String::from("writ-also-old"), b"b", t0);

    let t1 = t0 + TTL + Duration::from_millis(1);
    stamps.record_with_ttl(String::from("writ-new"), b"c", t1, TTL);

    assert_eq!(stamps.len(), 1);
    assert!(stamps.contains("writ-new"));
}
