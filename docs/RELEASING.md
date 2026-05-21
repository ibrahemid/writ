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

> **macOS notarization is a launch gate for auto-update — not optional.**
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

It opens a PR by default. Review, merge. If you prefer a direct push, uncheck
`create_pr` when dispatching, but PR is recommended for the audit trail.

### 2.2 Update CHANGELOG.md

Move the `Unreleased` section into a dated release section following
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Commit on the same
branch as the version bump.

### 2.3 Tag and push

Once the version-bump PR is merged to `main`:

```bash
git checkout main
git pull --ff-only
git tag v0.1.0          # match the version you bumped to
git push origin v0.1.0
```

The push triggers `.github/workflows/release.yml`.

### 2.4 What the release workflow does

1. `create-release`  creates (or reuses) a **draft** GitHub Release for the tag,
   with an auto-generated changelog grouped by `feat:` / `fix:` / other
   conventional-commit prefixes since the previous `v*.*.*` tag.
2. `build`  runs a matrix build on:
   - `macos-latest`  universal binary (`aarch64` + `x86_64` merged), producing
     `.pkg` (with quit/relaunch scripts), `.dmg` (drag-to-Applications),
     `.app.tar.gz`, and `.app.tar.gz.sig`. Both the `.pkg` and `.dmg` ship on
     every release; the `.pkg` is the default recommendation and the source
     for the Homebrew cask and the in-app updater.
   - `windows-latest`  x64, producing `.msi`, `.msi.zip`, `.msi.zip.sig`.
   - `ubuntu-22.04`  x64, producing `.deb`, `.AppImage`, `.AppImage.tar.gz`,
     `.AppImage.tar.gz.sig`.
   All artifacts are uploaded to the draft release by `tauri-action`.
3. `finalize`  downloads every release asset, generates `SHA256SUMS.txt`, and
   writes `latest.json` (the Tauri updater manifest). Both files are uploaded
   back to the draft release.

### 2.5 Smoke test before publishing

Download each installer from the draft release and install it on a clean
machine:

- macOS: double-click the `.pkg`, allow it through Gatekeeper if needed, and
  install. The installer quits any running Writ, swaps the bundle, and
  relaunches the new version. Confirm hotkey and autosave still work. As a
  secondary check, mount the `.dmg`, drag `Writ.app` to `/Applications`, and
  confirm it launches the same version.
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

The finalize step did not find any `.app.tar.gz`, `.msi.zip`, or
`.AppImage.tar.gz` among the release assets. This happens when:

- The `updater` bundle target is missing from `tauri.conf.json`.
- `TAURI_SIGNING_PRIVATE_KEY` is unset, so the updater bundles are skipped.

Verify both and re-run the finalize job.

### 5.3 Draft release already exists for the tag

The `create-release` job detects this and updates the existing draft instead
of failing. Safe to re-run after a failed build.

### 5.4 `SHA256SUMS.txt` missing some artifacts

The finalize job uses glob patterns covering `.pkg`, `.dmg`, `.msi`,
`.AppImage`, `.deb`, `.rpm`, `.tar.gz`, `.zip`, `.sig`, and `.exe`. Extend
`.github/workflows/release.yml` if new bundle types are added.

## 6. Testing the update flow locally (no release, no CI)

This proves the **mechanics** — minisign verification, download, in-place swap,
relaunch — against a local server, without tagging or triggering CI. It does
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
- `.github/scripts/bump_version.py`       version bump implementation
- `.github/scripts/build_latest_json.py`  updater manifest builder
- `src-tauri/tauri.conf.json`             bundle targets, updater pubkey
- `Cargo.toml`                            workspace version
- `package.json`                          frontend version
- `CHANGELOG.md`                          human-curated changelog
- `docs/adr/007-in-app-updater.md`        updater design, gates, test loop
- `src-tauri/src/commands/update.rs`      updater IPC + endpoint override
