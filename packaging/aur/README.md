# Writ AUR Package

This directory holds the Arch User Repository (AUR) recipe for Writ. End users on Arch and its derivatives install Writ via an AUR helper:

```sh
yay -S writ-bin
# or
paru -S writ-bin
```

## Package layout

```
packaging/aur/writ-bin/
  PKGBUILD    # Arch build recipe pulling the official AppImage from GitHub Releases
```

Package name: `writ-bin`. The `-bin` suffix follows AUR convention for packages that ship a prebuilt binary rather than compiling from source. If we add a from-source package later it will live at `packaging/aur/writ/`.

## Source strategy

The PKGBUILD pulls the official AppImage from GitHub Releases:

```
https://github.com/ibrahemid/writ/releases/download/v<version>/Writ_<version>_amd64.AppImage
```

`prepare()` first checks the AppImage's minisign signature (the release's `Writ_<version>_amd64.AppImage.sig`, saved as `writ-<version>.AppImage.minisign` so makepkg does not take it for a PGP signature) against the release key in `_minisign_pubkey`, which needs `minisign` at build time. It then extracts the AppImage under `/opt/writ/` and installs `/usr/bin/writ` as a `/bin/sh` wrapper that execs the CLI from the payload (`/opt/writ/usr/bin/writ`) with `WRIT_GUI_BIN` defaulting to `/opt/writ/AppRun`. The CLI therefore handles `writ file.rs` and `... | writ`, while the app starts through `AppRun`, which sets up the bundled GTK environment the payload expects. The desktop entry launches `AppRun` directly. `/opt/writ/.package-manager` (containing `pacman`) tells Writ that pacman owns the install, so the app reports a new release with "Update with your package manager" instead of downloading it. The AppImage is not executed at install time, only unpacked, so no FUSE runtime is required for `pacman -S writ-bin` to succeed. `fuse2` is still listed as a runtime dependency because the app launcher re-executes `AppRun`, which many Tauri AppImages expect to run under a FUSE loopback.

## Checksums

`sha256sums` pins the AppImage and the `LICENSE` at the release tag; the signature file is `SKIP`, since minisign checks it against the key. The post-release workflow rewrites both hashes: the AppImage's from the release asset, the licence's from `git show v<version>:LICENSE`.

## Local validation

On an Arch host with `base-devel` installed:

```sh
cd packaging/aur/writ-bin
makepkg -si --noconfirm
writ --version
sudo pacman -Rns writ-bin
```

To lint the PKGBUILD:

```sh
namcap PKGBUILD
```

## Generating `.SRCINFO`

AUR requires a `.SRCINFO` file alongside the PKGBUILD. Regenerate it whenever the PKGBUILD changes:

```sh
cd packaging/aur/writ-bin
makepkg --printsrcinfo > .SRCINFO
```

The post-release workflow regenerates `.SRCINFO` as part of the bump PR when it updates the version and SHA.

## Submission

Done. `writ-bin` is published at https://aur.archlinux.org/packages/writ-bin, first pushed at 0.2.0-1. The account key lives at `~/.ssh/aur_ed25519` with a matching `Host aur.archlinux.org` entry in `~/.ssh/config`.

## Updating on a new release

The post-release workflow `.github/workflows/packages.yml` rewrites `pkgver`, `sha256sums`, and `.SRCINFO` in this repo and opens a PR with the bumps. After the PR merges, push the updated files to the AUR remote:

```sh
git clone ssh://aur@aur.archlinux.org/writ-bin.git aur-writ-bin
install -m644 packaging/aur/writ-bin/PKGBUILD packaging/aur/writ-bin/.SRCINFO aur-writ-bin/
cd aur-writ-bin
git add PKGBUILD .SRCINFO
git commit -m "writ-bin <version>-1"
git push origin HEAD:master
```

AUR only accepts `master`, and a fresh clone checks out whatever `init.defaultBranch` says, so push `HEAD:master` rather than `master`. Copy the two files by name; `packaging/aur/` carries a `.DS_Store`. The AUR server validates `.SRCINFO` against the `PKGBUILD` on push and rejects the push if they disagree, which is what makes the script-generated `.SRCINFO` safe to ship without an Arch host.

A follow-up improvement is to push directly from the workflow using a deploy key registered with the AUR account.
