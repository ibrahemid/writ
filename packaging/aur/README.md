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
https://github.com/ibrahemid/writ/releases/download/v<version>/writ_<version>_amd64.AppImage
```

It then extracts the AppImage under `/opt/writ/` and symlinks `/usr/bin/writ` to the `AppRun` entrypoint. The AppImage is not executed at install time, only unpacked, so no FUSE runtime is required for `pacman -S writ-bin` to succeed. `fuse2` is still listed as a runtime dependency because the app launcher re-executes `AppRun`, which many Tauri AppImages expect to run under a FUSE loopback.

## Placeholders

The PKGBUILD uses one placeholder that the post-release workflow rewrites:

- `__SHA256_APPIMAGE__` replaced with the SHA256 of the released AppImage.

The `LICENSE` source is pinned with `SKIP` because GitHub serves the raw LICENSE file over HTTP without a stable content hash we can pin; auditing is done at the `v<version>` ref level via the `source` URL itself.

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

## Submitting to AUR

AUR submission is a one-time setup. After that every release is a `git push` to the AUR git remote.

1. Create an account at https://aur.archlinux.org.
2. Upload an SSH public key to your AUR profile. Only ed25519 and RSA 4096+ keys are accepted.
3. Confirm the package name is available:
   ```sh
   ssh aur@aur.archlinux.org list-repos
   ```
4. Clone the empty package repo (this creates it on first push):
   ```sh
   git clone ssh://aur@aur.archlinux.org/writ-bin.git aur-writ-bin
   ```
5. Copy the PKGBUILD and generated `.SRCINFO` into the clone:
   ```sh
   cp packaging/aur/writ-bin/PKGBUILD aur-writ-bin/PKGBUILD
   cd aur-writ-bin
   makepkg --printsrcinfo > .SRCINFO
   ```
6. Commit and push:
   ```sh
   git add PKGBUILD .SRCINFO
   git commit -m "writ-bin 0.1.0-1: initial release"
   git push origin master
   ```

## Updating on a new release

The post-release workflow `.github/workflows/packages.yml` rewrites `pkgver`, `sha256sums`, and `.SRCINFO` in this repo and opens a PR with the bumps. After the PR merges, push the updated `PKGBUILD` and `.SRCINFO` to the AUR remote:

```sh
cp packaging/aur/writ-bin/PKGBUILD aur-writ-bin/PKGBUILD
cp packaging/aur/writ-bin/.SRCINFO aur-writ-bin/.SRCINFO  # if present
cd aur-writ-bin
git add PKGBUILD .SRCINFO
git commit -m "writ-bin <version>-1"
git push origin master
```

A follow-up improvement is to push directly from the workflow using a deploy key registered with the AUR account.
