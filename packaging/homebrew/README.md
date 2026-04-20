# Writ Homebrew Cask

This directory holds the Homebrew Cask formula for Writ. On day one Writ ships through a self-hosted tap so we control the release cadence without waiting for `homebrew/homebrew-cask` review cycles. Once Writ has a stable track record we can submit to upstream.

## Self-hosted tap layout

The tap lives at a dedicated repository: `github.com/ibrahemid/homebrew-writ`.

```
homebrew-writ/
  README.md
  Casks/
    writ.rb
```

`Casks/writ.rb` in the tap repo is a copy of `packaging/homebrew/Casks/writ.rb` in this repo. The post-release workflow at `.github/workflows/packages.yml` keeps the copy in this repo up to date; publishing to the tap is currently a manual copy step until we wire up a push action.

## Publishing the tap (first time)

1. Create an empty public repo at `github.com/ibrahemid/homebrew-writ`.
2. Clone it locally.
3. Copy `packaging/homebrew/Casks/writ.rb` from this repo into `Casks/writ.rb` in the tap repo.
4. Commit and push.
5. Verify the tap works end to end:

   ```sh
   brew tap ibrahemid/writ
   brew install --cask writ
   open -a Writ
   brew uninstall --cask writ
   brew untap ibrahemid/writ
   ```

## Updating the tap on a new release

The post-release workflow rewrites `packaging/homebrew/Casks/writ.rb` in this repo with the new version and SHA256 values and opens a PR. After the PR merges:

1. `cp packaging/homebrew/Casks/writ.rb ../homebrew-writ/Casks/writ.rb`
2. `cd ../homebrew-writ && git add Casks/writ.rb && git commit -m "writ $VERSION" && git push`

A follow-up enhancement will push directly from the workflow using a deploy key.

## End-user install

```sh
brew tap ibrahemid/writ
brew install --cask writ
```

## Submitting to homebrew/homebrew-cask (later)

Eligibility requires:
- A stable release line (we suggest waiting past v0.2.0).
- The artifact must be downloadable from a public URL with no authentication.
- The binary must be signed and notarized for macOS.

When ready, follow the [Homebrew Cask Cookbook](https://docs.brew.sh/Cask-Cookbook) and open a PR against `homebrew/homebrew-cask` copying `writ.rb` to `Casks/w/writ.rb`.

## Placeholders

The cask in this repo uses placeholders:

- `__SHA256_ARM64__` for the Apple Silicon DMG
- `__SHA256_INTEL__` for the Intel DMG

The packages workflow replaces both on `release: published`.
