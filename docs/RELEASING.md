# Releasing Writ

Step-by-step procedure for publishing a new version of Writ to GitHub Releases.
The release pipeline lives in `.github/workflows/release.yml`. This document is
the runbook for the human-driven steps around it.

## 1. One-time setup

These steps are performed once per repository, before the first release.

### 1.1 Generate the Tauri updater keypair

On a secure workstation, run:

```bash
cargo tauri signer generate -w ~/.tauri/writ.key
```

You will be prompted for an optional password. Use a strong, random one and
store it in your password manager. The command produces:

- `~/.tauri/writ.key`       the ed25519 private key
- `~/.tauri/writ.key.pub`   the ed25519 public key

### 1.2 Install the public key into `tauri.conf.json`

Copy the contents of `~/.tauri/writ.key.pub` and paste the base64 string into
`src-tauri/tauri.conf.json` under `plugins.updater.pubkey`, replacing the
`REPLACE_WITH_PUBLIC_KEY_FROM_TAURI_SIGNER_GENERATE` placeholder.

Commit that change on a branch and merge via PR. Never commit the private key.

### 1.3 Load repository secrets

Navigate to `Settings` -> `Secrets and variables` -> `Actions` and add:

| Secret | Required | Notes |
|---|---|---|
| `TAURI_SIGNING_PRIVATE_KEY` | yes, for updater | Paste the contents of `~/.tauri/writ.key` |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | yes if you set one in 1.1 | Leave unset if the key has no password |
| `APPLE_CERTIFICATE` | optional, recommended | Base64-encoded `.p12` Developer ID Application cert |
| `APPLE_CERTIFICATE_PASSWORD` | optional | Password for the `.p12` |
| `APPLE_SIGNING_IDENTITY` | optional | e.g. `Developer ID Application: Your Name (TEAMID)` |
| `APPLE_ID` | optional | Apple ID email used for notarization |
| `APPLE_PASSWORD` | optional | App-specific password for notarization |
| `APPLE_TEAM_ID` | optional | 10-character Apple Team ID |
| `WINDOWS_CERTIFICATE` | optional | Base64-encoded `.pfx` code-signing cert |
| `WINDOWS_CERTIFICATE_PASSWORD` | optional | Password for the `.pfx` |

The updater-signing key (`TAURI_SIGNING_PRIVATE_KEY`) must be present for
auto-update to work at all.

> **macOS notarization is a launch gate for auto-update, not optional.**
> You can ship a *first install* unsigned (users click through Gatekeeper
> once). But when the **in-app updater** swaps the `.app` in place, macOS will
> quarantine or refuse to launch a bundle that is not Developer-ID signed,
> notarized, and stapled. An un-notarized auto-update will download, install,
> relaunch, and then be **blocked on the user's machine**. The minisign
> updater signature does **not** substitute for Apple notarization. Before the
> first release users will auto-update from, confirm the `APPLE_*` secrets
> above are populated; otherwise `release.yml` produces an ad-hoc-signed (`-`),
> un-notarized build. See `docs/adr/007-in-app-updater.md`.

### 1.4 Updater plugin wiring (already in place)

The updater plugin is registered at runtime in `src-tauri/src/lib.rs` and the
client flow lives in `src-tauri/src/commands/update.rs`:

- A silent **check-only** runs ~5s after launch; it surfaces a banner only when
  an update genuinely exists and never auto-installs.
- "Check for Updates…" (app menu) and the `app.check_updates` command run a
  user-visible check.
- The `UpdateBanner` component (backed by `src/stores/update.ts`) prompts the
  user to install, shows download progress, and offers "Restart now".

No further one-time code change is required.

## 2. Cutting a release

### 2.1 Bump the version

Trigger the `Bump version` workflow from the Actions tab and supply the new
semver (e.g. `0.1.0`, `0.2.0-rc.1`). The workflow updates:

- `Cargo.toml` (workspace package version)
- `src-tauri/tauri.conf.json` (top-level `version`)
- `package.json` (top-level `version`)
- `site/package.json` (top-level `version`)

