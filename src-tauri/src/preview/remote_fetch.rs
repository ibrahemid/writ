//! Fetching a remote image the editor was asked to show (ADR-057).
//!
//! One GET per request, made only when the person asks for the image or has
//! turned on loading remote images. No cookies are kept and no referrer is
//! sent; the body is capped while it streams and typed by its leading bytes.
//!
//! A load the person did not ask for goes through a second client that never
//! reaches this machine or the local network: local names and address
//! literals are refused before any request, every name is resolved once and
//! refused when any of its addresses is local, the connection goes to the
//! checked addresses with no proxy in between, and a redirect to a local host
//! ends the load.

use std::net::SocketAddr;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::redirect::{Attempt, Policy};
use tracing::warn;
use url::Url;
use writ_core::preview::remote_image::{
    check_remote_image_bytes, check_remote_image_size, is_followable_redirect, is_local_address,
    is_local_host, is_local_host_name, HeldImage, RemoteImageLoad, RemoteImageRefusal,
    REMOTE_IMAGE_MAX_REDIRECTS, REMOTE_IMAGE_TIMEOUT,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const USER_AGENT: &str = concat!("Writ/", env!("CARGO_PKG_VERSION"));

static ASKED_CLIENT: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();
static AUTOMATIC_CLIENT: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();

/// An automatic load reached for a local name, a local address, or a name
/// that resolves to one.
#[derive(Debug, thiserror::Error)]
#[error("an image load nobody asked for would reach a local address")]
struct LocalAddressRefused;

/// What a client does with the next hop of a redirect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RedirectStep {
    Follow,
    Stop,
    TooMany,
    LocalAddress,
}

fn redirect_step(next: &Url, hops: usize, load: RemoteImageLoad) -> RedirectStep {
    if hops > REMOTE_IMAGE_MAX_REDIRECTS {
        RedirectStep::TooMany
    } else if !is_followable_redirect(next) {
        RedirectStep::Stop
    } else if load == RemoteImageLoad::Automatic && is_local_host(next) {
        RedirectStep::LocalAddress
    } else {
        RedirectStep::Follow
    }
}

fn redirect_policy(load: RemoteImageLoad) -> Policy {
    Policy::custom(move |attempt: Attempt| {
        match redirect_step(attempt.url(), attempt.previous().len(), load) {
            RedirectStep::Follow => attempt.follow(),
            RedirectStep::Stop => attempt.stop(),
            RedirectStep::TooMany => attempt.error("too many redirects"),
            RedirectStep::LocalAddress => attempt.error(LocalAddressRefused),
        }
    })
}

/// Hands the connector a name's addresses only when none of them is local.
fn check_resolved_addresses(
    addresses: Vec<SocketAddr>,
) -> Result<Vec<SocketAddr>, LocalAddressRefused> {
    if addresses
        .iter()
        .any(|address| is_local_address(address.ip()))
    {
        Err(LocalAddressRefused)
    } else {
        Ok(addresses)
    }
}

/// The automatic client's resolver. The connector dials exactly the
/// addresses returned here, so the check holds for the connection made.
struct PublicAddressResolver;

impl Resolve for PublicAddressResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_owned();
        Box::pin(async move {
            if is_local_host_name(&host) {
                return Err(LocalAddressRefused.into());
            }
            let addresses = tokio::net::lookup_host((host.as_str(), 0)).await?.collect();
            let addresses = check_resolved_addresses(addresses)?;
            let addrs: Addrs = Box::new(addresses.into_iter());
            Ok(addrs)
        })
    }
}

