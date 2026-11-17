//! An `http:` or `https:` image in a Markdown file (ADR-057): which
//! references are remote, the host a placeholder names, what a fetch may
//! take, and the route the fetched bytes are served from.
//!
//! Nothing here touches the network. The fetch runs in the app, never in a
//! webview, and only after a click or the `editor.load_remote_images` switch.

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::Arc;
use std::time::Duration;

use url::{Host, Url};

use crate::hash::sha256_hex;
use crate::polish::is_private_network_address;
use crate::preview::protocol::{sniff_image_mime, ASSET_MAX_BYTES};

/// First segment of the remote-image route under the `document` scope.
pub const REMOTE_IMAGE_PREFIX: &str = "_remote-image";

/// The largest remote image fetched, the same cap as a file on disk.
pub const REMOTE_IMAGE_MAX_BYTES: u64 = ASSET_MAX_BYTES;

/// How long one fetch, redirects included, may take.
pub const REMOTE_IMAGE_TIMEOUT: Duration = Duration::from_secs(20);

/// How many redirects a fetch follows.
pub const REMOTE_IMAGE_MAX_REDIRECTS: usize = 5;

/// The most bytes of fetched images held at once. The least recently used
/// image goes first when a new one would pass it.
pub const REMOTE_IMAGE_STORE_BYTES: usize = 64 * 1024 * 1024;

/// Why a remote image is not drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteImageRefusal {
    /// The reference is not an `http:` or `https:` URL with a host.
    NotRemote,
    /// The host did not answer with the image: no connection, a timeout, or
    /// a status other than success.
    Unreachable,
    /// The answer is over [`REMOTE_IMAGE_MAX_BYTES`].
    TooLarge,
    /// The answer's leading bytes are not an image the editor draws.
    NotAnImage,
    /// A load the person did not ask for names a local host
    /// ([`is_local_host`]), so the placeholder stays until they click Load
    /// image.
    LocalAddress,
    /// A load the person did not ask for names a host that resolves to a
    /// local address ([`local_address_scope`]). The placeholder says where the
    /// address is and keeps Load image.
    ResolvesToLocal(LocalScope),
    /// A redirect leads to a local address at a host other than the one the
    /// placeholder named. Refused for every load, a click included.
    RedirectsToLocal(LocalScope),
}

/// Where a local name or address leads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalScope {
    /// This computer: a loopback or unspecified address, or `localhost`.
    ThisComputer,
    /// Another device on the local network: a private, shared, link-local or
    /// unique-local address, or an mDNS or `.internal` name.
    LocalNetwork,
}

/// Who started a remote image load.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RemoteImageLoad {
    /// The person clicked Load image or Try again.
    Asked,
    /// `editor.load_remote_images` loaded it with no click, so it must not
    /// reach this machine or the local network.
    Automatic,
}

impl RemoteImageRefusal {
    /// Stable identifier the editor maps to a caption.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotRemote => "not_remote",
            Self::Unreachable => "unreachable",
            Self::TooLarge => "too_large",
            Self::NotAnImage => "not_an_image",
            Self::LocalAddress => "local_address",
            Self::ResolvesToLocal(LocalScope::ThisComputer) => "resolves_to_this_computer",
            Self::ResolvesToLocal(LocalScope::LocalNetwork) => "resolves_to_local_network",
            Self::RedirectsToLocal(LocalScope::ThisComputer) => "redirects_to_this_computer",
            Self::RedirectsToLocal(LocalScope::LocalNetwork) => "redirects_to_local_network",
        }
    }
}

/// The URL a remote reference is fetched from: `http:` or `https:` with a
/// host, any user name and password removed so a document cannot make the
/// fetch send them, and the fragment dropped.
pub fn remote_image_url(reference: &str) -> Option<Url> {
    let trimmed = reference.trim();
    let scheme_end = trimmed.find(':')?;
    let scheme = &trimmed[..scheme_end];
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return None;
    }
    let mut url = Url::parse(trimmed).ok()?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none_or(str::is_empty) {
        return None;
    }
    url.set_username("").ok()?;
    url.set_password(None).ok()?;
    url.set_fragment(None);
    Some(url)
}

/// The host a placeholder names, with the port when the URL gives one.
pub fn remote_image_host(url: &Url) -> String {
    let host = url.host_str().unwrap_or_default();
    match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    }
}

/// The key a fetched image is held and served under: the SHA-256 of its URL.
pub fn remote_image_key(url: &Url) -> String {
    sha256_hex(url.as_str().as_bytes())
}

/// Whether a redirect target may be followed: `http:` or `https:` with a
/// host, as the first URL had to be.
pub fn is_followable_redirect(target: &Url) -> bool {
    matches!(target.scheme(), "http" | "https") && target.host_str().is_some_and(|h| !h.is_empty())
}

