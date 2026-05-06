#!/bin/sh
set -eu

REPO="ibrahemid/writ"
INSTALL_DIR="${HOME}/.local/bin"
BIN_NAME="writ"

need() {
  command -v "$1" >/dev/null 2>&1 || { echo "writ install: missing required tool '$1'." >&2; exit 1; }
}
need uname

OS="$(uname -s)"
if [ "${OS}" != "Linux" ]; then
  echo "writ install: this script is for Linux. On macOS use 'brew install --cask ibrahemid/writ/writ', on Windows use 'winget install --id ibrahemid.Writ -e'." >&2
  exit 1
fi

ARCH="$(uname -m)"
case "${ARCH}" in
  x86_64|amd64) ;;
  *)
    echo "writ install: unsupported architecture ${ARCH}." >&2
    echo "Only x86_64 Linux is built today. Track aarch64 at https://github.com/${REPO}/issues" >&2
    exit 1
    ;;
esac

need curl
need install

LATEST_URL="https://api.github.com/repos/${REPO}/releases/latest"
TAG="$(curl -fsSL "${LATEST_URL}" | grep '"tag_name":' | head -n 1 | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')"
if [ -z "${TAG}" ]; then
  echo "writ install: could not resolve latest release from ${LATEST_URL}." >&2
  echo "If the repo has no releases yet, this script will start working once v0.1.0 ships." >&2
  exit 1
fi
VERSION="${TAG#v}"

ASSET="writ_${VERSION}_amd64.AppImage"
URL="https://github.com/${REPO}/releases/download/${TAG}/${ASSET}"

TMP="$(mktemp -d 2>/dev/null || mktemp -d -t writ)"
trap 'rm -rf "${TMP}"' EXIT INT TERM
DOWNLOAD="${TMP}/${ASSET}"

echo "writ install: downloading ${ASSET} (${TAG})"
curl -fL --progress-bar -o "${DOWNLOAD}" "${URL}"

mkdir -p "${INSTALL_DIR}"
DEST="${INSTALL_DIR}/${BIN_NAME}"
install -m 0755 "${DOWNLOAD}" "${DEST}"

echo "writ install: installed ${VERSION} at ${DEST}"

case ":${PATH}:" in
  *":${INSTALL_DIR}:"*) ;;
  *)
    echo
    echo "Heads up: ${INSTALL_DIR} is not in your PATH."
    echo "Add this to your shell rc file, then re-source it:"
    echo "  export PATH=\"\${HOME}/.local/bin:\${PATH}\""
    ;;
esac
