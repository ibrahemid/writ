#!/usr/bin/env bash
set -euo pipefail

# Run all the same checks release.yml runs, locally, before tagging.
# Catches mac and Linux failures for free; Windows still requires CI or a VM.
#
# Usage: scripts/release-preflight.sh
#
# Requirements:
#   - macOS host (for the mac .app + .pkg build)
#   - Rust toolchain (already installed for normal dev)
#   - pnpm (already installed for normal dev), with the root node_modules
#     installed: pnpm install --frozen-lockfile
#   - cargo-about, for the third-party notices check:
#       cargo install cargo-about --version 0.9.1 --locked --features cli
#   - cargo-deny, for the advisory, licence, ban and source checks:
#       cargo install cargo-deny --version 0.20.2 --locked
#   - minisign 0.12 (brew install minisign), which signs the updater archive
#   - gh, signed in, to read the updater key of the latest published release
#   - Docker (only required for the act dry run of the release job graph)
#   - act (https://github.com/nektos/act, optional; install with `brew install act`)
#
# Tauri updater signing:
#   Gate 9 signs the mac updater archive the way the sign-updater job does and
#   checks it against the key installed copies trust, so it needs the updater
#   key and its password. This script reads:
#     - the key from $TAURI_SIGNING_PRIVATE_KEY if set, else from the file at
#       $TAURI_SIGNING_PRIVATE_KEY_PATH (default ~/.tauri/writ.key);
#     - the password from $TAURI_SIGNING_PRIVATE_KEY_PASSWORD if set, else from
#       the macOS Keychain item service="writ-tauri-signing" account="$USER".
#   It takes these variables out of the environment before anything runs and
#   hands the key and password to the signing command alone, so no build
#   script, proc macro or package script below can read them.
#   First-time setup (one shot, paste the password when prompted):
#     security add-generic-password -a "$USER" -s "writ-tauri-signing" -w

declare +x UPDATER_KEY="${TAURI_SIGNING_PRIVATE_KEY:-}"
declare +x UPDATER_KEY_PASSWORD="${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}"
declare +x UPDATER_KEY_PATH="${TAURI_SIGNING_PRIVATE_KEY_PATH:-${HOME}/.tauri/writ.key}"
unset TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD TAURI_SIGNING_PRIVATE_KEY_PATH

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "${ROOT}"

VERSION="$(awk -F'"' '/^version[[:space:]]*=/{print $2; exit}' Cargo.toml)"

step() { printf '\n\033[1;36m==> %s\033[0m\n' "$*"; }
warn() { printf '\033[1;33m!! %s\033[0m\n' "$*"; }
fail() { printf '\033[1;31mxx %s\033[0m\n' "$*" >&2; exit 1; }

load_updater_key() {
  if [[ -z "${UPDATER_KEY}" ]]; then
    [[ -f "${UPDATER_KEY_PATH}" ]] || fail "No TAURI_SIGNING_PRIVATE_KEY env and no key at ${UPDATER_KEY_PATH}.
       Generate one with: npx tauri signer generate -w ~/.tauri/writ.key"
    UPDATER_KEY="${UPDATER_KEY_PATH}"
  fi
  if [[ -z "${UPDATER_KEY_PASSWORD}" ]]; then
    UPDATER_KEY_PASSWORD="$(security find-generic-password -a "${USER}" -s "writ-tauri-signing" -w 2>/dev/null)" ||
      fail "No TAURI_SIGNING_PRIVATE_KEY_PASSWORD env and no Keychain entry.
       Store it once: security add-generic-password -a \"\$USER\" -s \"writ-tauri-signing\" -w
       (you will be prompted for the password; nothing hits disk)"
  fi
}

# Installed copies verify an update against the key of the latest published
# release, so a key transition release is signed with that key while carrying
# the next one (docs/RELEASING.md 1.5).
trusted_updater_config() {
  local out="$1" latest
  latest="$(gh release view --json tagName --jq .tagName)" ||
    fail "could not read the latest release with gh; run gh auth login"
  gh api -H "Accept: application/vnd.github.raw+json" \
    "repos/{owner}/{repo}/contents/src-tauri/tauri.conf.json?ref=${latest}" > "${out}" ||
    fail "could not read src-tauri/tauri.conf.json at ${latest}"
  echo "installed copies trust the updater key of ${latest}"
}

# tauri-build copies the sidecar in src-tauri/binaries over target/debug/writ
# every time src-tauri compiles, so a stale sidecar makes writ-cli's version
# test run last release's binary. Rebuild the sidecars before anything else
# compiles src-tauri.
refresh_mac_sidecars() {
  rustup target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null 2>&1 || true
  # Resolve the real target dir: a local .cargo/config.toml or CARGO_TARGET_DIR
  # override moves it away from ./target.
  TARGET_DIR="$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
  cargo build -p writ-cli --release --locked --target aarch64-apple-darwin
  cargo build -p writ-cli --release --locked --target x86_64-apple-darwin
  mkdir -p src-tauri/binaries
  # The universal target builds each arch separately; every pass resolves the
  # sidecar by its own triple, so all three names must exist.
  cp "${TARGET_DIR}/aarch64-apple-darwin/release/writ" src-tauri/binaries/writ-aarch64-apple-darwin
  cp "${TARGET_DIR}/x86_64-apple-darwin/release/writ" src-tauri/binaries/writ-x86_64-apple-darwin
  lipo -create \
    "${TARGET_DIR}/aarch64-apple-darwin/release/writ" \
    "${TARGET_DIR}/x86_64-apple-darwin/release/writ" \
    -output src-tauri/binaries/writ-universal-apple-darwin
}

