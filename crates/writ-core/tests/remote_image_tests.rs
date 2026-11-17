use std::net::IpAddr;

use url::Url;
use writ_core::preview::remote_image::{
    check_remote_image_bytes, check_remote_image_size, embedded_ipv4, is_allowed_redirect_scheme,
    is_followable_redirect, is_local_address, is_local_host, is_local_host_name,
    local_address_scope, local_host_name_scope, local_host_scope, remote_image_host,
    remote_image_key, remote_image_route, remote_image_target, remote_image_url,
    split_remote_image_request, LocalScope, RemoteImageLoad, RemoteImageRefusal,
    RemoteImageRequest, RemoteImageScheme, RemoteImageTarget, REMOTE_IMAGE_MAX_BYTES,
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
    assert_eq!(RemoteImageRefusal::NotRemote.as_str(), "not_remote");
    assert_eq!(RemoteImageRefusal::Unreachable.as_str(), "unreachable");
    assert_eq!(RemoteImageRefusal::TooLarge.as_str(), "too_large");
    assert_eq!(RemoteImageRefusal::NotAnImage.as_str(), "not_an_image");
    assert_eq!(RemoteImageRefusal::LocalAddress.as_str(), "local_address");
    assert_eq!(
        RemoteImageRefusal::ResolvesToLocal(LocalScope::ThisComputer).as_str(),
        "resolves_to_this_computer"
    );
    assert_eq!(
        RemoteImageRefusal::ResolvesToLocal(LocalScope::LocalNetwork).as_str(),
        "resolves_to_local_network"
    );
    assert_eq!(
        RemoteImageRefusal::RedirectsToLocal(LocalScope::ThisComputer).as_str(),
        "redirects_to_this_computer"
    );
    assert_eq!(
        RemoteImageRefusal::RedirectsToLocal(LocalScope::LocalNetwork).as_str(),
        "redirects_to_local_network"
    );
    assert_eq!(RemoteImageRefusal::NotSecure.as_str(), "not_secure");
}

const LOADS: [RemoteImageLoad; 2] = [RemoteImageLoad::Asked, RemoteImageLoad::Automatic];

fn web(text: &str) -> Url {
    Url::parse(text).unwrap()
}

#[test]
fn a_secure_reference_is_requested_as_written() {
    let url = web("https://images.example.com:8443/a.png?size=2");
    for load in LOADS {
        assert_eq!(
            remote_image_target(&url, load),
            Ok(RemoteImageTarget {
                url: url.clone(),
                scheme: RemoteImageScheme::Secure,
            }),
            "{load:?}"
        );
    }
}

#[test]
fn a_plain_reference_to_a_public_host_is_requested_over_https() {
    for load in LOADS {
        for (written, requested) in [
            (
                "http://images.example.com/a.png?size=2",
                "https://images.example.com/a.png?size=2",
            ),
            (
                "http://images.example.com:80/a.png",
                "https://images.example.com/a.png",
            ),
            (
                "http://images.example.com:8080/a.png",
                "https://images.example.com:8080/a.png",
            ),
            ("http://93.184.216.34/a.png", "https://93.184.216.34/a.png"),
            (
                "http://[2606:4700::1111]/a.png",
                "https://[2606:4700::1111]/a.png",
            ),
        ] {
            let target = remote_image_target(&web(written), load).unwrap();
            assert_eq!(target.url.as_str(), requested, "{written} {load:?}");
            assert_eq!(target.scheme, RemoteImageScheme::Upgraded, "{written}");
        }
    }
}

#[test]
fn a_plain_reference_to_a_local_host_stays_plain_on_a_click_only() {
    for written in [
        "http://localhost:8080/a.png",
        "http://127.0.0.1/a.png",
        "http://[::1]:9/a.png",
        "http://192.168.1.10/a.png",
        "http://nas.local/a.png",
    ] {
        let url = web(written);
        assert_eq!(
            remote_image_target(&url, RemoteImageLoad::Asked),
            Ok(RemoteImageTarget {
                url: url.clone(),
                scheme: RemoteImageScheme::LocalPlain,
            }),
            "{written}"
        );
        assert_eq!(
            remote_image_target(&url, RemoteImageLoad::Automatic),
            Err(RemoteImageRefusal::LocalAddress),
            "{written}"
        );
    }
    let secure = web("https://nas.local/a.png");
    assert_eq!(
        remote_image_target(&secure, RemoteImageLoad::Asked).map(|target| target.scheme),
        Ok(RemoteImageScheme::Secure)
    );
    assert_eq!(
        remote_image_target(&secure, RemoteImageLoad::Automatic),
        Err(RemoteImageRefusal::LocalAddress)
    );
}

