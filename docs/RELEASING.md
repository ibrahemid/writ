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

Keep two offline copies of the key file and its password, on separate media
stored in separate places, for example two encrypted drives, one of them off
site. The `release` environment secret (1.3) is a working copy, not a backup:
GitHub never shows a saved secret again. Installed copies verify every update
against the public key built into them, so a lost private key strands them: no
later release can reach them, and each person has to reinstall by hand.

### 1.2 Install the public key into `tauri.conf.json`

Copy the contents of `~/.tauri/writ.key.pub` and paste the base64 string into
`src-tauri/tauri.conf.json` under `plugins.updater.pubkey`, replacing the
`REPLACE_WITH_PUBLIC_KEY_FROM_TAURI_SIGNER_GENERATE` placeholder.

Commit that change on a branch and merge via PR. Never commit the private key.

### 1.3 Load the release environment secrets

The signing secrets live in the `release` GitHub Environment: `Settings` ->
`Environments` -> `release` -> `Environment secrets`. Only the jobs that sign
name that environment (`signing-config`, `sign-macos`, `sign-windows` and
`sign-updater` in `.github/workflows/release.yml`), so no other job can read
them. The environment has no required reviewer, and a release run does not
wait for approval.

| Secret | Read by | Notes |
|---|---|---|
| `TAURI_SIGNING_PRIVATE_KEY` | `sign-updater` | Paste the contents of `~/.tauri/writ.key` |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | `sign-updater` | Leave unset if the key has no password |
| `APPLE_CERTIFICATE` | `sign-macos` | Base64-encoded `.p12` Developer ID Application cert |
| `APPLE_CERTIFICATE_PASSWORD` | `sign-macos` | Password for the `.p12` |
| `APPLE_INSTALLER_CERTIFICATE` | `sign-macos` | Base64-encoded `.p12` Developer ID Installer cert, which signs the `.pkg` |
| `APPLE_INSTALLER_CERTIFICATE_PASSWORD` | `sign-macos` | Password for the installer `.p12` |
| `APPLE_SIGNING_IDENTITY` | `sign-macos` | Optional, e.g. `Developer ID Application: Your Name (TEAMID)`; read from the imported certificate when unset |
| `APPLE_ID` | `sign-macos` | Apple ID email used for notarization |
| `APPLE_PASSWORD` | `sign-macos` | App-specific password for notarization |
| `APPLE_TEAM_ID` | `sign-macos` | 10-character Apple Team ID |
| `AZURE_TENANT_ID` | `signing-config`, `sign-windows` | Windows signing, below |
| `AZURE_CLIENT_ID` | `signing-config`, `sign-windows` | Windows signing, below |
| `AZURE_CLIENT_SECRET` | `signing-config`, `sign-windows` | Windows signing, below |

The updater-signing key (`TAURI_SIGNING_PRIVATE_KEY`) must be present for
auto-update to work at all.

Every job of every workflow can read a repository secret, which defeats the
environment. Keep these secrets in the environment only, and delete any
repository secret of the same name under `Settings` -> `Secrets and variables`
-> `Actions`. A job in the environment reads the environment's value when both
exist, so moving a secret is safe in this order: add it to the environment,
then delete the repository copy.

**Windows signing.** `sign-windows` signs the sidecar, the app executable and
the MSI through `bundle.windows.signCommand`, with signtool and an Azure
Artifact Signing account. Setting one up:

1. Create an Artifact Signing account and a public-trust certificate profile
   in Azure, which includes Microsoft's identity validation.
2. Register an app with a client secret and give it the certificate profile
   signer role on the account.
