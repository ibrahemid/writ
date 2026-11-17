//! Fetching a remote image the editor was asked to show (ADR-057).
//!
//! One GET per request, made only when the person asks for the image or has
//! turned on loading remote images. No cookies are kept and no referrer is
//! sent; the body is capped while it streams and typed by its leading bytes.
//!
//! Each request gets its own client, whose redirect policy and resolver know
//! the host the placeholder named. A redirect to a local address at any other
//! host ends the load, a click included. A load the person did not ask for
//! never reaches a local host either: local names and address literals are
//! refused before any request, the named host is resolved once and refused
//! when any of its addresses is local, and the connection goes to the
//! checked addresses with no proxy in between.

use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::redirect::{Attempt, Policy};
use tracing::warn;
use url::{Host, Url};
use writ_core::preview::remote_image::{
    check_remote_image_bytes, check_remote_image_size, is_followable_redirect, is_local_host,
    local_address_scope, local_host_name_scope, local_host_scope, HeldImage, LocalScope,
    RemoteImageLoad, RemoteImageRefusal, REMOTE_IMAGE_MAX_REDIRECTS, REMOTE_IMAGE_TIMEOUT,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const USER_AGENT: &str = concat!("Writ/", env!("CARGO_PKG_VERSION"));

/// Why a request ended before it reached a local address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
enum LocalRefusal {
    #[error("an image load nobody asked for names a local host")]
    LocalHost,
    #[error("an image load nobody asked for names a host that resolves to a local address")]
    ResolvesToLocal(LocalScope),
    #[error("an image redirects to a local address at another host")]
    RedirectsToLocal(LocalScope),
}

impl From<LocalRefusal> for RemoteImageRefusal {
    fn from(refusal: LocalRefusal) -> Self {
        match refusal {
            LocalRefusal::LocalHost => Self::LocalAddress,
            LocalRefusal::ResolvesToLocal(scope) => Self::ResolvesToLocal(scope),
            LocalRefusal::RedirectsToLocal(scope) => Self::RedirectsToLocal(scope),
        }
    }
}

fn host_key(host: &str) -> String {
    host.strip_suffix('.').unwrap_or(host).to_ascii_lowercase()
}

fn is_same_host(a: &Url, b: &Url) -> bool {
    a.host_str().map(host_key) == b.host_str().map(host_key)
}

/// The names a request's redirects led to, other than the named host.
#[derive(Debug, Default)]
struct RedirectHosts(Mutex<HashSet<String>>);

impl RedirectHosts {
    fn insert(&self, name: &str) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(host_key(name));
    }

    fn contains(&self, name: &str) -> bool {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(&host_key(name))
    }
}

/// The name a redirect hop leads to when it is not the named host. An
/// address literal is judged by the redirect policy alone.
fn redirected_host_name<'a>(origin: &Url, next: &'a Url) -> Option<&'a str> {
    match next.host() {
        Some(Host::Domain(name)) if !is_same_host(origin, next) => Some(name),
        _ => None,
    }
}

/// What a client does with the next hop of a redirect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RedirectStep {
    Follow,
    Stop,
    TooMany,
    Refuse(LocalRefusal),
}

fn redirect_step(origin: &Url, next: &Url, hops: usize, load: RemoteImageLoad) -> RedirectStep {
    if hops > REMOTE_IMAGE_MAX_REDIRECTS {
        RedirectStep::TooMany
    } else if !is_followable_redirect(next) {
        RedirectStep::Stop
    } else {
        match next.host().as_ref().and_then(local_host_scope) {
            None => RedirectStep::Follow,
            Some(scope) if !is_same_host(origin, next) => {
                RedirectStep::Refuse(LocalRefusal::RedirectsToLocal(scope))
            }
            Some(_) if load == RemoteImageLoad::Automatic => {
                RedirectStep::Refuse(LocalRefusal::LocalHost)
            }
            Some(_) => RedirectStep::Follow,
        }
    }
}

fn redirect_policy(load: RemoteImageLoad, redirect_hosts: Arc<RedirectHosts>) -> Policy {
    Policy::custom(move |attempt: Attempt| {
        let next = attempt.url();
        let origin = attempt.previous().first().unwrap_or(next);
        let step = redirect_step(origin, next, attempt.previous().len(), load);
        if step == RedirectStep::Follow {
            if let Some(name) = redirected_host_name(origin, next) {
                redirect_hosts.insert(name);
            }
        }
        match step {
            RedirectStep::Follow => attempt.follow(),
            RedirectStep::Stop => attempt.stop(),
            RedirectStep::TooMany => attempt.error("too many redirects"),
            RedirectStep::Refuse(refusal) => attempt.error(refusal),
        }
    })
}