/// The IPv4 address an IPv6 address carries for a transition mechanism:
/// IPv4-mapped (`::ffff:0:0/96`), IPv4-compatible (`::/96`), NAT64
/// (`64:ff9b::/96`, the last 32 bits) and 6to4 (`2002::/16`, bits 16 to 48).
pub fn embedded_ipv4(v6: Ipv6Addr) -> Option<Ipv4Addr> {
    if let Some(v4) = v6.to_ipv4() {
        return Some(v4);
    }
    let joined = |high: u16, low: u16| Ipv4Addr::from((u32::from(high) << 16) | u32::from(low));
    match v6.segments() {
        [0x64, 0xff9b, 0, 0, 0, 0, high, low] | [0x2002, high, low, ..] => Some(joined(high, low)),
        _ => None,
    }
}

/// Where an address in a range reserved for this machine or a local network
/// leads, or `None` for any other address.
///
/// This computer: loopback (127/8, `::1`) and unspecified (0/8, `::`),
/// including their IPv4-mapped and IPv4-compatible forms. The local network:
/// the private, shared (100.64/10) and link-local ranges of
/// [`is_private_network_address`], IPv6 unique local and link-local, the
/// local-use NAT64 prefix `64:ff9b:1::/48`, and a NAT64 or 6to4 address whose
/// [`embedded_ipv4`] is local, which a gateway on the network translates. The
/// test is by range: a device on the local network that holds a public
/// address is not recognized.
pub fn local_address_scope(addr: IpAddr) -> Option<LocalScope> {
    let (v4, is_translated) = match addr {
        IpAddr::V4(v4) => (v4, false),
        IpAddr::V6(v6) if matches!(v6.segments(), [0x64, 0xff9b, 1, ..]) => {
            return Some(LocalScope::LocalNetwork);
        }
        IpAddr::V6(v6) => match embedded_ipv4(v6) {
            Some(v4) => (v4, v6.to_ipv4().is_none()),
            None => return is_private_network_address(addr).then_some(LocalScope::LocalNetwork),
        },
    };
    if v4.is_loopback() || v4.octets()[0] == 0 {
        return Some(if is_translated {
            LocalScope::LocalNetwork
        } else {
            LocalScope::ThisComputer
        });
    }
    is_private_network_address(IpAddr::V4(v4)).then_some(LocalScope::LocalNetwork)
}

/// Whether an address is in a range reserved for this machine or a local
/// network ([`local_address_scope`]).
pub fn is_local_address(addr: IpAddr) -> bool {
    local_address_scope(addr).is_some()
}

/// Where a host name leads by convention, in any case and with or without the
/// trailing dot: `localhost` and names under `.localhost` to this computer,
/// names under `.local` (mDNS) or `.internal` to the local network.
pub fn local_host_name_scope(name: &str) -> Option<LocalScope> {
    let name = name.strip_suffix('.').unwrap_or(name).to_ascii_lowercase();
    if name == "localhost" || name.ends_with(".localhost") {
        Some(LocalScope::ThisComputer)
    } else if [".local", ".internal"]
        .iter()
        .any(|suffix| name.ends_with(suffix))
    {
        Some(LocalScope::LocalNetwork)
    } else {
        None
    }
}

/// Whether a host name names this machine or the local network by
/// convention ([`local_host_name_scope`]).
pub fn is_local_host_name(name: &str) -> bool {
    local_host_name_scope(name).is_some()
}

/// Where a URL host that is a local name or a local address literal leads.
pub fn local_host_scope(host: &Host<&str>) -> Option<LocalScope> {
    match *host {
        Host::Domain(name) => local_host_name_scope(name),
        Host::Ipv4(v4) => local_address_scope(IpAddr::V4(v4)),
        Host::Ipv6(v6) => local_address_scope(IpAddr::V6(v6)),
    }
}

/// Whether a URL's host is a local name or a local address literal. A URL
/// with no host counts as local. A public name still has to resolve to
/// public addresses before an automatic load connects to it.
pub fn is_local_host(url: &Url) -> bool {
    url.host()
        .is_none_or(|host| local_host_scope(&host).is_some())
}

/// Turns away a body announced or grown past [`REMOTE_IMAGE_MAX_BYTES`].
///
/// # Errors
///
/// [`RemoteImageRefusal::TooLarge`].
pub fn check_remote_image_size(byte_len: u64) -> Result<(), RemoteImageRefusal> {
    if byte_len > REMOTE_IMAGE_MAX_BYTES {
        return Err(RemoteImageRefusal::TooLarge);
    }
    Ok(())
}

/// The MIME type of a fetched body, from its leading bytes. What the host
/// says the type is plays no part.
///
/// # Errors
///
/// [`RemoteImageRefusal::NotAnImage`].
pub fn check_remote_image_bytes(bytes: &[u8]) -> Result<&'static str, RemoteImageRefusal> {
    sniff_image_mime(bytes).ok_or(RemoteImageRefusal::NotAnImage)
}

