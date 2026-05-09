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

For the initial launch you can ship **unsigned** by leaving the Apple and
Windows secrets empty. The updater-signing key (`TAURI_SIGNING_PRIVATE_KEY`)
must be present for auto-update to work, even on unsigned builds.

### 1.4 Wire the updater plugin in Rust (one-time code change)

The workflow, config, and dependency are in place, but the updater plugin must
be registered at runtime in `src-tauri/src/lib.rs` before the client can fetch
updates:

```rust
tauri::Builder::default()
    .plugin(tauri_plugin_updater::Builder::new().build())
    // ...existing plugins...
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
```

This change is intentionally out of scope for the release-pipeline PR. Land it
before the first tagged release so that installed clients can update.

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

## 6. Related files

- `.github/workflows/release.yml`         release pipeline
- `.github/workflows/bump-version.yml`    version bump automation
- `.github/scripts/bump_version.py`       version bump implementation
- `.github/scripts/build_latest_json.py`  updater manifest builder
- `src-tauri/tauri.conf.json`             bundle targets, updater pubkey
- `Cargo.toml`                            workspace version
- `package.json`                          frontend version
- `CHANGELOG.md`                          human-curated changelog
