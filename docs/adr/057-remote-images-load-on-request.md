# ADR-057: Remote images in Markdown load on request, through the host

## Status

Accepted, 2026-11-17. Extends ADR-035.

## Context

Inline mode drew `![](https://…)` as missing: the app window's `img-src` does not grant http(s), and loading a web address from the webview would send a request, cookies and a referrer the moment a file opens.

## Decision

- No request is made when a file opens. A remote reference draws a placeholder naming its host with a Load image button; `[editor] load_remote_images` (Settings, Editor, Load web images) loads them without the click.
- The host fetches, never the webview. `preview_remote_image` takes a buffer id and the reference, checks it is an `http`/`https` URL and that the buffer is open, and fetches it with reqwest: no cookie store, no referrer, a fixed user agent, a 20 s timeout, at most 5 redirects and only to http(s), and a streaming cap of `REMOTE_IMAGE_MAX_BYTES` (16 MB, equal to `ASSET_MAX_BYTES`). The leading bytes must sniff as an image.
- The bytes are held in an in-memory LRU (`RemoteImageStore`, 64 MB) keyed by a hash of the URL and granted per buffer. They are served from `_remote-image/<key>` under the buffer's ADR-035 scope token, so the CSP gains nothing and the URL is a claim checked against the grant.
- Refusals map to the inline image failures; `unreachable` offers Try again.

## Consequences

A remote image costs one request per session per URL, and a file that is closed and reopened draws it from memory while it is still held. The preview pane is unchanged: its document policy still blocks remote images. Addresses are not filtered by network, so a reference to a loopback or private address is fetched like any other once the person asks; the request carries no credentials and its answer is only drawn as an image.