#[test]
fn only_a_web_address_has_a_target() {
    for written in [
        "file:///etc/passwd",
        "ftp://example.com/a.png",
        "data:image/png;base64,AAAA",
    ] {
        for load in LOADS {
            assert_eq!(
                remote_image_target(&web(written), load),
                Err(RemoteImageRefusal::NotRemote),
                "{written} {load:?}"
            );
        }
    }
}

#[test]
fn a_redirect_keeps_http_only_on_a_local_host() {
    for allowed in [
        "https://cdn.example.net/b.png",
        "https://nas.local/b.png",
        "http://nas.local:8080/b.png",
        "http://10.0.0.1/b.png",
        "http://localhost:9/b.png",
    ] {
        assert!(is_allowed_redirect_scheme(&web(allowed)), "{allowed}");
    }
    for refused in [
        "http://cdn.example.net/b.png",
        "http://93.184.216.34/b.png",
        "file:///etc/passwd",
    ] {
        assert!(!is_allowed_redirect_scheme(&web(refused)), "{refused}");
    }
}

#[test]
fn only_an_upgraded_request_fails_as_not_secure() {
    assert_eq!(
        RemoteImageScheme::Upgraded.failure(),
        RemoteImageRefusal::NotSecure
    );
    assert_eq!(
        RemoteImageScheme::Secure.failure(),
        RemoteImageRefusal::Unreachable
    );
    assert_eq!(
        RemoteImageScheme::LocalPlain.failure(),
        RemoteImageRefusal::Unreachable
    );
}

#[test]
fn loopback_and_unspecified_addresses_are_this_computer() {
    for text in [
        "127.0.0.1",
        "127.255.255.254",
        "0.0.0.0",
        "0.1.2.3",
        "::1",
        "::",
        "::ffff:127.0.0.1",
        "::ffff:0.0.0.0",
        "::7f00:1",
    ] {
        assert_eq!(
            local_address_scope(addr(text)),
            Some(LocalScope::ThisComputer),
            "{text}"
        );
    }
}

#[test]
fn private_link_local_unique_local_and_shared_addresses_are_the_local_network() {
    for text in [
        "10.0.0.5",
        "172.16.0.1",
        "192.168.1.1",
        "169.254.169.254",
        "100.64.0.1",
        "fd00::5",
        "fc00::1",
        "fe80::1",
        "::ffff:192.168.1.1",
        "64:ff9b:1::a00:1",
        "64:ff9b::a00:1",
        "2002:c0a8:101::1",
        "64:ff9b::7f00:1",
        "2002:7f00:1::1",
    ] {
        assert_eq!(
            local_address_scope(addr(text)),
            Some(LocalScope::LocalNetwork),
            "{text}"
        );
    }
}

#[test]
fn public_addresses_have_no_local_scope() {
    for text in [
        "8.8.8.8",
        "2606:4700::1111",
        "64:ff9b::808:808",
        "2002:808:808::1",
    ] {
        assert_eq!(local_address_scope(addr(text)), None, "{text}");
    }
}

#[test]
fn local_names_lead_to_this_computer_or_the_local_network() {
    for name in ["localhost", "LOCALHOST.", "app.localhost"] {
        assert_eq!(
            local_host_name_scope(name),
            Some(LocalScope::ThisComputer),
            "{name}"
        );
    }
    for name in ["nas.local", "Printer.LOCAL.", "db.internal"] {
        assert_eq!(
            local_host_name_scope(name),
            Some(LocalScope::LocalNetwork),
            "{name}"
        );
    }
    assert_eq!(local_host_name_scope("example.com"), None);
}

