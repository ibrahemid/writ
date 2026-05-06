# Writ Distribution Channels

This directory holds everything the Writ project needs to publish releases across platform-specific package managers.

## One-liner for users

| Platform | Install command |
|---|---|
| macOS | `brew install --cask ibrahemid/writ/writ` |
| Windows | `winget install --id ibrahemid.Writ -e` |
| Arch Linux | `yay -S writ-bin` (or any AUR helper) |
| Any Linux | `curl -fsSL https://github.com/ibrahemid/writ/raw/main/install.sh \| sh` |

## Layout

```
packaging/
  homebrew/                   # Homebrew cask (self-hosted tap)
    Casks/writ.rb
    README.md
  winget/                     # winget manifests for microsoft/winget-pkgs
    manifests/i/ibrahemid/Writ/0.1.0/
    README.md
  aur/                        # Arch User Repository package
    writ-bin/
      PKGBUILD
    README.md
```

Each subdirectory has its own README covering the channel-specific submission process. Start there if you are publishing a channel for the first time.

## Current status (v0.1.0)

| Channel | Status | Notes |
|---|---|---|
| Homebrew (self-hosted tap) | Ready to publish | Tap lives at `github.com/ibrahemid/homebrew-writ`. First publish is a manual copy-and-push; subsequent publishes are triggered by the post-release workflow. |
| Homebrew upstream | Deferred | Eligible after v0.2.0+ stability window and after macOS notarization is live. |
| winget | Pending upstream submission | Manifests produced in this repo. The first PR into `microsoft/winget-pkgs` is manual. |
| AUR `writ-bin` | Pending first push | Requires a one-time SSH key upload to aur.archlinux.org and initial `git push` to the AUR remote. |
| Flatpak | Deferred to v0.2.0+ | Flathub submission requires a stable release line and a Flatpak manifest (`org.writ.Writ.yaml`) tracked separately. Revisit once Linux distribution feedback justifies the maintenance cost. |
| Snap | Deferred to v0.2.0+ | Snap requires a `snapcraft.yaml` and a Snapcraft store account. Lower priority than Flatpak because most Arch and Debian users prefer AUR and AppImage respectively. |

## Artifact names (source of truth)

The post-release workflow expects these artifact names on GitHub Releases. They match the Tauri v2 default bundler output and the CI in `.github/workflows/release.yml`.

| Artifact | Name |
|---|---|
| macOS Apple Silicon DMG | `Writ_<version>_aarch64.dmg` |
| macOS Intel DMG | `Writ_<version>_x64.dmg` |
| Windows x64 MSI | `Writ_<version>_x64_en-US.msi` |
| Linux AppImage | `Writ_<version>_amd64.AppImage` |
| Linux deb | `Writ_<version>_amd64.deb` |
| Checksums | `SHA256SUMS.txt` |
| Tauri updater manifest | `latest.json` |

If these filenames change, update `.github/workflows/packages.yml` and the three bump scripts in `scripts/`.

## Post-release bump workflow

`.github/workflows/packages.yml` fires on `release: published`. It:

1. Downloads the release artifacts from GitHub Releases using `gh release download`.
2. Computes `sha256sum` for each artifact that a manifest needs.
3. Rewrites versions and SHAs in:
   - `packaging/homebrew/Casks/writ.rb`
   - `packaging/winget/manifests/i/ibrahemid/Writ/<new-version>/` (copied from the previous version folder)
   - `packaging/aur/writ-bin/PKGBUILD` and regenerates `.SRCINFO`
4. Opens a PR titled `chore(packaging): bump distribution manifests to v<version>` on the `packages/bump-<version>` branch.

The PR is NOT auto-merged. A human reviews the numbers before merging. After the PR merges, each channel has a short manual push step described in its own README.

The workflow also accepts `workflow_dispatch` with a `tag` input for replays and for bumping against a past release if the automatic run failed.

## Rewrite scripts

The workflow delegates rewriting to three standalone scripts so they are easy to run locally for dry runs and unit-testable:

- `scripts/packaging_bump_homebrew.py`
- `scripts/packaging_bump_winget.py`
- `scripts/packaging_bump_aur.py`

All three read `VERSION` and the relevant `SHA_*` values from environment variables. The winget script additionally reads `RELEASE_DATE`. See each script's docstring for contract details.

Local dry run example:

```sh
VERSION=0.1.0 \
SHA_ARM64=0000000000000000000000000000000000000000000000000000000000000000 \
SHA_INTEL=0000000000000000000000000000000000000000000000000000000000000000 \
python3 scripts/packaging_bump_homebrew.py
```

## Placeholders in source-controlled manifests

Each manifest ships with placeholders rather than real SHAs so the files are readable in the repo and the first release cycle starts clean.

| File | Placeholder | Replaced by |
|---|---|---|
| `packaging/homebrew/Casks/writ.rb` | `__SHA256_ARM64__`, `__SHA256_INTEL__` | `packages.yml` |
| `packaging/winget/.../installer.yaml` | `__SHA256_MSI__` | `packages.yml` |
| `packaging/aur/writ-bin/PKGBUILD` | `__SHA256_APPIMAGE__` | `packages.yml` |

Replacing these by hand for the first release is also supported; just run the dry-run command above and commit the result.
