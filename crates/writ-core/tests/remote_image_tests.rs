use writ_core::preview::remote_image::{
    check_remote_image_bytes, check_remote_image_size, is_followable_redirect, remote_image_host,
    remote_image_key, remote_image_route, remote_image_url, split_remote_image_request,
    RemoteImageRefusal, RemoteImageRequest, REMOTE_IMAGE_MAX_BYTES,
};

#[test]
fn only_http_and_https_references_are_remote() {
    assert!(remote_image_url("https://example.com/a.png").is_some());
    assert!(remote_image_url("HTTP://example.com/a.png").is_some());
    for reference in [
        "a.png",
        "assets/a.png",
        "file:///etc/passwd",
        "data:image/png;base64,AAAA",
        "javascript:alert(1)",
        "ftp://example.com/a.png",
        "https://",
        "//example.com/a.png",
    ] {
        assert!(remote_image_url(reference).is_none(), "{reference}");
    }
}

#[test]
fn credentials_and_fragments_never_reach_the_fetch() {
    let url = remote_image_url("https://user:pass@example.com/a.png#frag").unwrap();
    assert_eq!(url.as_str(), "https://example.com/a.png");
}

#[test]
fn the_placeholder_names_the_host_and_any_port() {
    let url = remote_image_url("https://images.example.com/a.png").unwrap();
    assert_eq!(remote_image_host(&url), "images.example.com");
    let url = remote_image_url("http://localhost:8080/a.png").unwrap();
    assert_eq!(remote_image_host(&url), "localhost:8080");
}

#[test]
fn the_key_is_stable_for_one_url_and_differs_across_urls() {
    let a = remote_image_url("https://example.com/a.png").unwrap();
    let b = remote_image_url("https://example.com/b.png").unwrap();
    assert_eq!(remote_image_key(&a), remote_image_key(&a));
    assert_ne!(remote_image_key(&a), remote_image_key(&b));
    assert_eq!(remote_image_key(&a).len(), 64);
}

#[test]
fn a_redirect_off_the_web_is_not_followed() {
    let ok = url::Url::parse("https://cdn.example.com/a.png").unwrap();
    let file = url::Url::parse("file:///etc/passwd").unwrap();
    assert!(is_followable_redirect(&ok));
    assert!(!is_followable_redirect(&file));
}

#[test]
fn a_body_over_the_cap_is_too_large() {
    assert!(check_remote_image_size(REMOTE_IMAGE_MAX_BYTES).is_ok());
    assert_eq!(
        check_remote_image_size(REMOTE_IMAGE_MAX_BYTES + 1),
        Err(RemoteImageRefusal::TooLarge)
    );
}

#[test]
fn a_body_is_typed_by_its_bytes() {
    assert_eq!(check_remote_image_bytes(b"GIF89a\x01\x00"), Ok("image/gif"));
    assert_eq!(
        check_remote_image_bytes(b"<!doctype html><svg>"),
        Err(RemoteImageRefusal::NotAnImage)
    );
}

#[test]
fn the_route_round_trips_through_the_split() {
    let url = remote_image_url("https://example.com/a.png").unwrap();
    let key = remote_image_key(&url);
    let route = remote_image_route("buf-1", "tok_2", &key).unwrap();
    assert_eq!(
        split_remote_image_request(&route),
        Some(RemoteImageRequest {
            buffer_id: "buf-1",
            token: "tok_2",
            key: &key,
        })
    );
}

#[test]
fn a_malformed_route_splits_to_nothing() {
    let key = "a".repeat(64);
    assert!(remote_image_route("buf/1", "tok", &key).is_none());
    assert!(remote_image_route("buf", "tok", "short").is_none());
    for path in [
        "_remote-image",
        "_remote-image/",
        "_remote-image/buf/tok",
        &format!("_remote-image/buf/tok/{key}/extra"),
        &format!("_remote-image/buf/tok/{}", "A".repeat(64)),
        &format!("_remote-image/buf//{key}"),
        &format!("_note-asset/buf/tok/{key}"),
    ] {
        assert!(split_remote_image_request(path).is_none(), "{path}");
    }
}

#[test]
fn refusals_map_to_the_editor_captions() {
    assert_eq!(RemoteImageRefusal::Unreachable.as_str(), "unreachable");
    assert_eq!(RemoteImageRefusal::TooLarge.as_str(), "too_large");
    assert_eq!(RemoteImageRefusal::NotAnImage.as_str(), "not_an_image");
}

fn held(bytes: &[u8]) -> writ_core::preview::remote_image::HeldImage {
    writ_core::preview::remote_image::HeldImage {
        mime: "image/png",
        bytes: bytes.into(),
    }
}

#[test]
fn a_held_image_is_read_only_by_the_tabs_that_asked_for_it() {
    use writ_core::preview::remote_image::RemoteImageStore;
    let mut store = RemoteImageStore::default();
    store.insert("k", "buf-1", held(b"one"));
    assert_eq!(store.read("k", "buf-1"), Some(held(b"one")));
    assert_eq!(store.read("k", "buf-2"), None);
    assert_eq!(store.grant("k", "buf-2"), Some(held(b"one")));
    assert_eq!(store.read("k", "buf-2"), Some(held(b"one")));
    assert_eq!(store.grant("missing", "buf-1"), None);
}

#[test]
fn the_least_recently_used_image_goes_when_the_store_is_full() {
    use writ_core::preview::remote_image::RemoteImageStore;
    let mut store = RemoteImageStore::with_capacity(10);
    store.insert("a", "buf", held(b"aaaa"));
    store.insert("b", "buf", held(b"bbbb"));
    assert!(store.read("a", "buf").is_some());
    store.insert("c", "buf", held(b"cccc"));
    assert!(store.read("a", "buf").is_some());
    assert!(store.read("b", "buf").is_none());
    assert!(store.read("c", "buf").is_some());
    assert_eq!(store.held_bytes(), 8);
}

#[test]
fn an_image_larger_than_the_store_is_not_held() {
    use writ_core::preview::remote_image::RemoteImageStore;
    let mut store = RemoteImageStore::with_capacity(4);
    store.insert("big", "buf", held(b"12345"));
    assert!(store.read("big", "buf").is_none());
    assert_eq!(store.held_bytes(), 0);
}

#[test]
fn holding_a_key_again_keeps_its_readers_and_counts_bytes_once() {
    use writ_core::preview::remote_image::RemoteImageStore;
    let mut store = RemoteImageStore::with_capacity(100);
    store.insert("k", "buf-1", held(b"one"));
    store.insert("k", "buf-2", held(b"three"));
    assert_eq!(store.read("k", "buf-1"), Some(held(b"three")));
    assert_eq!(store.held_bytes(), 5);
}