/// The route path a held image is served at for `buffer_id`'s scope, or
/// `None` when the id or token would not survive as one URL segment.
pub fn remote_image_route(buffer_id: &str, token: &str, key: &str) -> Option<String> {
    if !is_url_segment(buffer_id) || !is_url_segment(token) || !is_key(key) {
        return None;
    }
    Some(format!("{REMOTE_IMAGE_PREFIX}/{buffer_id}/{token}/{key}"))
}

/// The parts of a remote-image request, split out of a document-scope path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemoteImageRequest<'a> {
    /// The tab whose editor asked for the image.
    pub buffer_id: &'a str,
    /// The token of that tab's asset scope.
    pub token: &'a str,
    /// The held image's key, [`remote_image_key`] of its URL.
    pub key: &'a str,
}

/// Split a document-scope path into a remote-image request. `None` for any
/// other path or a malformed one. Pure and total.
pub fn split_remote_image_request(document_path: &str) -> Option<RemoteImageRequest<'_>> {
    let rest = document_path
        .strip_prefix(REMOTE_IMAGE_PREFIX)?
        .strip_prefix('/')?;
    let mut parts = rest.split('/');
    let buffer_id = parts.next()?;
    let token = parts.next()?;
    let key = parts.next()?;
    if parts.next().is_some()
        || !is_url_segment(buffer_id)
        || !is_url_segment(token)
        || !is_key(key)
    {
        return None;
    }
    Some(RemoteImageRequest {
        buffer_id,
        token,
        key,
    })
}

fn is_url_segment(value: &str) -> bool {
    !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

fn is_key(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// A fetched image, typed by its leading bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldImage {
    /// MIME type [`check_remote_image_bytes`] decided.
    pub mime: &'static str,
    /// The body, shared so serving it copies nothing.
    pub bytes: Arc<[u8]>,
}

#[derive(Debug)]
struct HeldEntry {
    image: HeldImage,
    readers: HashSet<String>,
    used: u64,
}

/// Fetched images held in memory for the session, each readable only by the
/// tabs whose editor asked for it.
///
/// Nothing is written to disk, so a remote image leaves no trace once the
/// app quits. Bounded by bytes; the least recently used image is dropped
/// first.
#[derive(Debug)]
pub struct RemoteImageStore {
    entries: HashMap<String, HeldEntry>,
    held_bytes: usize,
    capacity: usize,
    clock: u64,
}

impl Default for RemoteImageStore {
    fn default() -> Self {
        Self::with_capacity(REMOTE_IMAGE_STORE_BYTES)
    }
}

impl RemoteImageStore {
    /// A store holding at most `capacity` bytes of images.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: HashMap::new(),
            held_bytes: 0,
            capacity,
            clock: 0,
        }
    }

    /// The image held under `key`, which `buffer_id` may read from now on.
    /// For the editor's request: a second tab showing the same URL is served
    /// without a second fetch.
    pub fn grant(&mut self, key: &str, buffer_id: &str) -> Option<HeldImage> {
        let used = self.tick();
        let entry = self.entries.get_mut(key)?;
        entry.used = used;
        entry.readers.insert(buffer_id.to_string());
        Some(entry.image.clone())
    }

    /// The image held under `key` when `buffer_id` was granted it. For the
    /// route, which never widens who may read an image.
    pub fn read(&mut self, key: &str, buffer_id: &str) -> Option<HeldImage> {
        let used = self.tick();
        let entry = self.entries.get_mut(key)?;
        if !entry.readers.contains(buffer_id) {
            return None;
        }
        entry.used = used;
        Some(entry.image.clone())
    }

    /// Holds `image` under `key` for `buffer_id`, dropping the least recently
    /// used images until it fits. An image larger than the whole store is
    /// not held.
    pub fn insert(&mut self, key: &str, buffer_id: &str, image: HeldImage) {
        let size = image.bytes.len();
        if size > self.capacity {
            return;
        }
        let used = self.tick();
        let mut readers = HashSet::new();
        if let Some(old) = self.entries.remove(key) {
            self.held_bytes -= old.image.bytes.len();
            readers = old.readers;
        }
        readers.insert(buffer_id.to_string());
        while self.held_bytes + size > self.capacity {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.used)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            if let Some(dropped) = self.entries.remove(&oldest) {
                self.held_bytes -= dropped.image.bytes.len();
            }
        }
        self.held_bytes += size;
        self.entries.insert(
            key.to_string(),
            HeldEntry {
                image,
                readers,
                used,
            },
        );
    }

    /// Takes `buffer_id` off every image's readers, once its tab has closed,
    /// and drops the images no open tab may read any more.
    pub fn release_buffer(&mut self, buffer_id: &str) {
        let mut freed = 0;
        self.entries.retain(|_, entry| {
            entry.readers.remove(buffer_id);
            let keep = !entry.readers.is_empty();
            if !keep {
                freed += entry.image.bytes.len();
            }
            keep
        });
        self.held_bytes -= freed;
    }

    /// Bytes of images held now.
    pub fn held_bytes(&self) -> usize {
        self.held_bytes
    }

    fn tick(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }
}