fn build_client(
    cell: &'static OnceLock<Result<reqwest::Client, String>>,
    load: RemoteImageLoad,
) -> Result<&'static reqwest::Client, RemoteImageRefusal> {
    cell.get_or_init(|| {
        writ_ai::connection::ensure_crypto_provider();
        let builder = reqwest::Client::builder()
            .redirect(redirect_policy(load))
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REMOTE_IMAGE_TIMEOUT)
            .user_agent(USER_AGENT)
            .referer(false);
        let builder = match load {
            RemoteImageLoad::Asked => builder,
            RemoteImageLoad::Automatic => builder
                .no_proxy()
                .dns_resolver(Arc::new(PublicAddressResolver)),
        };
        builder.build().map_err(|e| e.to_string())
    })
    .as_ref()
    .map_err(|error| {
        warn!(%error, ?load, "remote image client could not be built");
        RemoteImageRefusal::Unreachable
    })
}

fn remote_image_client(
    load: RemoteImageLoad,
) -> Result<&'static reqwest::Client, RemoteImageRefusal> {
    match load {
        RemoteImageLoad::Asked => build_client(&ASKED_CLIENT, load),
        RemoteImageLoad::Automatic => build_client(&AUTOMATIC_CLIENT, load),
    }
}

fn is_local_address_refusal(error: &reqwest::Error) -> bool {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(current) = source {
        if current.is::<LocalAddressRefused>() {
            return true;
        }
        source = current.source();
    }
    false
}

fn request_refusal(url: &Url, error: &reqwest::Error) -> RemoteImageRefusal {
    if is_local_address_refusal(error) {
        warn!(
            host = url.host_str(),
            "automatic remote image load reached a local address"
        );
        RemoteImageRefusal::LocalAddress
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
/// [`RemoteImageLoad::Automatic`] and the host, an address it resolves to,
/// or a redirect is on this machine or the local network;
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

    /// Sends through the automatic client without the host check
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

    #[test]
    fn the_automatic_resolver_refuses_a_local_name_before_looking_it_up() {
        for name in ["localhost", "printer.local", "db.internal", "app.localhost"] {
            let refused = tauri::async_runtime::block_on(
                PublicAddressResolver.resolve(name.parse().unwrap()),
            );
            let error = refused.err().unwrap_or_else(|| panic!("{name} resolved"));
            assert!(error.is::<LocalAddressRefused>(), "{name}: {error}");
        }
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
        assert_eq!(
            check_resolved_addresses(vec![public, public_v6]).unwrap(),
            vec![public, public_v6]
        );
        for local in [
            "127.0.0.1:0",
            "10.0.0.1:0",
            "172.16.0.1:0",
            "192.168.0.1:0",
            "100.64.0.1:0",
            "169.254.169.254:0",
            "0.0.0.0:0",
            "[::1]:0",
            "[::]:0",
            "[fd00::1]:0",
            "[fe80::1]:0",
            "[::ffff:10.0.0.1]:0",
        ] {
            let local: SocketAddr = local.parse().unwrap();
            assert!(
                check_resolved_addresses(vec![public, local]).is_err(),
                "{local}"
            );
        }
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
    fn redirect_steps_depend_on_who_asked() {
        let local = Url::parse("http://10.0.0.1/a.png").unwrap();
        let named = Url::parse("http://nas.local/a.png").unwrap();
        let public = Url::parse("https://cdn.example.com/a.png").unwrap();
        let file = Url::parse("file:///etc/passwd").unwrap();
        let automatic = RemoteImageLoad::Automatic;
        let asked = RemoteImageLoad::Asked;
        assert_eq!(redirect_step(&local, 1, asked), RedirectStep::Follow);
        assert_eq!(
            redirect_step(&local, 1, automatic),
            RedirectStep::LocalAddress
        );
        assert_eq!(
            redirect_step(&named, 1, automatic),
            RedirectStep::LocalAddress
        );
        assert_eq!(redirect_step(&public, 1, automatic), RedirectStep::Follow);
        assert_eq!(redirect_step(&file, 1, automatic), RedirectStep::Stop);
        assert_eq!(
            redirect_step(&public, REMOTE_IMAGE_MAX_REDIRECTS + 1, asked),
            RedirectStep::TooMany
        );
    }
}