It opens a PR against `main` by default. Review, merge. If you prefer a direct
push, uncheck `create_pr` when dispatching, but PR is recommended for the audit
trail.

### 2.2 Update the changelogs

Move the `Unreleased` section of `CHANGELOG.md` into a dated release section
following [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

Mirror that section into `site/src/data/changelog.ts` as a new first entry in
`releases`: `version`, `date`, and one note per line with its `kind`. The
`/changelog` page renders that array verbatim and nothing generates it. The site
footer takes its version from the published release instead, so skipping this
leaves the deployed site advertising the new version beside a changelog that
stops at the previous one.

Commit both on the same branch as the version bump.

### 2.3 Pre-tag checks, tag, and push

Branches come off `main`, pull requests target `main`, and tags are cut from
`main`. There is no integration branch to merge first.

Before tagging:

1. `origin/main` is green. `.github/workflows/ci.yml` runs the three-platform
   matrix on every push to `main` and every pull request targeting it.
2. `scripts/release-preflight.sh` passes locally:

   ```bash
   scripts/release-preflight.sh
   ```

   It runs seven gates on your machine: `cargo fmt --all --check`,
   `cargo test --workspace`, `cargo clippy --workspace -- -D warnings`,
   `npx tsc --noEmit`, `pnpm --dir site build`, a universal macOS `app,dmg`
   build followed by `scripts/build-mac-pkg.sh`, and an `act --dryrun` of the
   Linux release leg. Gate 7 checks the workflow shape, not the Linux build;
   drop `--dryrun` to run the build itself. Gate 6 is skipped with a warning
   off Darwin, gate 7 when `act` or Docker is missing, and the closing
   `preflight: OK` says nothing about a gate that was skipped. A mac build
   that would fail the tag fails here instead of burning macOS CI minutes.
   Windows still needs CI or a VM.

The script ends by printing the tag command, reading the version from
`site/package.json`:

```bash
git checkout main
git pull --ff-only
git tag "v$(jq -r .version site/package.json)"
git push origin "v$(jq -r .version site/package.json)"
```

The push triggers `.github/workflows/release.yml`.

### 2.4 What the release workflow does

1. `create-release`  creates (or reuses) a **draft** GitHub Release for the tag,
   with an auto-generated changelog grouped by `feat:` / `fix:` / other
   conventional-commit prefixes since the previous `v*.*.*` tag.
2. `build`  runs a matrix build on:
   - `macos-latest`  universal binary (`aarch64` + `x86_64` merged), producing
     `Writ_<version>_universal.pkg` (with quit/relaunch scripts),
     `Writ_<version>_universal.dmg` (drag-to-Applications), and
     `Writ_universal.app.tar.gz` + `.sig`. Both the `.pkg` and the `.dmg` ship
     on every release; the `.pkg` is the default recommendation and the source
     for the Homebrew cask. The in-app updater consumes neither: it downloads
     `Writ_universal.app.tar.gz`, verifies the `.sig` against the embedded
     minisign public key, and swaps the `.app` in place.
   - `windows-latest`  x64, producing `Writ_<version>_x64_en-US.msi` + `.sig`.
   - `ubuntu-22.04`  x64, producing `Writ_<version>_amd64.deb` + `.sig` and
     `Writ_<version>_amd64.AppImage` + `.sig`.

   Tauri v2 signs the platform installer directly instead of wrapping it, so
   there is no `.msi.zip` and no `.AppImage.tar.gz`; the `.sig` sits beside the
   raw installer (`createUpdaterArtifacts` in `src-tauri/tauri.conf.json`).
   Both Linux installers are updater payloads: `latest.json` carries the
   `.AppImage` under `linux-x86_64` and the `.deb` under `linux-x86_64-deb`,
   and the plugin picks by the bundle type baked into the running binary, so a
   deb install updates from the `.deb`. All artifacts are uploaded to the draft
   release.
3. `finalize`  downloads every release asset, generates `SHA256SUMS.txt`, and
   writes `latest.json` (the Tauri updater manifest). Both files are uploaded
   back to the draft release.

**macOS signing and notarization.** The mac leg handles three artifacts
separately, and the `.pkg` needs a different certificate from the other two:

- The `.app` is codesigned by `tauri-action` with the Developer ID
  **Application** identity (`APPLE_SIGNING_IDENTITY`, falling back to ad-hoc
  `-` when the secret is absent) and notarized when `APPLE_ID`,
  `APPLE_PASSWORD`, and `APPLE_TEAM_ID` are all set.
- The `.pkg` is built by `scripts/build-mac-pkg.sh`. `pkgbuild` produces the
  component package carrying the pre-install (quit a running Writ) and
  post-install (relaunch) scripts; `productbuild` wraps it in a distribution
  declaring the macOS 12.0 floor, which a bare component package cannot
  express; `productsign` signs the result when
  `WRIT_INSTALLER_SIGNING_IDENTITY` is set. The workflow sets that itself,
  resolving the Developer ID **Installer** identity out of the build keychain
  rather than reading it from a secret; the certificate arrives as
  `APPLE_INSTALLER_CERTIFICATE`. That is a different certificate from the
  Application one: a `.pkg` signed with the Application identity is rejected.
  With the variable unset the script still writes an installer, unsigned, and
  prints that Gatekeeper will reject it. That is what a local preflight run
  produces, since nothing in `scripts/release-preflight.sh` sets the identity.
- The `.dmg` wraps an already-notarized `.app`, but the wrapper needs its own
  ticket.

With the notarization secrets set, both wrappers go through
`xcrun notarytool submit --wait`, then `xcrun stapler staple` and
`xcrun stapler validate`. Stapling embeds the ticket in the file, so each
upload comes after it: the `.pkg` uploads once its own ticket is stapled, and
the stapled `.dmg` overwrites the pre-staple copy `tauri-action` already
pushed. Without those secrets the job logs what it skipped and the wrappers
ship without tickets.

Two guards sit on this path. The workflow reads the `.pkg` signature back with
`pkgutil --check-signature` and **fails the job** when
`APPLE_INSTALLER_CERTIFICATE` is configured but the signature is absent, so a
silently unsigned installer cannot ship. And it writes `release-meta.json` from
the outcomes it observed, uploaded as a release asset:

```json
{
  "schema": 1,
  "pkg": { "signed": true, "notarized": true, "stapled": true },
  "dmg": { "stapled": true }
}
```

`site.yml` reads that asset back and sets the site's `notarized` flag true only
when `pkg.stapled` and `dmg.stapled` are both true. A missing or unparseable
asset reads as false, so the site cannot claim notarization a release did not
produce. It is deliberately absent from `SHA256SUMS.txt`, where the checksum
step globs binary artifacts only: this is a derived status file the site reads,
not an integrity-bearing download.

### 2.5 Smoke test before publishing

Download each installer from the draft release and install it on a clean
machine:

- macOS: check the `.pkg` before installing it.

  ```bash
  pkgutil --check-signature Writ_<version>_universal.pkg   # Developer ID Installer chain
  xcrun stapler validate Writ_<version>_universal.pkg      # ticket is embedded
  ```

  Both must pass, and `release-meta.json` has to sit among the draft's assets
  with `pkg.signed`, `pkg.notarized`, `pkg.stapled`, and `dmg.stapled` all
  true. Then install. The installer quits any running Writ, swaps the bundle,
  and relaunches the new version. Confirm hotkey and autosave still work. A
  signed and stapled installer draws no Gatekeeper prompt on a machine that
  has never held the certificate; if one appears, stop and find out why
  instead of clicking through. As a secondary check, mount the `.dmg`, drag
  `Writ.app` to `/Applications`, and confirm it launches the same version.
- Windows: run the `.msi`, launch from Start menu, confirm hotkey and
  autosave work.
- Linux: install the `.deb` with `sudo dpkg -i`, or run the `.AppImage`
  directly, and confirm hotkey and autosave work.

Verify `SHA256SUMS.txt`:

```bash
sha256sum --check SHA256SUMS.txt
```

Verify `latest.json` parses and points at real asset URLs:

```bash
curl -fsSL https://github.com/ibrahemid/writ/releases/download/v0.1.0/latest.json | jq .
```

### 2.6 Publish the release

Edit the draft release in the GitHub UI. Adjust the changelog body if needed,
uncheck `Set as a pre-release` for stable releases, and click **Publish**.

Publishing causes GitHub to serve the assets at:

```
https://github.com/ibrahemid/writ/releases/latest/download/latest.json
```

This URL is what installed clients poll for updates (configured in
`tauri.conf.json` under `plugins.updater.endpoints`).

### 2.7 After publishing

Publishing fires two workflows: `site.yml` deploys the site and rewrites
`site/src/data/release.json` from the latest release, and `packages.yml` opens
the distribution-manifest bump PR. Then check:

- `/download` on the site offers the new version and every link resolves.
- The macOS card carries the `notarized` badge. `site.yml` sets that flag from
  the `release-meta.json` asset, true only when `pkg.stapled` and `dmg.stapled`
  are both true, so a missing badge means one of the two never got a stapled
  ticket.
- `/changelog` shows the new version. If it stops at the previous one, the
  `site/src/data/changelog.ts` entry from §2.2 is missing: add it, merge, and
  the site redeploys.
- The Linux one-liner resolves the new tag:

  ```bash
  curl -s https://api.github.com/repos/ibrahemid/writ/releases/latest | jq -r .tag_name
  ```

  This must print the tag you just published. `install.sh` reads that endpoint
  to decide which release to download, and a draft release is not served by it.
- The packaging bump PR carries the right SHAs. Merge it, then follow the
  per-channel push steps in `packaging/README.md`.

## 3. Release-candidate builds

For pre-release testing, tag with a suffix:

```bash
git tag v0.2.0-rc.1
git push origin v0.2.0-rc.1
```

The workflow detects the suffix and marks the draft release as a pre-release
automatically. Pre-releases are not served by `releases/latest`, so installed
clients will not auto-update to them unless you temporarily override the
updater endpoint.

Cut an `-rc` before any release that changes signing, notarization, or the
installer. Signing failures are only visible on a published artifact: the
build succeeds either way, and an unsigned or unstapled `.pkg` looks identical
until Gatekeeper sees it. Download the `.pkg` from the pre-release and run:

```bash
pkgutil --check-signature Writ_0.2.0-rc.1_universal.pkg   # Developer ID Installer chain
spctl -a -t install -vv Writ_0.2.0-rc.1_universal.pkg     # what Gatekeeper decides
xcrun stapler validate Writ_0.2.0-rc.1_universal.pkg      # ticket is embedded, works offline
```

Run them on a machine that has never had the signing certificate installed. All
three must pass, and `release-meta.json` on the same pre-release must report
every flag true, before the real tag goes out.

## 4. Rolling back

If a release is broken after publishing:

1. Mark the release as a pre-release in the GitHub UI (removes it from
   `releases/latest`).
2. Publish a patch release (`v0.1.1`) with the fix.
3. Do not delete broken artifacts; downstream users may have in-flight
   downloads. Let the patch version supersede it.

## 5. Troubleshooting

### 5.1 `tauri-action` fails with "invalid signing identity"

Apple or Windows signing env vars are wrong. Either fix the secret values or
clear the Apple / Windows secrets to fall back to unsigned builds.

### 5.2 `latest.json` has an empty `platforms` object

The finalize step identifies an updater payload by the `.sig` beside it, so an
empty or partial `platforms` means one of `Writ_universal.app.tar.gz`, `.msi`,
`.AppImage`, or `.deb` reached the release without its `.sig`.
`build_latest_json.py` requires all five keys (`darwin-aarch64`,
`darwin-x86_64`, `windows-x86_64`, `linux-x86_64`, `linux-x86_64-deb`) and
fails the job naming the ones it could not fill instead of publishing a partial
manifest. This happens when:

- `createUpdaterArtifacts` is off in `tauri.conf.json`, so nothing is signed.
- `TAURI_SIGNING_PRIVATE_KEY` is unset, so the signatures are skipped.

Verify both and re-run the finalize job.

### 5.3 Draft release already exists for the tag

The `create-release` job detects this and updates the existing draft instead
of failing. Safe to re-run after a failed build.

### 5.4 `SHA256SUMS.txt` missing some artifacts

The finalize job uses glob patterns covering `.pkg`, `.dmg`, `.msi`,
`.AppImage`, `.deb`, `.rpm`, `.tar.gz`, `.zip`, `.sig`, and `.exe`. Extend
`.github/workflows/release.yml` if new bundle types are added.

## 6. Testing the update flow locally (no release, no CI)

This proves the **mechanics** (minisign verification, download, in-place swap,
relaunch) against a local server, without tagging or triggering CI. It does
**not** prove macOS Gatekeeper acceptance: locally built apps are not
quarantined, so a notarized→notarized swap can only be verified with a real
signed release. Treat "works locally" accordingly.

The override path (`WRIT_UPDATER_ENDPOINT` / `WRIT_UPDATER_PUBKEY`) is compiled
in **debug builds only**; a release binary ignores both.

1. Generate a throwaway signing keypair (do not reuse the production key):

   ```bash
   cargo tauri signer generate -w /tmp/writ-test.key
   ```

2. Build a "newer" bundle. Bump the version in `Cargo.toml`,
   `src-tauri/tauri.conf.json`, and `package.json` to a high value
   (e.g. `0.9.0`), then:

   ```bash
   TAURI_SIGNING_PRIVATE_KEY="$(cat /tmp/writ-test.key)" \
     cargo tauri build --bundles app
   ```

   This produces `Writ_0.9.0_universal.app.tar.gz` and a `.sig` under
   `target/universal-apple-darwin/release/bundle/macos/` (or the per-arch
   target dir for a non-universal local build).

3. Serve a manifest + the bundle locally:

   ```bash
   mkdir -p /tmp/writ-staging && cd /tmp/writ-staging
   cp /path/to/Writ_0.9.0_universal.app.tar.gz .
   # latest.json: version 0.9.0; darwin-aarch64 and darwin-x86_64 both point at
   # http://localhost:8000/Writ_0.9.0_universal.app.tar.gz; signature = the
   # contents of the .sig file.
   python3 -m http.server 8000
   ```

4. Run a lower-version debug build pointed at the local endpoint with the
   matching test public key (contents of `/tmp/writ-test.key.pub`):

   ```bash
   WRIT_UPDATER_ENDPOINT="http://localhost:8000/latest.json" \
   WRIT_UPDATER_PUBKEY="$(cat /tmp/writ-test.key.pub)" \
     cargo tauri dev
   ```

5. Trigger "Check for Updates…", click **Install**, watch the progress bar,
   then **Restart now**. Confirm the app relaunches reporting `0.9.0`.

Revert the version bumps from step 2 before committing.

## 7. Related files

- `.github/workflows/release.yml`         release pipeline
- `.github/workflows/bump-version.yml`    version bump automation
- `.github/workflows/packages.yml`        post-release manifest bumps
- `.github/workflows/site.yml`            site deploy, release.json sync
- `.github/scripts/bump_version.py`       version bump implementation
- `.github/scripts/build_latest_json.py`  updater manifest builder
- `scripts/release-preflight.sh`          local gates to run before tagging
- `scripts/build-mac-pkg.sh`              macOS installer build and signing
- `src-tauri/tauri.conf.json`             bundle targets, updater pubkey
- `Cargo.toml`                            workspace version
- `package.json`                          frontend version
- `site/package.json`                     site version, read by the tag command
- `CHANGELOG.md`                          human-curated changelog
- `site/src/data/changelog.ts`            source of the `/changelog` page
- `release-meta.json` (release asset)     macOS signing outcomes the site reads
- `docs/adr/007-in-app-updater.md`        updater design, gates, test loop
- `src-tauri/src/commands/update.rs`      updater IPC + endpoint override
