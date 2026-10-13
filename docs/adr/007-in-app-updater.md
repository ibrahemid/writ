# ADR-007: In-App Updater

**Status:** Accepted. Amended 2026-10-13: signing runs in its own jobs, apart
from the build, and the updater key has a custody rule and a transition path
(Signing; Key custody and transition).
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

Update bundles are signed with a minisign keypair. The public key is embedded
in `tauri.conf.json`, and the client rejects any bundle whose `.sig` does not
verify against it. The minisign signature is the updater's integrity check and
is independent of Apple notarization (below).

The private key (`TAURI_SIGNING_PRIVATE_KEY`, with its password) is a secret of
the `release` GitHub Environment, and only the `sign-updater` job of
`release.yml` reads it. That job checks out the release scripts and
`tauri.conf.json`, installs a pinned minisign, and signs the payloads the build
and signing jobs uploaded. Until this amendment `tauri-action` signed inside
the build, so every build script, proc macro and Vite plugin ran with the key
in its environment; the build now runs with no secret and no write token. The
Apple certificates and notarization credentials belong in the same environment,
where only `sign-macos` reads them. `sign-windows` runs the Tauri CLI beside the
Azure signing credentials, because the bundler calls
`bundle.windows.signCommand`; the CLI is installed with package scripts off,
and nothing is compiled there.

When this amendment was written, the Apple secrets and a second copy of the
updater key were also repository secrets, which every workflow in the
repository can read. `docs/RELEASING.md` section 1.3 moves them into the
environment and deletes the repository copies; until that is done, the
environment does not keep them from other workflows.

Each `.sig` keeps the format `tauri signer sign --app-version` writes: the
trusted comment names the file and the version, and the plugin refuses a
payload whose signed version differs from the version `latest.json` announces.
The same key signs `SHA256SUMS.txt` as `SHA256SUMS.txt.minisig`.
`sign-updater` checks every signature it makes against the public key in
`tauri.conf.json` of the latest published release, the key installed copies
hold, and fails the release on any other key.

### Key custody and transition

The key file and its password must have two offline copies, on separate media
in separate places (RELEASING 1.1). They are made by hand, and nothing in the
repository records that they exist. The environment secret is a working copy:
GitHub never shows it again, and a lost key cannot be replaced by any release,
because installed copies trust only the key built into them.

A new key reaches installed copies through one release (N) signed with the old
key that carries the new public key. Every release after N is signed with the
new key. The latest-release check above enforces both halves: while N-1 is the
latest release only the old key passes, and once N is published only the new
one does. A copy that never installs N keeps trusting the old key and cannot
verify a later release; its owner has to reinstall. `install.sh` embeds one
key, so it has to accept both during a transition. The procedure is
`docs/RELEASING.md` section 1.5, and `test_release_signing.py` rehearses it
with throwaway keys.

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
- A key transition takes two releases and strands every copy that skips the
  first one.
- The notarization gate is enforced by humans/secrets, not by the compiler; a
  release shipped with empty Apple secrets will produce auto-updates that break
  on launch. The risk is documented here and in `docs/RELEASING.md` rather than
  prevented mechanically.
