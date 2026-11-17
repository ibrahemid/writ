//! Fetching a remote image the editor was asked to show (ADR-057).
//!
//! One GET per request, made only when the person asks for the image or has
//! turned on loading remote images. No cookies are kept and no referrer is
//! sent; the body is capped while it streams and typed by its leading bytes.

use std::sync::OnceLock;
use std::time::Duration;

use tracing::warn;
use url::Url;
use writ_core::preview::remote_image::{
    check_remote_image_bytes, check_remote_image_size, is_followable_redirect, HeldImage,
    RemoteImageRefusal, REMOTE_IMAGE_MAX_REDIRECTS, REMOTE_IMAGE_TIMEOUT,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const USER_AGENT: &str = concat!("Writ/", env!("CARGO_PKG_VERSION"));

static CLIENT: OnceLock<Result<reqwest::Client, String>> = OnceLock::new();

fn remote_image_client() -> Result<&'static reqwest::Client, RemoteImageRefusal> {
    CLIENT
        .get_or_init(|| {
            writ_ai::connection::ensure_crypto_provider();
            reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::custom(|attempt| {
                    if attempt.previous().len() > REMOTE_IMAGE_MAX_REDIRECTS {
                        attempt.error("too many redirects")
                    } else if is_followable_redirect(attempt.url()) {
                        attempt.follow()
                    } else {
                        attempt.stop()
                    }
                }))
                .connect_timeout(CONNECT_TIMEOUT)
                .timeout(REMOTE_IMAGE_TIMEOUT)
                .user_agent(USER_AGENT)
                .build()
                .map_err(|e| e.to_string())
        })
        .as_ref()
        .map_err(|error| {
            warn!(%error, "remote image client could not be built");
            RemoteImageRefusal::Unreachable
        })
}

/// Fetch the image at `url`.
///
/// # Errors
///
/// [`RemoteImageRefusal::Unreachable`] when the request fails, times out, is
/// redirected off the web or answers with anything but success;
/// [`RemoteImageRefusal::TooLarge`] when the body passes the cap, decided
/// from the declared length or while it streams;
/// [`RemoteImageRefusal::NotAnImage`] when its bytes are not an image the
/// editor draws.
pub async fn fetch_remote_image(url: &Url) -> Result<HeldImage, RemoteImageRefusal> {
    let client = remote_image_client()?;
    let mut response = client.get(url.clone()).send().await.map_err(|error| {
        warn!(host = url.host_str(), %error, "remote image request failed");
        RemoteImageRefusal::Unreachable
    })?;
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
        tauri::async_runtime::block_on(fetch_remote_image(&url))
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
}
