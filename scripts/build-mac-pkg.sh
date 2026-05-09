#!/usr/bin/env bash
# Wrap the Tauri-built Writ.app into a .pkg installer with pre/post-install
# scripts that quit any running Writ before swapping the bundle and relaunch
# the new one. Solves the macOS Finder "item is in use" problem for users who
# install a fresh download instead of using the in-app updater.
#
# Usage:
#   scripts/build-mac-pkg.sh
#     - Reads version from Cargo.toml
#     - Reads .app from target/universal-apple-darwin/release/bundle/macos/Writ.app
#     - Writes target/universal-apple-darwin/release/bundle/macos/Writ_<version>_universal.pkg
#
# Requirements:
#   pkgbuild (ships with macOS)
#   The Tauri build must have already produced Writ.app

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

VERSION="$(awk -F'"' '/^version[[:space:]]*=/{print $2; exit}' Cargo.toml)"
[ -n "$VERSION" ] || { echo "could not read version from Cargo.toml" >&2; exit 1; }

APP_PATH="target/universal-apple-darwin/release/bundle/macos/Writ.app"
[ -d "$APP_PATH" ] || { echo "no .app at $APP_PATH; run cargo tauri build first" >&2; exit 1; }

OUT_DIR="target/universal-apple-darwin/release/bundle/macos"
OUT="${OUT_DIR}/Writ_${VERSION}_universal.pkg"

WORK="$(mktemp -d -t writ-pkg.XXXXXX)"
trap 'rm -rf "$WORK"' EXIT

mkdir -p "$WORK/root/Applications"
cp -R "$APP_PATH" "$WORK/root/Applications/"

mkdir -p "$WORK/scripts"

cat > "$WORK/scripts/preinstall" <<'PREINSTALL'
#!/bin/bash
# Ask Writ to quit gracefully, wait, then force-kill anything left.
osascript -e 'tell application "Writ" to quit' 2>/dev/null || true

for _ in 1 2 3 4 5 6 7 8 9 10; do
  if ! pgrep -f "Writ.app/Contents/MacOS/writ" >/dev/null 2>&1; then
    exit 0
  fi
  sleep 0.3
done

pkill -9 -f "Writ.app/Contents/MacOS/writ" 2>/dev/null || true
exit 0
PREINSTALL

cat > "$WORK/scripts/postinstall" <<'POSTINSTALL'
#!/bin/bash
# Relaunch Writ as the user who triggered the install (postinstall runs as root).
LOGGED_IN_USER="$(stat -f '%Su' /dev/console)"
if [ -n "$LOGGED_IN_USER" ] && [ "$LOGGED_IN_USER" != "root" ]; then
  sudo -u "$LOGGED_IN_USER" open -a "/Applications/Writ.app" 2>/dev/null || true
fi
exit 0
POSTINSTALL

chmod +x "$WORK/scripts/preinstall" "$WORK/scripts/postinstall"

pkgbuild \
  --root "$WORK/root" \
  --identifier com.writ.editor \
  --version "$VERSION" \
  --install-location / \
  --scripts "$WORK/scripts" \
  "$OUT"

echo
echo "wrote $OUT"
ls -lh "$OUT"