3. In the `release` environment, add `AZURE_TENANT_ID`, `AZURE_CLIENT_ID` and
   `AZURE_CLIENT_SECRET` as secrets, and `AZURE_SIGNING_ENDPOINT` (the
   account's regional endpoint, e.g. `https://eus.codesigning.azure.net`),
   `AZURE_SIGNING_ACCOUNT` and `AZURE_SIGNING_PROFILE` as environment
   variables.

Until the three secrets exist, `signing-config` reports no account,
`sign-windows` is skipped, and the release ships the build's unsigned MSI,
which SmartScreen warns about. Once they exist, a failing `sign-windows` stops
the release rather than shipping the MSI unsigned.

> **macOS notarization is a launch gate for auto-update, not optional.**
> You can ship a *first install* unsigned (users click through Gatekeeper
> once). But when the **in-app updater** swaps the `.app` in place, macOS will
> quarantine or refuse to launch a bundle that is not Developer-ID signed,
> notarized, and stapled. An un-notarized auto-update will download, install,
> relaunch, and then be **blocked on the user's machine**. The minisign
> updater signature does **not** substitute for Apple notarization. In this
> repository `sign-macos` stops the release unless the `.app`, the `.pkg` and
> the `.dmg` are all notarized and stapled; a fork without the `APPLE_*`
> secrets builds an ad-hoc-signed (`-`), un-notarized release and says so. See
> `docs/adr/007-in-app-updater.md`.

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

### 1.5 Moving to a new updater key

Installed copies trust one key, the one built into them, so a new key reaches
them through a release signed with the old key that carries the new public
key. `sign-updater` checks every signature against the public key in
`src-tauri/tauri.conf.json` of the latest published release, which is the key
installed copies hold, and fails the run on any other key.

Before starting, change `install.sh` to accept the old and the new key. It
checks the AppImage of the latest published release against the single key it
embeds (`MINISIGN_PUBKEY`): that release is signed with the old key through N
(below), and with the new key from the release after N.

1. Generate the new key offline (1.1) and make its two offline copies.
2. Release N: put the new public key in `plugins.updater.pubkey`, keep the old
   key in the `release` environment, and release as usual. N is signed with
   the old key, which every installed copy accepts, and carries the new one.
3. Once N is published, replace `TAURI_SIGNING_PRIVATE_KEY` and its password
   in the environment with the new key. Every later release is signed with it,
   since the latest published release is now N.
4. Leave N as the latest release long enough for most installed copies to take
   it. A copy that skips N keeps trusting the old key and cannot verify any
   later release: its owner has to reinstall from the site.
5. Nothing signs with the old key after step 3: destroy its copies. Once the
   first release signed with the new key is published, drop the old key from
   `install.sh`.

`KeyTransitionRehearsal` in `.github/scripts/test_release_signing.py` runs
these steps with throwaway keys: N signed with the old key passes against the
previous release's key and carries old-key signatures in `latest.json`, the
release after it passes only with the new key, and a release signed with the
wrong key fails in either position.

## 2. Cutting a release

### 2.1 Bump the version

Trigger the `Bump version` workflow from the Actions tab and supply the new
semver (e.g. `0.1.0`, `0.2.0-1`). The workflow updates:

- `Cargo.toml` (workspace package version) and `Cargo.lock` (the workspace
  crates only)
- `src-tauri/tauri.conf.json` (top-level `version`)
- `package.json` (top-level `version`)
- `site/package.json` (top-level `version`)
- `site/src/data/release.json` (version, tag and download URLs; the artifact
  fields are cleared)

It opens a PR against `main` by default. A PR the workflow opens does not start
CI, so the changelog commit from §2.2, pushed to the same branch, is what runs
the required checks. If you prefer a direct push, uncheck `create_pr` when
dispatching, but PR is recommended for the audit trail.

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

   Step 0 rebuilds the mac sidecars. Then ten gates run on your machine:
   `cargo deny --locked check advisories bans licenses sources` against
   `deny.toml`, `cargo fmt --all --check`, `cargo test --workspace --locked`,
   `cargo clippy --workspace --locked -- -D warnings`, `npx tsc --noEmit`, the
   third-party notices check, `node scripts/check-release-json.mjs`,
   `pnpm --dir site build`, a universal macOS `app,dmg` build with `--locked`
   followed by the `sign-macos` steps with an ad-hoc identity and the updater
   signature checked against the latest release's key, and an `act --dryrun`
   of the release job graph. Gate 1 needs cargo-deny 0.20.2
   (`cargo install cargo-deny --version 0.20.2 --locked`) and fails when it is
   missing; it catches an advisory published after the last merge. Gate 9
   needs minisign (`brew install minisign`), the updater key from 1.1 and a
   signed-in `gh`. On macOS the script reads the key and its password before
   gate 0, stops there if either is missing, and passes them to the signing
   step alone. Gate 10 checks the workflow shape and the job graph, not the
   builds; `sign-windows` shows as skipped there because its condition reads
   an output only a real run sets. Gate 9 is skipped with a warning off
   Darwin, gate 10 when `act` or Docker is missing, and the closing
   `preflight: OK` says nothing about a gate that was skipped. A mac build or
   signing step that would fail the tag fails here instead of burning macOS CI
   minutes. Windows still needs CI or a VM.

   rustup installs the compiler `rust-toolchain.toml` pins on the first cargo
   command, the same one CI and the release workflow use.

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

The workflow token can only read the repository; the two jobs that need more
say so. Every checkout is the tag `plan` resolved, on a manual run too.

1. `plan` resolves the tag, checks `site/src/data/release.json`, and writes the
   release notes, grouped by `feat:` / `fix:` / other conventional-commit
   prefixes (`.github/scripts/release_notes.py`). A stable tag lists the
   commits since the previous stable `v*.*.*` tag, skipping candidates; a
   candidate lists the commits since the previous tag of either kind.
2. `build` runs the matrix with no secret and no write token:
   - `macos-latest`  universal binary (`aarch64` + `x86_64` merged), producing
     `Writ.app`, ad-hoc signed, inside `Writ_<version>_universal.dmg`.
   - `windows-latest`  x64, producing `Writ_<version>_x64_en-US.msi`, plus the
     two executables `sign-windows` signs.
   - `ubuntu-22.04`  x64, producing `Writ_<version>_amd64.deb` and
     `Writ_<version>_amd64.AppImage`.

   Every leg builds with `--locked`, so the release compiles the lockfile CI
   tested, and with `createUpdaterArtifacts` off: no updater signature is made
   where dependency code runs.
3. `sbom` writes CycloneDX SBOMs: `writ-tauri_<version>_<target>.cdx.json` and
   `writ-cli_<version>_<target>.cdx.json` for each release target
   (cargo-cyclonedx), and `writ-frontend_<version>.cdx.json` for the pnpm tree
   (cdxgen). It refuses an SBOM that `actions/attest` would reject (no
   `bomFormat`, `specVersion` or `serialNumber`), one that describes another
   project or version, and one with no components.
4. `signing-config` reports whether the Windows signing secrets exist (1.3).
5. `sign-macos` signs, notarizes and staples the app, the `.pkg` and the
   `.dmg`, and archives the signed app as the updater payload
   `Writ_<version>_universal.app.tar.gz` (below).
6. `sign-windows` rebuilds the MSI around the build's executables with
   `tauri bundle`, signing each through `bundle.windows.signCommand`. It runs
   only when a Windows signing account exists.
7. `sign-updater` checks out the release scripts and `tauri.conf.json` only,
   installs a pinned minisign (`scripts/install-minisign.sh`), and refuses any
   input file missing or extra. It signs each updater payload as
   `<payload>.sig`, in the format `tauri signer sign` writes with the version
   in the trusted comment, writes `SHA256SUMS.txt` over the installers, their
   `.sig` files and the SBOMs, signs it as `SHA256SUMS.txt.minisig`, and
   writes `latest.json` (the Tauri updater manifest). It then verifies all of
   it against the key of the latest published release (1.5).
8. `attest-sboms` attests each SBOM against the installers it describes.
9. `publish`, the only job with a write token, downloads the signed set,
   refuses a missing or extra file and any checksum mismatch, attests build
   provenance for every asset, creates (or reuses) the **draft** release with
   the notes, and uploads the assets.

The `.pkg` (with quit/relaunch scripts) and the `.dmg` (drag-to-Applications)
ship on every release; the `.pkg` is the default recommendation and the source
for the Homebrew cask. The in-app updater consumes neither: it downloads
`Writ_<version>_universal.app.tar.gz`, verifies the `.sig` against the embedded
minisign public key, and swaps the `.app` in place.

There is no `.msi.zip` and no `.AppImage.tar.gz`: the Windows and Linux updater
payloads are the raw installers, with the `.sig` beside them. Both Linux
installers are updater payloads: `latest.json` carries the `.AppImage` under
`linux-x86_64` and the `.deb` under `linux-x86_64-deb`, and the plugin picks by
the bundle type baked into the running binary, so a deb install updates from
the `.deb`.

**macOS signing and notarization.** `sign-macos` works on the app the build
job made, and the `.pkg` needs a different certificate from the other two
artifacts. `scripts/macos-release.sh` holds the signing steps:

- The `.app` is extracted from the build's `.dmg` and signed inside out with
  the Developer ID **Application** identity (`APPLE_SIGNING_IDENTITY`, or the
  one in the imported certificate): nested executables first, then the bundle,
  each with `--options runtime --timestamp`. It is then notarized and stapled.
- The `.pkg` is built from that signed app by `scripts/build-mac-pkg.sh`.
  `pkgbuild` produces the component package carrying the pre-install (quit a
  running Writ) and post-install (relaunch) scripts; `productbuild` wraps it
  in a distribution declaring the macOS 12.0 floor, which a bare component
  package cannot express; `productsign` signs the result when
  `WRIT_INSTALLER_SIGNING_IDENTITY` is set. The workflow sets that itself,
  resolving the Developer ID **Installer** identity out of the build keychain
  rather than reading it from a secret; the certificate arrives as
  `APPLE_INSTALLER_CERTIFICATE`. That is a different certificate from the
  Application one: a `.pkg` signed with the Application identity is rejected.
  With the variable unset the script still writes an installer, unsigned, and
  prints that Gatekeeper will reject it.
- The `.dmg` is the build's image with its `Writ.app` swapped for the signed
  one, keeping the window layout and volume icon. The wrapper is then signed,
  notarized and stapled on its own.

With the notarization secrets set, each artifact goes through
`xcrun notarytool submit --wait`, then `xcrun stapler staple` and
`xcrun stapler validate`; the submission status is read from the tool's JSON
output rather than its exit code. Without those secrets the job logs what it
skipped. In this repository the job then fails: the app, the `.pkg` and the
`.dmg` must all be notarized and stapled before anything publishes.

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
produce. It is deliberately absent from `SHA256SUMS.txt`, which lists the
installers, their `.sig` files and the SBOMs: this is a derived status file the
site reads, not an integrity-bearing download.

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

Verify `SHA256SUMS.txt`, then its signature against the key installed copies
hold, the one in the latest published release (during a key move that is the
old key, 1.5):

```bash
sha256sum --check SHA256SUMS.txt
git fetch --tags
git show "$(gh release view --json tagName --jq .tagName):src-tauri/tauri.conf.json" \
  | jq -r .plugins.updater.pubkey | base64 --decode > writ-updater.pub
minisign -Vm SHA256SUMS.txt -p writ-updater.pub
```

Check the attestations on one installer per platform: build provenance, then
the SBOM attestation for the same file.

```bash
for asset in Writ_<version>_universal.pkg Writ_<version>_x64_en-US.msi Writ_<version>_amd64.AppImage; do
  gh attestation verify "$asset" --repo ibrahemid/writ
  gh attestation verify "$asset" --repo ibrahemid/writ --predicate-type https://cyclonedx.org/bom
done
```

Verify `latest.json` parses and points at real asset URLs:

```bash
VERSION="$(jq -r .version site/package.json)"
curl -fsSL "https://github.com/ibrahemid/writ/releases/download/v${VERSION}/latest.json" | jq .
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
the distribution-manifest bump PR. Pages keeps the live site when a commit it
already deployed is deployed again, so `site.yml` skips the push deploy of the
commit the `Bump version` workflow creates (`chore(release): bump version to
v<version>`). Tag that commit: the release run then makes its first and only
deploy, with the published release data. A release on a commit a push already
deployed leaves the old site live, and dispatching `site.yml` on an already
deployed commit changes nothing. Push a new commit to refresh the site. Then
check:

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

For pre-release testing, bump to a numeric pre-release and tag it:

```bash
git tag v0.2.0-1
git push origin v0.2.0-1
```

The MSI bundler accepts only a number of at most 65535 after the dash
(`0.2.0-1` becomes MSI version `0.2.0.1`); `-rc.1` fails the Windows leg. The
MSI of a candidate sorts above the final `0.2.0` (`0.2.0.0`), so do not install
it on a machine that should take the release.

The workflow detects the suffix and marks the draft release as a pre-release
automatically. Pre-releases are not served by `releases/latest`, so installed
clients will not auto-update to them unless you temporarily override the
updater endpoint.

Cut a candidate before any release that changes signing, notarization, the
installer, or the release workflow. Signing failures are only visible on a
published artifact: the build succeeds either way, and an unsigned or
unstapled `.pkg` looks identical until Gatekeeper sees it. The order
`sign-macos` signs, notarizes, staples and repacks in is only proven by a
candidate. Download the `.pkg` from the pre-release and run:

```bash
pkgutil --check-signature Writ_0.2.0-1_universal.pkg   # Developer ID Installer chain
spctl -a -t install -vv Writ_0.2.0-1_universal.pkg     # what Gatekeeper decides
xcrun stapler validate Writ_0.2.0-1_universal.pkg      # ticket is embedded, works offline
```

Run them on a machine that has never had the signing certificate installed. All
three must pass, and `release-meta.json` on the same pre-release must report
every flag true, before the real tag goes out. On the same pre-release,
`minisign` must verify `SHA256SUMS.txt` and `gh attestation verify` must pass
for one installer per platform (2.5). Without a Windows signing account the
candidate's MSI is unsigned (1.3); with one, check it on Windows:

```powershell
Get-AuthenticodeSignature .\Writ_0.2.0-1_x64_en-US.msi | Format-List Status, SignerCertificate
```

## 4. Rolling back

If a release is broken after publishing:

1. Mark the release as a pre-release in the GitHub UI (removes it from
   `releases/latest`).
2. Publish a patch release (`v0.1.1`) with the fix.
3. Do not delete broken artifacts; downstream users may have in-flight
   downloads. Let the patch version supersede it.

## 5. Troubleshooting

### 5.1 `sign-macos` finds no Developer ID identity

`APPLE_CERTIFICATE` or `APPLE_INSTALLER_CERTIFICATE` is set but the identity
never reached the build keychain. Check the matching `_PASSWORD` secret and
that the secret holds a base64 Developer ID `.p12` of the right kind:
Application for the app and the `.dmg`, Installer for the `.pkg`.

### 5.2 `sign-updater` names a missing file

`sign-updater` and `publish` take an exact set of files, named in
`.github/scripts/release_assets.py`, and stop on any file missing or not part
of it. A missing installer means its build leg or `sign-macos` did not upload
it; re-run the failed job. `build_latest_json.py` then requires all five
`latest.json` keys (`darwin-aarch64`, `darwin-x86_64`, `windows-x86_64`,
`linux-x86_64`, `linux-x86_64-deb`), so a partial manifest cannot publish. A
new bundle type goes into `release_assets.py` before it can ship.

### 5.3 Draft release already exists for the tag

The `publish` job detects this and updates the existing draft instead of
failing. Safe to re-run after a failed build.

### 5.4 `sign-updater` says a file "is signed with key X, and installed copies trust key Y"

The key in the `release` environment is not the key of the latest published
release. Put the right key back, or finish the key move in 1.5 if one is under
way. `TAURI_SIGNING_PRIVATE_KEY` unset or its password wrong fails the signing
step itself, naming minisign's error.

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
- `.github/scripts/release_notes.py`      draft release notes and their range
- `.github/scripts/build_latest_json.py`  updater manifest builder
- `.github/scripts/release_assets.py`     the exact set of files a release publishes
- `.github/scripts/release_signing.py`    updater and checksum signatures, and their checks
- `.github/scripts/archive_app.py`        macOS updater archive of the signed app
- `.github/scripts/windows_signing.py`    signCommand config for Azure Artifact Signing
- `scripts/macos-release.sh`              macOS signing, notarization and dmg repack
- `scripts/install-minisign.sh`           pinned minisign for CI and the release
- `scripts/release-preflight.sh`          local gates to run before tagging
- `deny.toml`                             advisory, licence, ban and source policy
- `rust-toolchain.toml`                   the pinned compiler
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
