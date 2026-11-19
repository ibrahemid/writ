#!/usr/bin/env bash
# Wrap the Tauri-built Jotra.app into a .pkg installer with pre/post-install
# scripts that quit any running Jotra before swapping the bundle and relaunch
# the new one. Solves the macOS Finder "item is in use" problem for users who
# install a fresh download instead of using the in-app updater.
#
# Usage:
#   scripts/build-mac-pkg.sh
#     - Reads version from Cargo.toml
#     - Reads .app from target/universal-apple-darwin/release/bundle/macos/Jotra.app,
#       or from JOTRA_APP_PATH when set
#     - Writes target/universal-apple-darwin/release/bundle/macos/Jotra_<version>_universal.pkg,
#       or into JOTRA_PKG_OUT_DIR when set
#
#     - Signs the installer when JOTRA_INSTALLER_SIGNING_IDENTITY is set
#       (e.g. "Developer ID Installer: Ibrahem Mahyob (5C6Y52822Q)")
#
# Requirements:
#   pkgbuild, productbuild, productsign (ship with macOS)
#   The Tauri build must have already produced Jotra.app

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

VERSION="$(awk -F'"' '/^version[[:space:]]*=/{print $2; exit}' Cargo.toml)"
[ -n "$VERSION" ] || { echo "could not read version from Cargo.toml" >&2; exit 1; }

if [ -z "${JOTRA_APP_PATH:-}" ] || [ -z "${JOTRA_PKG_OUT_DIR:-}" ]; then
  # Resolve the real target dir: a local .cargo/config.toml or CARGO_TARGET_DIR
  # override moves it away from ./target.
  TARGET_DIR="$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
fi

APP_PATH="${JOTRA_APP_PATH:-${TARGET_DIR}/universal-apple-darwin/release/bundle/macos/Jotra.app}"
[ -d "$APP_PATH" ] || { echo "no .app at $APP_PATH; run cargo tauri build first" >&2; exit 1; }

OUT_DIR="${JOTRA_PKG_OUT_DIR:-${TARGET_DIR}/universal-apple-darwin/release/bundle/macos}"
mkdir -p "$OUT_DIR"
OUT="${OUT_DIR}/Jotra_${VERSION}_universal.pkg"

WORK="$(mktemp -d -t jotra-pkg.XXXXXX)"
trap 'rm -rf "$WORK"' EXIT

mkdir -p "$WORK/root/Applications"
cp -R "$APP_PATH" "$WORK/root/Applications/"

mkdir -p "$WORK/scripts"

# Installer passes the target volume as $3.
cat > "$WORK/scripts/preinstall" <<'PREINSTALL'
#!/bin/bash
set -euo pipefail

VOLUME="${3:-/}"
APPLICATIONS="${VOLUME%/}/Applications"
APP="$APPLICATIONS/Jotra.app"
LEGACY_APP="$APPLICATIONS/Writ.app"
BUNDLE_ID="com.writ.editor"
PATTERN='Jotra\.app/Contents/MacOS/'
LEGACY_PATTERN='Writ\.app/Contents/MacOS/'

is_app_running() {
  pgrep -f "$PATTERN" >/dev/null 2>&1 || pgrep -f "$LEGACY_PATTERN" >/dev/null 2>&1
}

osascript -e "if application id \"$BUNDLE_ID\" is running then tell application id \"$BUNDLE_ID\" to quit" >/dev/null 2>&1 || true

HAS_QUIT=false
for _ in 1 2 3 4 5 6 7 8 9 10; do
  if ! is_app_running; then
    HAS_QUIT=true
    break
  fi
  sleep 0.3
done

if [ "$HAS_QUIT" != "true" ]; then
  pkill -9 -f "$PATTERN" 2>/dev/null || true
  pkill -9 -f "$LEGACY_PATTERN" 2>/dev/null || true
fi

