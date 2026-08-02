#!/bin/sh
set -eu

REPO="ibrahemid/writ"
BIN_DIR="${HOME}/.local/bin"
LIB_DIR="${HOME}/.local/share/writ"

# Base64-decoded from the updater pubkey at src-tauri/tauri.conf.json ("plugins.updater.pubkey").
MINISIGN_PUBKEY="RWT2Ic6WHdF65qf9kASdZd1XZWkUJbTZGCCEcnYK5DlCSDbKNEQX/OHk"

fail() {
  echo "writ install: $1" >&2
  exit 1
}

have() {
  command -v "$1" >/dev/null 2>&1
}

need() {
  have "$1" || fail "missing required tool '$1'."
}

parse_tag() {
  tr -d '\r' | awk '
    tolower($1) == "location:" {
      n = split($2, seg, "/")
      if (n > 1 && seg[n - 1] == "tag" && seg[n] != "") tag = seg[n]
    }
    END { if (tag != "") print tag }
  '
}

sha256_check_file() {
  if have sha256sum; then
    sha256sum -c "$1"
  else
    shasum -a 256 -c "$1"
  fi
}

# Returns 2 when the asset has no entry in the sums file, 1 when the digest does not match.
verify_sha256() {
  vs_dir="$1"
  vs_asset="$2"
  vs_sums="$3"
  if ! vs_line="$(awk -v name="$vs_asset" '$2 == name { line = $0 } END { if (line == "") exit 1; print line }' "$vs_sums")"; then
    return 2
  fi
  printf '%s\n' "$vs_line" >"${vs_dir}/${vs_asset}.sha256"
  (cd "$vs_dir" && sha256_check_file "${vs_asset}.sha256" >/dev/null 2>&1) || return 1
}

# Tauri publishes the minisign file base64-encoded; older assets may be raw.
decode_sig() {
  ds_raw="$1"
  ds_out="$2"
  if base64 -d <"$ds_raw" >"$ds_out" 2>/dev/null && head -n 1 "$ds_out" | grep -q '^untrusted comment:'; then
    return 0
  fi
  cp "$ds_raw" "$ds_out"
}

verify_minisign() {
  minisign -V -P "$MINISIGN_PUBKEY" -x "$2" -m "$1" >/dev/null 2>&1
}

# Removing first: install(1) into a path that is currently executing fails with ETXTBSY.
install_bin() {
  rm -f "$2"
  install -m 0755 "$1" "$2"
}

main() {
  need uname

  OS="$(uname -s)"
  if [ "${OS}" != "Linux" ]; then
    fail "this script is for Linux. For macOS and Windows, download the installer from https://github.com/${REPO}/releases/latest."
  fi

  ARCH="$(uname -m)"
  case "${ARCH}" in
    x86_64 | amd64) ;;
    *)
      echo "writ install: unsupported architecture ${ARCH}." >&2
      echo "Only x86_64 Linux is built today. Track aarch64 at https://github.com/${REPO}/issues" >&2
      exit 1
      ;;
  esac

  need curl
  need install
  have sha256sum || have shasum || fail "missing required tool 'sha256sum' or 'shasum'."

  TAG="$(curl -fsSI "https://github.com/${REPO}/releases/latest" | parse_tag)"
  case "${TAG}" in
    v[0-9]*) ;;
    *) fail "could not reach GitHub or resolve the latest release. Check your connection and retry." ;;
  esac
  VERSION="${TAG#v}"

  ASSET="Writ_${VERSION}_amd64.AppImage"
  BASE_URL="https://github.com/${REPO}/releases/download/${TAG}"

  TMP="$(mktemp -d 2>/dev/null || mktemp -d -t writ)"
  trap 'rm -rf "${TMP}"' EXIT INT TERM
  DOWNLOAD="${TMP}/${ASSET}"

  echo "writ install: downloading ${ASSET} (${TAG})"
  curl -fL --progress-bar -o "${DOWNLOAD}" "${BASE_URL}/${ASSET}" ||
    fail "could not download ${ASSET}. Check your connection and retry."
  curl -fsSL -o "${TMP}/SHA256SUMS.txt" "${BASE_URL}/SHA256SUMS.txt" ||
    fail "could not download SHA256SUMS.txt. Check your connection and retry."

  SHA_RC=0
  verify_sha256 "${TMP}" "${ASSET}" "${TMP}/SHA256SUMS.txt" || SHA_RC=$?
  case "${SHA_RC}" in
    0) ;;
    2) fail "${ASSET} is not listed in SHA256SUMS.txt. Nothing was installed." ;;
    *) fail "checksum mismatch for ${ASSET}. The download is corrupt or was tampered with. Nothing was installed." ;;
  esac

  if have minisign; then
    curl -fsSL -o "${DOWNLOAD}.sig" "${BASE_URL}/${ASSET}.sig" ||
      fail "could not download ${ASSET}.sig. Check your connection and retry."
    decode_sig "${DOWNLOAD}.sig" "${DOWNLOAD}.minisig"
    verify_minisign "${DOWNLOAD}" "${DOWNLOAD}.minisig" ||
      fail "signature check failed for ${ASSET}. Nothing was installed."
  fi

  chmod 0755 "${DOWNLOAD}"
  (cd "${TMP}" && "./${ASSET}" --appimage-extract usr/bin/writ >/dev/null 2>&1) || true
  if [ ! -f "${TMP}/squashfs-root/usr/bin/writ" ]; then
    (cd "${TMP}" && "./${ASSET}" --appimage-extract >/dev/null 2>&1) || true
  fi
  [ -f "${TMP}/squashfs-root/usr/bin/writ" ] ||
    fail "could not unpack the writ command from ${ASSET}. Nothing was installed."

  mkdir -p "${LIB_DIR}" "${BIN_DIR}"

  APP_PATH="${LIB_DIR}/Writ.AppImage"
  CLI_PATH="${LIB_DIR}/writ-cli"
  GUI_PATH="${APP_PATH}"

  install_bin "${DOWNLOAD}" "${APP_PATH}"
  install_bin "${TMP}/squashfs-root/usr/bin/writ" "${CLI_PATH}"

  if ! have fusermount; then
    echo "writ install: fusermount not found, so Writ will run from an unpacked copy. Install the 'fuse' package and re-run this installer to change that." >&2
    GUI_PATH="${LIB_DIR}/writ-gui"
    cat >"${TMP}/writ-gui" <<EOF
#!/bin/sh
exec "${APP_PATH}" --appimage-extract-and-run "\$@"
EOF
    install_bin "${TMP}/writ-gui" "${GUI_PATH}"
  fi

  cat >"${TMP}/writ" <<EOF
#!/bin/sh
WRIT_GUI_BIN="${GUI_PATH}"; export WRIT_GUI_BIN
exec "${CLI_PATH}" "\$@"
EOF
  install_bin "${TMP}/writ" "${BIN_DIR}/writ"

  echo "writ install: installed ${VERSION}"
  echo "  command: ${BIN_DIR}/writ"
  echo "  app:     ${APP_PATH}"

  case ":${PATH}:" in
    *":${BIN_DIR}:"*) ;;
    *)
      echo
      echo "Heads up: ${BIN_DIR} is not in your PATH."
      echo "Add this to your shell rc file, then re-source it:"
      echo "  export PATH=\"\${HOME}/.local/bin:\${PATH}\""
      ;;
  esac
}

main "$@"
