#!/usr/bin/env bash
# Installs minisign on a Linux x64 runner into <dir> and prints the binary's
# path. The release signs with it and CI tests the signing script with it.
#
# Usage: scripts/install-minisign.sh <dir>

set -euo pipefail

MINISIGN_VERSION="0.12"
# minisign-0.12-linux.tar.gz; its .minisig verifies against the author's key
# RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3.
MINISIGN_SHA256="9a599b48ba6eb7b1e80f12f36b94ceca7c00b7a5173c95c3efc88d9822957e73"

fail() {
  echo "install-minisign: $*" >&2
  exit 1
}

[ "$#" -eq 1 ] || fail "usage: install-minisign.sh <dir>"
dir="$1"
[ "$(uname -s)" = "Linux" ] && [ "$(uname -m)" = "x86_64" ] ||
  fail "this installs the Linux x86_64 build, not $(uname -s) $(uname -m)"

mkdir -p "$dir"
archive="$dir/minisign-$MINISIGN_VERSION-linux.tar.gz"
curl -fsSL --retry 3 -o "$archive" \
  "https://github.com/jedisct1/minisign/releases/download/$MINISIGN_VERSION/minisign-$MINISIGN_VERSION-linux.tar.gz"
actual="$(sha256sum "$archive" | cut -d ' ' -f 1)"
if [ "$actual" != "$MINISIGN_SHA256" ]; then
  rm -f "$archive"
  fail "minisign-$MINISIGN_VERSION-linux.tar.gz has SHA-256 $actual, expected $MINISIGN_SHA256"
fi
tar -xzf "$archive" -C "$dir" minisign-linux/x86_64/minisign
binary="$dir/minisign-linux/x86_64/minisign"
"$binary" -v >&2
printf '%s\n' "$binary"