#[test]
fn a_url_host_leads_where_its_name_or_literal_does() {
    let scope = |reference: &str| {
        let url = remote_image_url(reference).unwrap();
        local_host_scope(&url.host().unwrap())
    };
    assert_eq!(
        scope("http://localhost:8080/a.png"),
        Some(LocalScope::ThisComputer)
    );
    assert_eq!(scope("http://[::1]/a.png"), Some(LocalScope::ThisComputer));
    assert_eq!(
        scope("http://2130706433/a.png"),
        Some(LocalScope::ThisComputer)
    );
    assert_eq!(
        scope("http://nas.local/a.png"),
        Some(LocalScope::LocalNetwork)
    );
    assert_eq!(
        scope("http://[fd00::5]/a.png"),
        Some(LocalScope::LocalNetwork)
    );
    assert_eq!(scope("https://example.com/a.png"), None);
}

fn addr(text: &str) -> IpAddr {
    text.parse().unwrap()
}

fn assert_local(addresses: &[&str]) {
    for text in addresses {
        assert!(is_local_address(addr(text)), "{text} should be local");
    }
}

fn assert_public(addresses: &[&str]) {
    for text in addresses {
        assert!(!is_local_address(addr(text)), "{text} should be public");
    }
}

#[test]
fn loopback_addresses_are_local() {
    assert_local(&["127.0.0.1", "127.255.255.254", "::1", "::ffff:127.0.0.1"]);
    assert_public(&["128.0.0.1", "126.255.255.255"]);
}

#[test]
fn unspecified_addresses_are_local() {
    assert_local(&["0.0.0.0", "0.1.2.3", "::", "::ffff:0.0.0.0"]);
    assert_public(&["1.0.0.0"]);
}

#[test]
fn private_ipv4_ranges_are_local_up_to_their_edges() {
    assert_local(&[
        "10.0.0.0",
        "10.255.255.255",
        "172.16.0.0",
        "172.31.255.255",
        "192.168.0.0",
        "192.168.255.255",
    ]);
    assert_public(&[
        "9.255.255.255",
        "11.0.0.0",
        "172.15.255.255",
        "172.32.0.0",
        "192.169.0.0",
    ]);
}

#[test]
fn the_shared_cgnat_range_is_local_up_to_its_edges() {
    assert_local(&["100.64.0.0", "100.127.255.255"]);
    assert_public(&["100.63.255.255", "100.128.0.0"]);
}

#[test]
fn link_local_addresses_are_local() {
    assert_local(&["169.254.0.1", "169.254.169.254", "fe80::1", "febf::ffff"]);
    assert_public(&["169.253.255.255", "169.255.0.0", "fec0::1"]);
}

#[test]
fn ipv6_unique_local_addresses_are_local() {
    assert_local(&["fc00::1", "fd12:3456::1", "fdff:ffff::1"]);
    assert_public(&["fbff::1", "fe00::1"]);
}

#[test]
fn ipv4_mapped_ipv6_is_judged_by_its_ipv4_address() {
    assert_local(&[
        "::ffff:10.1.2.3",
        "::ffff:192.168.0.9",
        "::ffff:100.64.0.1",
        "::ffff:169.254.1.1",
        "::127.0.0.1",
        "::10.0.0.1",
    ]);
    assert_public(&["::ffff:8.8.8.8"]);
}

#[test]
fn the_local_use_nat64_prefix_is_local_up_to_its_edges() {
    assert_local(&[
        "64:ff9b:1::",
        "64:ff9b:1::808:808",
        "64:ff9b:1:ffff:ffff:ffff:ffff:ffff",
    ]);
    assert_public(&["64:ff9b:2::", "64:ff9b:0:ffff:ffff:ffff:ffff:ffff"]);
}

#[test]
fn nat64_addresses_are_judged_by_their_ipv4_address() {
    assert_local(&[
        "64:ff9b::10.0.0.1",
        "64:ff9b::127.0.0.1",
        "64:ff9b::192.168.1.1",
        "64:ff9b::100.64.0.1",
        "64:ff9b::169.254.169.254",
        "64:ff9b::0.0.0.0",
    ]);
    assert_public(&["64:ff9b::8.8.8.8", "64:ff9b::93.184.216.34"]);
}

#[test]
fn six_to_four_addresses_are_judged_by_their_ipv4_address() {
    assert_local(&[
        "2002:a00:1::1",
        "2002:7f00:1::",
        "2002:c0a8:101:ffff::1",
        "2002:6440:1::1",
        "2002:a9fe:a9fe::1",
        "2002::1",
    ]);
    assert_public(&["2002:808:808::1", "2002:5db8:d822::1"]);
}

