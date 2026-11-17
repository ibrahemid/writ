# ADR-057: Remote images in Markdown load on request, through the host

## Status

Accepted, 2026-11-17. Extends ADR-035.

## Context

Inline mode drew `![](https://…)` as missing: the app window's `img-src` does not grant http(s), and loading a web address from the webview would send a request, cookies and a referrer the moment a file opens.

## Decision

- No request is made when a file opens. A remote reference draws a placeholder naming its host with a Load image button; `[editor] load_remote_images` (Settings, Editor, Load web images) loads them without the click.
- A load nobody clicked for (`load: automatic`) never reaches this machine or the local network. It is refused with `local_address`, and the placeholder and its button stay, when the host is `localhost` or ends in `.localhost`, `.local` or `.internal`; when it is a loopback, unspecified, private, CGNAT, link-local or unique-local address, IPv4 or IPv6, including IPv4-mapped and IPv4-compatible IPv6; when the name resolves to any such address; or when a redirect leads to one. The automatic client resolves each name once, checks every address, and connects to those addresses with no proxy; the classification is `writ_core::preview::remote_image::is_local_address` and `is_local_host_name`. A click (`load: asked`) fetches any http(s) address.
- The host fetches, never the webview. `preview_remote_image` takes a buffer id, the reference and the load kind, checks it is an `http`/`https` URL and that the buffer is open, and fetches it with reqwest: no cookie store, no referrer, a fixed user agent, a 20 s timeout, at most 5 redirects and only to http(s), and a streaming cap of `REMOTE_IMAGE_MAX_BYTES` (16 MB, equal to `ASSET_MAX_BYTES`). The leading bytes must sniff as an image.
- The bytes are held in an in-memory LRU (`RemoteImageStore`, 64 MB) keyed by a hash of the URL and granted per buffer. They are served from `_remote-image/<key>` under the buffer's ADR-035 scope token, so the CSP gains nothing and the URL is a claim checked against the grant.
- Refusals map to the inline image failures; `unreachable` offers Try again, and `local_address` keeps Load image.

## Consequences

A remote image costs one request per session per URL, and a file that is closed and reopened draws it from memory while it is still held. The preview pane is unchanged: its document policy still blocks remote images. With loading turned on, a file cannot make Writ request a router, printer or service on the local network or this machine; the person can still click to load such an image, and the request carries no credentials and its answer is only drawn as an image. Automatic loads go direct and ignore a system proxy, so behind a proxy that is the only way out they fail and need the click. NAT64 and 6to4 addresses that embed a private IPv4 address are not classified as local.