if [ -d "$LEGACY_APP" ] && [ ! -L "$LEGACY_APP" ]; then
  LEGACY_ID="$(plutil -extract CFBundleIdentifier raw -o - "$LEGACY_APP/Contents/Info.plist" 2>/dev/null || true)"
  if [ "$LEGACY_ID" = "$BUNDLE_ID" ]; then
    if [ -e "$APP" ] || [ -L "$APP" ]; then
      rm -rf "$LEGACY_APP"
    else
      mv "$LEGACY_APP" "$APP"
    fi
  fi
fi
exit 0
PREINSTALL

cat > "$WORK/scripts/postinstall" <<'POSTINSTALL'
#!/bin/bash
set -euo pipefail

VOLUME="${3:-/}"
APPLICATIONS="${VOLUME%/}/Applications"
APP="$APPLICATIONS/Jotra.app"
LEGACY_APP="$APPLICATIONS/Writ.app"

if [ ! -e "$LEGACY_APP" ] && [ ! -L "$LEGACY_APP" ]; then
  if ln -s Jotra.app "$LEGACY_APP"; then
    chflags -h hidden "$LEGACY_APP" || echo "could not hide the legacy link $LEGACY_APP" >&2
  else
    echo "could not create the legacy link $LEGACY_APP" >&2
  fi
fi

# postinstall runs as root; the app opens as the person at the console.
CONSOLE_USER="$(stat -f '%Su' /dev/console)"
if [ -n "$CONSOLE_USER" ] && [ "$CONSOLE_USER" != "root" ]; then
  sudo -u "$CONSOLE_USER" open -a "$APP" 2>/dev/null || true
fi
exit 0
POSTINSTALL

cat > "$WORK/component.plist" <<'COMPONENT'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<array>
    <dict>
        <key>BundleHasStrictIdentifier</key>
        <true/>
        <key>BundleIsRelocatable</key>
        <false/>
        <key>BundleIsVersionChecked</key>
        <true/>
        <key>BundleOverwriteAction</key>
        <string>upgrade</string>
        <key>RootRelativeBundlePath</key>
        <string>Applications/Jotra.app</string>
    </dict>
</array>
</plist>
COMPONENT

chmod +x "$WORK/scripts/preinstall" "$WORK/scripts/postinstall"

# The component package goes to a scratch dir, not next to $OUT: the release
# publishes every file in JOTRA_PKG_OUT_DIR and refuses one it does not expect.
mkdir -p "$WORK/component"
pkgbuild \
  --root "$WORK/root" \
  --identifier com.writ.editor \
  --version "$VERSION" \
  --install-location / \
  --component-plist "$WORK/component.plist" \
  --scripts "$WORK/scripts" \
  "$WORK/component/Jotra.pkg"

# A pkgbuild component package cannot carry an OS requirement. productbuild
# with a Distribution file can, and MIN_OS is the floor the Homebrew cask
# already declares (packaging/homebrew/Casks/jotra.rb: depends_on macos).
MIN_OS="12.0"
cat > "$WORK/distribution.xml" <<DISTRIBUTION
<?xml version="1.0" encoding="utf-8"?>
<installer-gui-script minSpecVersion="2">
    <title>Jotra</title>
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
    <pkg-ref id="com.writ.editor" version="${VERSION}" onConclusion="none">Jotra.pkg</pkg-ref>
</installer-gui-script>
DISTRIBUTION

productbuild \
  --distribution "$WORK/distribution.xml" \
  --package-path "$WORK/component" \
  "$WORK/unsigned.pkg"

if [ -n "${JOTRA_INSTALLER_SIGNING_IDENTITY:-}" ]; then
  productsign --sign "$JOTRA_INSTALLER_SIGNING_IDENTITY" "$WORK/unsigned.pkg" "$OUT"
  if ! pkgutil --check-signature "$OUT"; then
    echo "productsign produced a package pkgutil will not accept" >&2
    exit 1
  fi
else
  cp "$WORK/unsigned.pkg" "$OUT"
  echo "JOTRA_INSTALLER_SIGNING_IDENTITY unset: installer is unsigned, Gatekeeper will reject it" >&2
fi

echo
echo "wrote $OUT"
ls -lh "$OUT"
