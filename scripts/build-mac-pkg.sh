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
#     - Signs the installer when WRIT_INSTALLER_SIGNING_IDENTITY is set
#       (e.g. "Developer ID Installer: Ibrahem Mahyob (5C6Y52822Q)")
#
# Requirements:
#   pkgbuild, productbuild, productsign (ship with macOS)
#   The Tauri build must have already produced Writ.app

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

VERSION="$(awk -F'"' '/^version[[:space:]]*=/{print $2; exit}' Cargo.toml)"
[ -n "$VERSION" ] || { echo "could not read version from Cargo.toml" >&2; exit 1; }

# Resolve the real target dir: a local .cargo/config.toml or CARGO_TARGET_DIR
# override moves it away from ./target.
TARGET_DIR="$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"

APP_PATH="${TARGET_DIR}/universal-apple-darwin/release/bundle/macos/Writ.app"
[ -d "$APP_PATH" ] || { echo "no .app at $APP_PATH; run cargo tauri build first" >&2; exit 1; }

OUT_DIR="${TARGET_DIR}/universal-apple-darwin/release/bundle/macos"
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

# The component package goes to a scratch dir, not next to $OUT: release.yml
# globs Writ_*_universal.pkg in the bundle dir and a second match breaks it.
mkdir -p "$WORK/component"
pkgbuild \
  --root "$WORK/root" \
  --identifier com.writ.editor \
  --version "$VERSION" \
  --install-location / \
  --scripts "$WORK/scripts" \
  "$WORK/component/Writ.pkg"

# A pkgbuild component package cannot carry an OS requirement. productbuild
# with a Distribution file can, and MIN_OS is the floor the Homebrew cask
# already declares (packaging/homebrew/Casks/writ.rb: depends_on macos).
MIN_OS="12.0"
cat > "$WORK/distribution.xml" <<DISTRIBUTION
<?xml version="1.0" encoding="utf-8"?>
<installer-gui-script minSpecVersion="2">
    <title>Writ</title>
    <options customize="never" require-scripts="false" hostArchitectures="arm64,x86_64"/>
    <allowed-os-versions>
        <os-version min="${MIN_OS}"/>
    </allowed-os-versions>
    <choices-outline>
        <line choice="com.writ.editor"/>
    </choices-outline>
    <choice id="com.writ.editor" visible="false">
        <pkg-ref id="com.writ.editor"/>
    </choice>
    <pkg-ref id="com.writ.editor" version="${VERSION}" onConclusion="none">Writ.pkg</pkg-ref>
</installer-gui-script>
DISTRIBUTION

productbuild \
  --distribution "$WORK/distribution.xml" \
  --package-path "$WORK/component" \
  "$WORK/unsigned.pkg"

if [ -n "${WRIT_INSTALLER_SIGNING_IDENTITY:-}" ]; then
  productsign --sign "$WRIT_INSTALLER_SIGNING_IDENTITY" "$WORK/unsigned.pkg" "$OUT"
  if ! pkgutil --check-signature "$OUT"; then
    echo "productsign produced a package pkgutil will not accept" >&2
    exit 1
  fi
else
  cp "$WORK/unsigned.pkg" "$OUT"
  echo "WRIT_INSTALLER_SIGNING_IDENTITY unset: installer is unsigned, Gatekeeper will reject it" >&2
fi

echo
echo "wrote $OUT"
ls -lh "$OUT"