/// What the resolver lets one name resolve to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NameCheck {
    /// The named host, on a click, or a proxy: every answer is dialed.
    Open,
    /// The named host, on a load nobody asked for.
    Unasked,
    /// A host a redirect led to.
    Redirected,
}

impl NameCheck {
    fn for_name(name: &str, load: RemoteImageLoad, redirect_hosts: &RedirectHosts) -> Self {
        if redirect_hosts.contains(name) {
            Self::Redirected
        } else if load == RemoteImageLoad::Automatic {
            Self::Unasked
        } else {
            Self::Open
        }
    }

    fn check_name(self, name: &str) -> Result<(), LocalRefusal> {
        match (self, local_host_name_scope(name)) {
            (Self::Open, _) | (_, None) => Ok(()),
            (Self::Unasked, Some(_)) => Err(LocalRefusal::LocalHost),
            (Self::Redirected, Some(scope)) => Err(LocalRefusal::RedirectsToLocal(scope)),
        }
    }

    /// Hands the connector a name's addresses when none of them is refused.
    /// A refusal names where the first local address leads.
    fn check_addresses(self, addresses: Vec<SocketAddr>) -> Result<Vec<SocketAddr>, LocalRefusal> {
        let scope = addresses
            .iter()
            .find_map(|address| local_address_scope(address.ip()));
        match (self, scope) {
            (Self::Open, _) | (_, None) => Ok(addresses),
            (Self::Unasked, Some(scope)) => Err(LocalRefusal::ResolvesToLocal(scope)),
            (Self::Redirected, Some(scope)) => Err(LocalRefusal::RedirectsToLocal(scope)),
        }
    }
}

/// One request's resolver. The connector dials exactly the addresses
/// returned here, so a check holds for the connection made.
struct CheckedResolver {
    load: RemoteImageLoad,
    redirect_hosts: Arc<RedirectHosts>,
}

impl Resolve for CheckedResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_owned();
        let check = NameCheck::for_name(&host, self.load, &self.redirect_hosts);
        Box::pin(async move {
            check.check_name(&host)?;
            let addresses = tokio::net::lookup_host((host.as_str(), 0)).await?.collect();
            let addresses = check.check_addresses(addresses)?;
            let addrs: Addrs = Box::new(addresses.into_iter());
            Ok(addrs)
        })
    }
}

/// A client for one request. A click goes through any system proxy, which
/// resolves the names it is sent; a load nobody asked for goes direct.
fn remote_image_client(load: RemoteImageLoad) -> Result<reqwest::Client, RemoteImageRefusal> {
    writ_ai::connection::ensure_crypto_provider();
    let redirect_hosts = Arc::new(RedirectHosts::default());
    let builder = reqwest::Client::builder()
        .redirect(redirect_policy(load, Arc::clone(&redirect_hosts)))
        .dns_resolver(Arc::new(CheckedResolver {
            load,
            redirect_hosts,
        }))
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REMOTE_IMAGE_TIMEOUT)
        .user_agent(USER_AGENT)
        .referer(false);
    let builder = match load {
        RemoteImageLoad::Asked => builder,
        RemoteImageLoad::Automatic => builder.no_proxy(),
    };
    builder.build().map_err(|error| {
        warn!(%error, ?load, "remote image client could not be built");
        RemoteImageRefusal::Unreachable
    })
}

fn local_refusal(error: &reqwest::Error) -> Option<LocalRefusal> {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(current) = source {
        if let Some(refusal) = current.downcast_ref::<LocalRefusal>() {
            return Some(*refusal);
        }
        source = current.source();
    }
    None
}

fn request_refusal(url: &Url, error: &reqwest::Error) -> RemoteImageRefusal {
    if let Some(refusal) = local_refusal(error) {
        warn!(host = url.host_str(), %refusal, "remote image load refused a local address");
        refusal.into()
    } else {
        warn!(host = url.host_str(), %error, "remote image request failed");
        RemoteImageRefusal::Unreachable
    }
}

