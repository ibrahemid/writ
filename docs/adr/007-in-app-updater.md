# ADR-007: In-App Updater

**Status:** Accepted
**Date:** 2026-05-21

## Context

Writ ships as a desktop app outside any app store, so it must update itself.
`tauri-plugin-updater` provides the mechanism (fetch a manifest, download a
signed bundle, verify, swap, relaunch), but several decisions around it are
Writ-specific and easy to get wrong in ways that only surface on real users'
machines after a release has shipped.

The wiring that existed before this ADR was incomplete and unsafe:

- The startup path silently auto-downloaded, auto-installed, and restarted
  with no consent — it could interrupt a user mid-edit.
- It surfaced state through native `tauri-plugin-dialog` message boxes, which
  violates Writ's "no native `confirm`/`alert`" rule and cannot show progress.
- The config carried `active` / `dialog` keys that are Tauri v1 leftovers (v2
  has no built-in updater dialog) and silently do nothing.

## Decision

### Policy in core, mechanism in the adapter

The legal update lifecycle is a pure state machine in
`writ-core::update::UpdatePhase` (`idle → checking → available → downloading →
installing → ready`, plus `up_to_date` / `failed` / dismiss paths). It has no
Tauri or network dependency and is unit-tested in isolation. `src-tauri`
(`commands/update.rs`) drives the mechanism and feeds observed events through
`UpdatePhase::apply`, which rejects illegal transitions. The phase is the
single source of truth: `AppState` holds a `Mutex<UpdatePhase>`, and every
change is mirrored to the frontend as one `writ://update-status` event
carrying the serialized phase. The frontend store mirrors it; the
`UpdateBanner` component renders it. No JS updater/process plugin is needed, so
no new capabilities are granted.

### Consent and visibility

The startup check (5s after launch) is **check-only and silent**: it never
installs, and it only surfaces a banner when an update genuinely exists. A
failed silent check (including the pre-launch 404, see below) is swallowed with
a single debug log — no banner, no log spam. A manual "Check for Updates…"
(menu → `app.check_updates` command) surfaces every outcome, including a
graceful, non-alarming failure message. Install and restart are always
user-initiated through the banner.

### Distribution endpoint

Updates are served from public GitHub Releases:
`https://github.com/ibrahemid/writ/releases/latest/download/latest.json`.

This URL is **not fetchable while the repository is private**. It begins
working the moment the repo is made public at launch (the repo flip is already
a launch gate). The first public release's `latest.json` must be *published*,
not left as a draft, or clients receive a 404. This coupling of update
availability to repo visibility is accepted for launch; revisit a dedicated
public distribution channel if updates are ever needed before the source repo
is public.

### Signing

Update bundles are signed with a minisign keypair. The private key
(`TAURI_SIGNING_PRIVATE_KEY`, optional password) lives only in GitHub Actions
secrets and is injected into `tauri-action`; it is never committed or logged.
The public key is embedded in `tauri.conf.json`. The client rejects any bundle
whose `.sig` does not verify against that key. The minisign signature is the
updater's integrity check and is independent of Apple notarization (below).

### Test-only endpoint override (debug builds only)

For the local test loop, `WRIT_UPDATER_ENDPOINT` (and optional
`WRIT_UPDATER_PUBKEY`) override the endpoint and verification key via
`updater_builder()`. This branch is compiled out of release builds with
`#[cfg(debug_assertions)]`. A release binary that let an environment variable
redirect both the update source and the verification key would be a
supply-chain hole; the override therefore cannot exist in a shipped binary.

### Error sanitization

`sanitize_update_error` redacts URLs from updater error strings before they
reach logs or the UI, so an endpoint (which may later carry signed query
parameters) cannot leak. It is unit-tested.

## Launch gates and limitations

These are **blocking** for relying on auto-update in production. They are not
fixable in code from this branch:

1. **macOS notarization is required, not optional.** When the updater swaps the
   `.app`, macOS Gatekeeper will quarantine or refuse to launch a bundle that
   is not Developer-ID signed, notarized, and stapled. `release.yml` only signs
   and notarizes when the `APPLE_CERTIFICATE` / `APPLE_SIGNING_IDENTITY` /
   `APPLE_ID` / `APPLE_PASSWORD` / `APPLE_TEAM_ID` secrets are present;
   otherwise it produces an ad-hoc-signed (`-`), un-notarized build. An
   un-notarized auto-update will download, install, relaunch, and then be
   **blocked on real users' machines**. Confirm those secrets are configured
   before the first release that users will auto-update from.

2. **The local test loop cannot prove notarization acceptance.** Locally built
   apps are not quarantined, so the throwaway-keypair loop (see
   `docs/RELEASING.md`) proves only minisign verification + download + swap +
   relaunch mechanics. It says nothing about whether Gatekeeper will accept a
   notarized→notarized swap. "Proven locally" must never be read as "proven for
   production."

## Consequences

**Positive:**
- The phase machine is testable without a Tauri runtime or network, and the
  frontend has exactly one state model to mirror.
- No silent installs, no native dialogs, no new IPC capabilities.
- The test override makes the full check→download→install→relaunch loop
  reproducible locally without cutting a release or triggering CI.

**Negative / risks:**
- Update availability is coupled to repo visibility (accepted for launch).
- The notarization gate is enforced by humans/secrets, not by the compiler; a
  release shipped with empty Apple secrets will produce auto-updates that break
  on launch. The risk is documented here and in `docs/RELEASING.md` rather than
  prevented mechanically.
