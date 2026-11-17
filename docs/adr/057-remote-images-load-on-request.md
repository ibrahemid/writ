# ADR-057: Remote images in Markdown load on request, through the host

## Status

Accepted, 2026-11-17. Extends ADR-035.

## Context

Inline mode drew `![](https://…)` as missing: the app window's `img-src` does not grant http(s), and loading a web address from the webview would send a request, cookies and a referrer the moment a file opens.

## Decision

- No request is made when a file opens. A remote reference draws a placeholder naming its host with a Load image button; `[editor] load_remote_images` (Settings, Editor, Load web images) loads them without the click.
- A load nobody clicked for (`load: automatic`) waits for a click when its host is local by name or by address range. It is refused with `local_address` when the host is `localhost` or ends in `.localhost`, `.local` or `.internal`, or is a loopback, unspecified, private, CGNAT, link-local or unique-local address, IPv4 or IPv6, the local-use NAT64 prefix `64:ff9b:1::/48`, or an IPv6 address that embeds such an IPv4 address (IPv4-mapped, IPv4-compatible, NAT64 `64:ff9b::/96`, 6to4 `2002::/16`). It is refused with `resolves_to_local` when the name resolves to any such address, and the caption then says the host is on the network. Both keep the placeholder and its button. The automatic client resolves each name once, checks every address, and connects to those addresses with no proxy; the classification is `writ_core::preview::remote_image::is_local_address` and `is_local_host_name`. A click (`load: asked`) fetches any http(s) address, a local one included.
- Each request builds its own client, whose redirect policy and resolver know the host the placeholder named. A redirect to a local name, literal or resolved address at any other host is refused with `redirects_to_local`, a click included, since the placeholder named a different host. A redirect within the named host follows that host's rule. Through a system proxy, a click's redirect target is resolved by the proxy, so only its name and literal are checked.
- The host fetches, never the webview. `preview_remote_image` takes a buffer id, the reference and the load kind, checks it is an `http`/`https` URL and that the buffer is open, and fetches it with reqwest: no cookie store, no referrer, a fixed user agent, a 20 s timeout, at most 5 redirects and only to http(s), and a streaming cap of `REMOTE_IMAGE_MAX_BYTES` (16 MB, equal to `ASSET_MAX_BYTES`). The leading bytes must sniff as an image.
- The bytes are held in an in-memory LRU (`RemoteImageStore`, 64 MB) keyed by a hash of the URL and granted per buffer. They are served from `_remote-image/<key>` under the buffer's ADR-035 scope token, so the CSP gains nothing and the URL is a claim checked against the grant.
- Refusals map to the inline image failures; `unreachable` offers Try again, `local_address` and `resolves_to_local` keep Load image, and `redirects_to_local` has no button.

## Consequences

A remote image costs one request per session per URL, and a file that is closed and reopened draws it from memory while it is still held. The preview pane is unchanged: its document policy still blocks remote images. With loading turned on, a file cannot make Writ request a host that is local by name or whose addresses are in a local-use range; the person can still click to load such an image, and the request carries no credentials and its answer is only drawn as an image. This is the address-space rule browsers apply to local network requests, with no interface enumeration: a device on the local network that holds a public address, such as a global IPv6 address or this machine's own public address, is not recognized as local and loads without a click. Automatic loads go direct and ignore a system proxy, so behind a proxy that is the only way out they fail and need the click.
