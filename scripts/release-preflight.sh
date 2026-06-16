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
#   - pnpm (already installed for normal dev)
#   - Docker (only required for the act-driven Linux dry run)
#   - act (https://github.com/nektos/act, optional; install with `brew install act`)
#
# Tauri updater signing:
#   The mac build needs TAURI_SIGNING_PRIVATE_KEY and
#   TAURI_SIGNING_PRIVATE_KEY_PASSWORD to produce the .sig used by the
#   in-app updater. This script reads:
#     - the key from $TAURI_SIGNING_PRIVATE_KEY_PATH (default
#       ~/.tauri/writ.key);
#     - the password from the macOS Keychain item
#       service="writ-tauri-signing" account="$USER", or from
#       $TAURI_SIGNING_PRIVATE_KEY_PASSWORD if pre-set.
#   First-time setup (one shot, paste the password when prompted):
#     security add-generic-password -a "$USER" -s "writ-tauri-signing" -w

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "${ROOT}"

step() { printf '\n\033[1;36m==> %s\033[0m\n' "$*"; }
warn() { printf '\033[1;33m!! %s\033[0m\n' "$*"; }
fail() { printf '\033[1;31mxx %s\033[0m\n' "$*" >&2; exit 1; }

load_signing_secrets() {
  local key_path="${TAURI_SIGNING_PRIVATE_KEY_PATH:-${HOME}/.tauri/writ.key}"
  if [[ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" ]]; then
    if [[ -f "${key_path}" ]]; then
      TAURI_SIGNING_PRIVATE_KEY="$(cat "${key_path}")"
      export TAURI_SIGNING_PRIVATE_KEY
    else
      fail "No TAURI_SIGNING_PRIVATE_KEY env and no key at ${key_path}.
       Generate one with: npx tauri signer generate -w ~/.tauri/writ.key"
    fi
  fi
  if [[ -z "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}" ]]; then
    if pw="$(security find-generic-password -a "${USER}" -s "writ-tauri-signing" -w 2>/dev/null)"; then
      TAURI_SIGNING_PRIVATE_KEY_PASSWORD="${pw}"
      export TAURI_SIGNING_PRIVATE_KEY_PASSWORD
    else
      fail "No TAURI_SIGNING_PRIVATE_KEY_PASSWORD env and no Keychain entry.
       Store it once: security add-generic-password -a \"\$USER\" -s \"writ-tauri-signing\" -w
       (you will be prompted for the password; nothing hits disk)"
    fi
  fi
}

step "1/6 cargo test --workspace"
cargo test --workspace

step "2/6 cargo clippy --workspace -- -D warnings"
cargo clippy --workspace -- -D warnings

step "3/6 npx tsc --noEmit"
npx tsc --noEmit

step "4/6 pnpm --dir site build"
pnpm --dir site build

step "5/6 cargo tauri build (universal mac .app + .dmg + .pkg, ad-hoc signed)"
if [[ "$(uname -s)" != "Darwin" ]]; then
  warn "Skipping mac build: this script is running on $(uname -s), not Darwin."
else
  load_signing_secrets
  rustup target add aarch64-apple-darwin x86_64-apple-darwin >/dev/null 2>&1 || true
  cargo build -p writ-cli --release --target aarch64-apple-darwin
  cargo build -p writ-cli --release --target x86_64-apple-darwin
  mkdir -p src-tauri/binaries
  lipo -create \
    target/aarch64-apple-darwin/release/writ \
    target/x86_64-apple-darwin/release/writ \
    -output src-tauri/binaries/writ-universal-apple-darwin
  APPLE_SIGNING_IDENTITY="-" \
    npx tauri build \
      --target universal-apple-darwin \
      --bundles app,dmg
  bash scripts/build-mac-pkg.sh
  echo
  echo "Mac artefacts:"
  find target/universal-apple-darwin/release/bundle -type f \( -name '*.pkg' -o -name '*.dmg' -o -name '*.tar.gz' -o -name '*.sig' \) 2>/dev/null | sort
fi

step "6/6 act --dryrun (Linux release leg)"
if ! command -v act >/dev/null 2>&1; then
  warn "act not installed; skipping Linux dry run."
  warn "Install with: brew install act"
  warn "Then run: act -W .github/workflows/release.yml -j build --matrix os:ubuntu-22.04 --container-architecture linux/amd64"
elif ! docker info >/dev/null 2>&1; then
  warn "Docker daemon not running; skipping Linux dry run."
  warn "Start Docker Desktop, then re-run this script."
else
  act -W .github/workflows/release.yml \
      -j build \
      --matrix os:ubuntu-22.04 \
      --container-architecture linux/amd64 \
      --dryrun
  echo
  warn "act --dryrun only validates the workflow shape, not the build itself."
  warn "For a full Linux build run drop --dryrun. It pulls a ~2 GB image and takes ~15 min."
fi

echo
echo "preflight: OK"
echo "If everything above is green, tag and push:"
echo "  git tag v\$(jq -r .version site/package.json)"
echo "  git push origin v\$(jq -r .version site/package.json)"
