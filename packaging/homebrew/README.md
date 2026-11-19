# Jotra Homebrew Cask

This directory holds the Homebrew Cask formula for Jotra. Jotra ships through a self-hosted tap, so release cadence does not wait on `homebrew/homebrew-cask` review cycles. Upstream submission comes later.

## Self-hosted tap layout

The tap lives at a dedicated repository: `github.com/ibrahemid/homebrew-writ`.

```
homebrew-writ/
  README.md
  cask_renames.json
  Casks/
    jotra.rb
```

`Casks/jotra.rb` and `cask_renames.json` in the tap repo are copies of `packaging/homebrew/Casks/jotra.rb` and `packaging/homebrew/cask_renames.json` in this repo. `cask_renames.json` maps each retired cask token to its replacement, so `brew update` moves existing installs to the new token; the cask file of a retired token is deleted from the tap. The post-release workflow at `.github/workflows/packages.yml` keeps the copy in this repo up to date; publishing to the tap is a manual copy step, with no push action wired up yet.

## Verifying the tap

The tap is published and serves the current release. To check it end to end:

```sh
brew tap ibrahemid/writ
brew install --cask jotra
open -a Jotra
brew uninstall --cask jotra
brew untap ibrahemid/writ
```

## Updating the tap on a new release

The post-release workflow rewrites `packaging/homebrew/Casks/jotra.rb` in this repo with the new version and SHA256 values and opens a PR. After the PR merges:

1. `cp packaging/homebrew/Casks/jotra.rb ../homebrew-writ/Casks/jotra.rb`
2. `cp packaging/homebrew/cask_renames.json ../homebrew-writ/cask_renames.json`
3. `cd ../homebrew-writ && git add Casks/jotra.rb cask_renames.json && git commit -m "jotra $VERSION" && git push`

A follow-up enhancement will push directly from the workflow using a deploy key.

## End-user install

```sh
brew tap ibrahemid/writ
brew install --cask jotra
```

## Submitting to homebrew/homebrew-cask (later)

Eligibility requires:
- A stable release line (a window on the 0.2.x line).
- The artifact must be downloadable from a public URL with no authentication.
- The binary must be signed and notarized for macOS.

When ready, follow the [Homebrew Cask Cookbook](https://docs.brew.sh/Cask-Cookbook) and open a PR against `homebrew/homebrew-cask` copying `jotra.rb` to `Casks/w/jotra.rb`.

## The SHA field

The cask installs one artifact for both architectures, the universal
`Jotra_<version>_universal.pkg`, so it carries a single `sha256`. Before a
release fills it in that field holds the placeholder `__SHA256_UNIVERSAL__`.

`.github/workflows/packages.yml` rewrites the version and that field on
`release: published`, delegating to `scripts/packaging_bump_homebrew.py`, which
rewrites one `sha256` field and exits non-zero if it finds none.