#[test]
fn the_embedded_ipv4_address_is_read_from_each_transition_form() {
    let v4 = |text: &str| Some(text.parse::<std::net::Ipv4Addr>().unwrap());
    let v6 = |text: &str| text.parse::<std::net::Ipv6Addr>().unwrap();
    assert_eq!(embedded_ipv4(v6("::ffff:10.1.2.3")), v4("10.1.2.3"));
    assert_eq!(embedded_ipv4(v6("::10.1.2.3")), v4("10.1.2.3"));
    assert_eq!(embedded_ipv4(v6("64:ff9b::c0a8:101")), v4("192.168.1.1"));
    assert_eq!(embedded_ipv4(v6("2002:c0a8:101:5::9")), v4("192.168.1.1"));
    assert_eq!(embedded_ipv4(v6("64:ff9b:0:1::c0a8:101")), None);
    assert_eq!(embedded_ipv4(v6("2001:db8::1")), None);
    assert_eq!(embedded_ipv4(v6("fd00::1")), None);
}

#[test]
fn public_addresses_are_not_local() {
    assert_public(&[
        "8.8.8.8",
        "1.1.1.1",
        "93.184.216.34",
        "2606:4700::1111",
        "2001:4860:4860::8888",
    ]);
}

#[test]
fn local_names_are_known_by_convention() {
    for name in [
        "localhost",
        "LOCALHOST",
        "localhost.",
        "app.localhost",
        "nas.local",
        "Printer.LOCAL.",
        "db.internal",
        "metadata.google.internal",
    ] {
        assert!(is_local_host_name(name), "{name}");
    }
    for name in [
        "example.com",
        "localhost.example.com",
        "local",
        "internal",
        "mylocal",
        "notinternal",
        "local.example.com",
    ] {
        assert!(!is_local_host_name(name), "{name}");
    }
}

#[test]
fn a_url_host_is_local_by_name_or_address_literal() {
    for reference in [
        "http://localhost:8080/a.png",
        "http://127.0.0.1/a.png",
        "http://10.0.0.5/a.png",
        "http://[::1]/a.png",
        "http://[fd00::5]/a.png",
        "http://[::ffff:192.168.1.1]/a.png",
        "http://0.0.0.0/a.png",
        "http://nas.local/a.png",
        "https://db.internal/a.png",
        "http://2130706433/a.png",
    ] {
        let url = remote_image_url(reference).unwrap();
        assert!(is_local_host(&url), "{reference}");
    }
    for reference in [
        "https://example.com/a.png",
        "https://8.8.8.8/a.png",
        "https://[2606:4700::1111]/a.png",
    ] {
        let url = remote_image_url(reference).unwrap();
        assert!(!is_local_host(&url), "{reference}");
    }
}

#[test]
fn a_load_names_who_started_it() {
    assert_eq!(
        serde_json::from_str::<RemoteImageLoad>("\"asked\"").unwrap(),
        RemoteImageLoad::Asked
    );
    assert_eq!(
        serde_json::from_str::<RemoteImageLoad>("\"automatic\"").unwrap(),
        RemoteImageLoad::Automatic
    );
    assert!(serde_json::from_str::<RemoteImageLoad>("\"other\"").is_err());
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

#[test]
fn a_closed_tab_loses_its_images_and_the_last_reader_frees_them() {
    use writ_core::preview::remote_image::RemoteImageStore;
    let mut store = RemoteImageStore::with_capacity(100);
    store.insert("shared", "buf-1", held(b"one"));
    store.grant("shared", "buf-2");
    store.insert("own", "buf-1", held(b"three"));
    store.release_buffer("buf-1");
    assert_eq!(store.read("shared", "buf-1"), None);
    assert_eq!(store.read("own", "buf-1"), None);
    assert_eq!(store.read("shared", "buf-2"), Some(held(b"one")));
    assert_eq!(store.held_bytes(), 3);
    store.release_buffer("buf-2");
    assert_eq!(store.grant("shared", "buf-3"), None);
    assert_eq!(store.held_bytes(), 0);
}