/// Fetch the image at `url`.
///
/// # Errors
///
/// [`RemoteImageRefusal::LocalAddress`] when `load` is
/// [`RemoteImageLoad::Automatic`] and the host is a local name or address;
/// [`RemoteImageRefusal::ResolvesToLocal`] when `load` is
/// [`RemoteImageLoad::Automatic`] and the host resolves to a local address;
/// [`RemoteImageRefusal::RedirectsToLocal`] for any `load` when a redirect
/// leads to a local address at another host;
/// [`RemoteImageRefusal::Unreachable`] when the request fails, times out, is
/// redirected off the web or answers with anything but success;
/// [`RemoteImageRefusal::TooLarge`] when the body passes the cap, decided
/// from the declared length or while it streams;
/// [`RemoteImageRefusal::NotAnImage`] when its bytes are not an image the
/// editor draws.
pub async fn fetch_remote_image(
    url: &Url,
    load: RemoteImageLoad,
) -> Result<HeldImage, RemoteImageRefusal> {
    if load == RemoteImageLoad::Automatic && is_local_host(url) {
        warn!(
            host = url.host_str(),
            "automatic remote image load refused a local host"
        );
        return Err(RemoteImageRefusal::LocalAddress);
    }
    let client = remote_image_client(load)?;
    let mut response = client
        .get(url.clone())
        .send()
        .await
        .map_err(|error| request_refusal(url, &error))?;
    if !response.status().is_success() {
        warn!(
            host = url.host_str(),
            status = response.status().as_u16(),
            "remote image answered without success"
        );
        return Err(RemoteImageRefusal::Unreachable);
    }
    if let Some(declared) = response.content_length() {
        check_remote_image_size(declared)?;
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| {
        warn!(host = url.host_str(), %error, "remote image body failed");
        RemoteImageRefusal::Unreachable
    })? {
        body.extend_from_slice(&chunk);
        check_remote_image_size(body.len() as u64)?;
    }
    let mime = check_remote_image_bytes(&body)?;
    Ok(HeldImage {
        mime,
        bytes: body.into(),
    })
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread::JoinHandle;

    use writ_core::preview::remote_image::REMOTE_IMAGE_MAX_BYTES;

    use super::*;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";

    /// Serves each canned response to one connection, in order, then stops.
    fn serve(responses: Vec<Vec<u8>>) -> (String, JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for response in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = Vec::new();
                let mut buf = [0u8; 1024];
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    let n = stream.read(&mut buf).unwrap();
                    if n == 0 {
                        break;
                    }
                    request.extend_from_slice(&buf[..n]);
                }
                requests.push(String::from_utf8_lossy(&request).into_owned());
                let _ = stream.write_all(&response);
            }
            requests
        });
        (base, handle)
    }

    fn reply(status: &str, headers: &str, body: &[u8]) -> Vec<u8> {
        let mut out = format!(
            "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        out.extend_from_slice(body);
        out
    }

    fn fetch(url: &str) -> Result<HeldImage, RemoteImageRefusal> {
        let url = Url::parse(url).unwrap();
        tauri::async_runtime::block_on(fetch_remote_image(&url, RemoteImageLoad::Asked))
    }

    fn fetch_automatic(url: &str) -> Result<HeldImage, RemoteImageRefusal> {
        let url = Url::parse(url).unwrap();
        tauri::async_runtime::block_on(fetch_remote_image(&url, RemoteImageLoad::Automatic))
    }

    /// Sends through an automatic client without the host check
    /// [`fetch_remote_image`] makes first, so the resolver and the redirect
    /// policy are what refuse.
    fn send_automatic(url: &str) -> Result<reqwest::StatusCode, RemoteImageRefusal> {
        let url = Url::parse(url).unwrap();
        let client = remote_image_client(RemoteImageLoad::Automatic).unwrap();
        tauri::async_runtime::block_on(async {
            client
                .get(url.clone())
                .send()
                .await
                .map(|response| response.status())
                .map_err(|error| request_refusal(&url, &error))
        })
    }

    #[test]
    fn an_image_is_fetched_and_typed_by_its_bytes() {
        let (base, server) = serve(vec![reply("200 OK", "Content-Type: text/plain\r\n", PNG)]);
        let image = fetch(&format!("{base}/a.png")).unwrap();
        assert_eq!(image.mime, "image/png");
        assert_eq!(image.bytes.as_ref(), PNG);
        let requests = server.join().unwrap();
        assert!(requests[0].starts_with("GET /a.png "));
        let lower = requests[0].to_ascii_lowercase();
        assert!(lower.contains("user-agent: writ/"));
        assert!(!lower.contains("cookie:"));
        assert!(!lower.contains("referer:"));
    }

    #[test]
    fn a_missing_image_is_unreachable() {
        let (base, server) = serve(vec![reply("404 Not Found", "", b"gone")]);
        assert_eq!(
            fetch(&format!("{base}/a.png")),
            Err(RemoteImageRefusal::Unreachable)
        );
        server.join().unwrap();
    }

    #[test]
    fn a_page_is_not_an_image() {
        let (base, server) = serve(vec![reply(
            "200 OK",
            "Content-Type: image/png\r\n",
            b"<!doctype html><p>hi</p>",
        )]);
        assert_eq!(
            fetch(&format!("{base}/a.png")),
            Err(RemoteImageRefusal::NotAnImage)
        );
        server.join().unwrap();
    }

    #[test]
    fn a_declared_length_over_the_cap_is_too_large_before_the_body() {
        let declared = REMOTE_IMAGE_MAX_BYTES + 1;
        let head =
            format!("HTTP/1.1 200 OK\r\nContent-Length: {declared}\r\nConnection: close\r\n\r\n");
        let mut response = head.into_bytes();
        response.extend_from_slice(PNG);
        let (base, server) = serve(vec![response]);
        assert_eq!(
            fetch(&format!("{base}/a.png")),
            Err(RemoteImageRefusal::TooLarge)
        );
        server.join().unwrap();
    }

    #[test]
    fn a_redirect_to_another_web_address_is_followed() {
        let (target, target_server) = serve(vec![reply("200 OK", "", PNG)]);
        let (base_redirect, redirect_server) = serve(vec![reply(
            "302 Found",
            &format!("Location: {target}/b.png\r\n"),
            b"",
        )]);
        let image = fetch(&format!("{base_redirect}/a.png")).unwrap();
        assert_eq!(image.mime, "image/png");
        redirect_server.join().unwrap();
        let requests = target_server.join().unwrap();
        assert!(requests[0].starts_with("GET /b.png "));
        assert!(!requests[0].to_ascii_lowercase().contains("referer:"));
    }

    #[test]
    fn a_redirect_off_the_web_is_not_followed() {
        let (base, server) = serve(vec![reply(
            "302 Found",
            "Location: file:///etc/passwd\r\n",
            b"",
        )]);
        assert_eq!(
            fetch(&format!("{base}/a.png")),
            Err(RemoteImageRefusal::Unreachable)
        );
        server.join().unwrap();
    }

    #[test]
    fn an_automatic_load_of_a_local_address_never_connects() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        for url in [
            format!("http://127.0.0.1:{port}/a.png"),
            format!("http://localhost:{port}/a.png"),
            format!("http://[::ffff:127.0.0.1]:{port}/a.png"),
            "http://192.168.1.1/a.png".to_owned(),
            "http://nas.local/a.png".to_owned(),
        ] {
            assert_eq!(
                fetch_automatic(&url),
                Err(RemoteImageRefusal::LocalAddress),
                "{url}"
            );
        }
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }

    fn resolver(load: RemoteImageLoad, redirected: &[&str]) -> CheckedResolver {
        let redirect_hosts = RedirectHosts::default();
        for name in redirected {
            redirect_hosts.insert(name);
        }
        CheckedResolver {
            load,
            redirect_hosts: Arc::new(redirect_hosts),
        }
    }

    fn resolve_refusal(resolver: &CheckedResolver, name: &str) -> Option<LocalRefusal> {
        let resolved = tauri::async_runtime::block_on(resolver.resolve(name.parse().unwrap()));
        resolved
            .err()
            .map(|error| *error.downcast_ref::<LocalRefusal>().unwrap())
    }

    #[test]
    fn the_automatic_resolver_refuses_a_local_name_before_looking_it_up() {
        let automatic = resolver(RemoteImageLoad::Automatic, &[]);
        for name in ["localhost", "printer.local", "db.internal", "app.localhost"] {
            assert_eq!(
                resolve_refusal(&automatic, name),
                Some(LocalRefusal::LocalHost),
                "{name}"
            );
        }
    }

    #[test]
    fn a_click_resolves_the_named_host_even_when_it_is_local() {
        assert_eq!(
            resolve_refusal(&resolver(RemoteImageLoad::Asked, &[]), "localhost"),
            None
        );
    }

    #[test]
    fn a_local_name_a_redirect_led_to_is_refused_on_a_click() {
        for load in [RemoteImageLoad::Asked, RemoteImageLoad::Automatic] {
            let redirected = resolver(load, &["localhost", "printer.local"]);
            for (name, scope) in [
                ("localhost", LocalScope::ThisComputer),
                ("LOCALHOST.", LocalScope::ThisComputer),
                ("printer.local", LocalScope::LocalNetwork),
            ] {
                assert_eq!(
                    resolve_refusal(&redirected, name),
                    Some(LocalRefusal::RedirectsToLocal(scope)),
                    "{name} {load:?}"
                );
            }
        }
    }

    #[test]
    fn only_a_host_other_than_the_named_one_is_a_redirect_target() {
        let origin = Url::parse("https://images.example.com/a.png").unwrap();
        let elsewhere = Url::parse("https://CDN.example.net./b.png").unwrap();
        let same = Url::parse("http://images.example.com:8080/b.png").unwrap();
        let literal = Url::parse("https://93.184.216.34/b.png").unwrap();
        assert_eq!(
            redirected_host_name(&origin, &elsewhere),
            Some("cdn.example.net.")
        );
        assert_eq!(redirected_host_name(&origin, &same), None);
        assert_eq!(redirected_host_name(&origin, &literal), None);
        let hosts = RedirectHosts::default();
        hosts.insert("cdn.example.net.");
        assert!(hosts.contains("CDN.Example.net"));
        assert!(!hosts.contains("images.example.com"));
    }

    #[test]
    fn a_refused_name_surfaces_as_a_local_address_refusal() {
        assert_eq!(
            send_automatic("http://localhost:9/a.png"),
            Err(RemoteImageRefusal::LocalAddress)
        );
    }

    #[test]
    fn a_name_with_any_local_address_is_refused() {
        let public: SocketAddr = "93.184.216.34:0".parse().unwrap();
        let public_v6: SocketAddr = "[2606:4700::1111]:0".parse().unwrap();
        for check in [NameCheck::Open, NameCheck::Unasked, NameCheck::Redirected] {
            assert_eq!(
                check.check_addresses(vec![public, public_v6]).unwrap(),
                vec![public, public_v6]
            );
        }
        let this_computer = LocalScope::ThisComputer;
        let network = LocalScope::LocalNetwork;
        for (local, scope) in [
            ("127.0.0.1:0", this_computer),
            ("10.0.0.1:0", network),
            ("172.16.0.1:0", network),
            ("192.168.0.1:0", network),
            ("100.64.0.1:0", network),
            ("169.254.169.254:0", network),
            ("0.0.0.0:0", this_computer),
            ("[::1]:0", this_computer),
            ("[::]:0", this_computer),
            ("[fd00::1]:0", network),
            ("[fe80::1]:0", network),
            ("[::ffff:10.0.0.1]:0", network),
        ] {
            let local: SocketAddr = local.parse().unwrap();
            assert_eq!(
                NameCheck::Unasked.check_addresses(vec![public, local]),
                Err(LocalRefusal::ResolvesToLocal(scope)),
                "{local}"
            );
            assert_eq!(
                NameCheck::Redirected.check_addresses(vec![public, local]),
                Err(LocalRefusal::RedirectsToLocal(scope)),
                "{local}"
            );
            assert_eq!(
                NameCheck::Open.check_addresses(vec![public, local]),
                Ok(vec![public, local])
            );
        }
    }

    #[test]
    fn a_refusal_names_where_the_first_local_address_leads() {
        let loopback: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let private: SocketAddr = "10.0.0.1:0".parse().unwrap();
        assert_eq!(
            NameCheck::Unasked.check_addresses(vec![loopback, private]),
            Err(LocalRefusal::ResolvesToLocal(LocalScope::ThisComputer))
        );
        assert_eq!(
            NameCheck::Unasked.check_addresses(vec![private, loopback]),
            Err(LocalRefusal::ResolvesToLocal(LocalScope::LocalNetwork))
        );
    }

    #[test]
    fn an_automatic_redirect_to_a_local_host_ends_the_load() {
        let (base, server) = serve(vec![reply(
            "302 Found",
            "Location: http://127.0.0.1:9/b.png\r\n",
            b"",
        )]);
        assert_eq!(
            send_automatic(&format!("{base}/a.png")),
            Err(RemoteImageRefusal::LocalAddress)
        );
        assert_eq!(server.join().unwrap().len(), 1);
    }

    #[test]
    fn an_automatic_redirect_to_another_local_host_is_a_redirect_refusal() {
        let (base, server) = serve(vec![reply(
            "302 Found",
            "Location: http://192.168.1.1/b.png\r\n",
            b"",
        )]);
        assert_eq!(
            send_automatic(&format!("{base}/a.png")),
            Err(RemoteImageRefusal::RedirectsToLocal(
                LocalScope::LocalNetwork
            ))
        );
        assert_eq!(server.join().unwrap().len(), 1);
    }

    #[test]
    fn a_click_refuses_a_redirect_to_a_local_address_at_another_host() {
        for (target, scope) in [
            ("http://localhost:9/b.png", LocalScope::ThisComputer),
            ("http://192.168.1.1/b.png", LocalScope::LocalNetwork),
            ("http://[::1]:9/b.png", LocalScope::ThisComputer),
            ("http://nas.local/b.png", LocalScope::LocalNetwork),
        ] {
            let (base, server) = serve(vec![reply(
                "302 Found",
                &format!("Location: {target}\r\n"),
                b"",
            )]);
            assert_eq!(
                fetch(&format!("{base}/a.png")),
                Err(RemoteImageRefusal::RedirectsToLocal(scope)),
                "{target}"
            );
            assert_eq!(server.join().unwrap().len(), 1, "{target}");
        }
    }

    #[test]
    fn a_click_loads_an_image_from_a_local_name() {
        let (base, server) = serve(vec![reply("200 OK", "", PNG)]);
        let port = Url::parse(&base).unwrap().port().unwrap();
        let image = fetch(&format!("http://localhost:{port}/a.png")).unwrap();
        assert_eq!(image.mime, "image/png");
        server.join().unwrap();
    }

    #[test]
    fn redirect_steps_depend_on_the_named_host_and_who_asked() {
        let public = Url::parse("https://images.example.com/a.png").unwrap();
        let cdn = Url::parse("https://cdn.example.net/a.png").unwrap();
        let nas = Url::parse("http://10.0.0.1/a.png").unwrap();
        let nas_port = Url::parse("http://10.0.0.1:8080/b.png").unwrap();
        let other_nas = Url::parse("http://10.0.0.2/b.png").unwrap();
        let named = Url::parse("http://nas.local/a.png").unwrap();
        let file = Url::parse("file:///etc/passwd").unwrap();
        let automatic = RemoteImageLoad::Automatic;
        let asked = RemoteImageLoad::Asked;
        let loopback = Url::parse("http://127.0.0.1/b.png").unwrap();
        let redirects =
            RedirectStep::Refuse(LocalRefusal::RedirectsToLocal(LocalScope::LocalNetwork));
        for load in [asked, automatic] {
            assert_eq!(redirect_step(&public, &cdn, 1, load), RedirectStep::Follow);
            assert_eq!(redirect_step(&public, &nas, 1, load), redirects);
            assert_eq!(redirect_step(&public, &named, 1, load), redirects);
            assert_eq!(redirect_step(&nas, &other_nas, 1, load), redirects);
            assert_eq!(
                redirect_step(&public, &loopback, 1, load),
                RedirectStep::Refuse(LocalRefusal::RedirectsToLocal(LocalScope::ThisComputer))
            );
            assert_eq!(redirect_step(&public, &file, 1, load), RedirectStep::Stop);
            assert_eq!(
                redirect_step(&public, &cdn, REMOTE_IMAGE_MAX_REDIRECTS + 1, load),
                RedirectStep::TooMany
            );
        }
        assert_eq!(
            redirect_step(&nas, &nas_port, 1, asked),
            RedirectStep::Follow
        );
        assert_eq!(
            redirect_step(&nas, &nas_port, 1, automatic),
            RedirectStep::Refuse(LocalRefusal::LocalHost)
        );
    }
}