if [[ "$(uname -s)" == "Darwin" ]]; then
  load_updater_key
fi

step "0/10 rebuild the mac sidecars"
if [[ "$(uname -s)" != "Darwin" ]]; then
  warn "Skipping sidecar rebuild: this script is running on $(uname -s), not Darwin."
else
  refresh_mac_sidecars
fi

step "1/10 cargo deny --locked check advisories bans licenses sources"
cargo deny --locked check advisories bans licenses sources

step "2/10 cargo fmt --all --check"
cargo fmt --all --check

step "3/10 cargo test --workspace --locked"
cargo test --workspace --locked

step "4/10 cargo clippy --workspace --locked -- -D warnings"
cargo clippy --workspace --locked -- -D warnings

step "5/10 npx tsc --noEmit"
npx tsc --noEmit

# Runs before the bundle so the notices copied into the installers are the ones
# that were checked. The check needs cargo-about and the root node_modules; a
# missing prerequisite fails rather than skips, or the file rots unnoticed.
step "6/10 scripts/gen-third-party-notices.py --check"
python3 scripts/gen-third-party-notices.py --check

# The release workflow fails a tag build whose committed release.json names
# another version, so it is worth knowing before the tag is pushed.
step "7/10 node scripts/check-release-json.mjs"
node scripts/check-release-json.mjs

step "8/10 pnpm --dir site build"
pnpm --dir site build

step "9/10 mac release: unsigned build, then the sign-macos and sign-updater steps with an ad-hoc identity"
if [[ "$(uname -s)" != "Darwin" ]]; then
  warn "Skipping mac build: this script is running on $(uname -s), not Darwin."
else
  command -v minisign >/dev/null 2>&1 || fail "minisign not installed; brew install minisign"
  BUILT_DMG="${TARGET_DIR}/universal-apple-darwin/release/bundle/dmg/Writ_${VERSION}_universal.dmg"
  OUT_DIR="${TARGET_DIR}/release-preflight"
  APP_ARCHIVE="${OUT_DIR}/assets/Writ_${VERSION}_universal.app.tar.gz"
  APPLE_SIGNING_IDENTITY="-" \
    npx tauri build \
      --target universal-apple-darwin \
      --bundles app,dmg \
      --config '{"bundle":{"createUpdaterArtifacts":false}}' \
      -- --locked
  rm -rf "${OUT_DIR}"
  mkdir -p "${OUT_DIR}/assets"
  bash scripts/macos-release.sh extract-app "${BUILT_DMG}" "${OUT_DIR}/Writ.app"
  bash scripts/macos-release.sh sign-app "${OUT_DIR}/Writ.app" -
  WRIT_APP_PATH="${OUT_DIR}/Writ.app" WRIT_PKG_OUT_DIR="${OUT_DIR}/assets" bash scripts/build-mac-pkg.sh
  bash scripts/macos-release.sh repack-dmg "${BUILT_DMG}" "${OUT_DIR}/Writ.app" "${OUT_DIR}/assets/Writ_${VERSION}_universal.dmg"
  python3 .github/scripts/archive_app.py create "${OUT_DIR}/Writ.app" "${APP_ARCHIVE}"
  python3 .github/scripts/archive_app.py check "${APP_ARCHIVE}" Writ.app
  TAURI_SIGNING_PRIVATE_KEY="${UPDATER_KEY}" TAURI_SIGNING_PRIVATE_KEY_PASSWORD="${UPDATER_KEY_PASSWORD}" \
    python3 .github/scripts/release_signing.py sign-payloads --version "${VERSION}" "${APP_ARCHIVE}"
  trusted_updater_config "${OUT_DIR}/trusted-tauri.conf.json"
  python3 .github/scripts/release_signing.py verify-payloads --version "${VERSION}" \
    --pubkey-from "${OUT_DIR}/trusted-tauri.conf.json" "${APP_ARCHIVE}"
  echo
  echo "Mac artefacts:"
  ls -la "${OUT_DIR}/assets"
fi

step "10/10 act --dryrun (the release job graph)"
if ! command -v act >/dev/null 2>&1; then
  warn "act not installed; skipping the dry run."
  warn "Install with: brew install act"
elif ! docker info >/dev/null 2>&1; then
  warn "Docker daemon not running; skipping the dry run."
  warn "Start Docker Desktop, then re-run this script."
else
  ACT_EVENT="$(mktemp -t writ-act-event.XXXXXX)"
  trap 'rm -f "${ACT_EVENT}"' EXIT
  printf '{"ref":"refs/tags/v%s","ref_name":"v%s"}\n' "${VERSION}" "${VERSION}" > "${ACT_EVENT}"
  # The macOS and Windows jobs map to a Linux image only because --dryrun runs
  # nothing. Never map them to -self-hosted: act then runs the steps on this
  # Mac even under --dryrun.
  act push -W .github/workflows/release.yml \
      -e "${ACT_EVENT}" \
      -P ubuntu-latest=catthehacker/ubuntu:act-latest \
      -P ubuntu-22.04=catthehacker/ubuntu:act-22.04 \
      -P macos-latest=catthehacker/ubuntu:act-latest \
      -P windows-latest=catthehacker/ubuntu:act-latest \
      --container-architecture linux/amd64 \
      --dryrun
  echo
  warn "act --dryrun only validates the workflow shape and job graph, not the build."
  warn "sign-windows shows as skipped: its condition reads an output only a real run sets."
fi

echo
echo "preflight: OK"
echo "If everything above is green, tag and push:"
echo "  git tag v\$(jq -r .version site/package.json)"
echo "  git push origin v\$(jq -r .version site/package.json)"
