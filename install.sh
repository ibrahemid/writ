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

# OpenSSL before 3.0 has no Ed25519 verify over raw input, and LibreSSL none.
openssl_is_3() {
  oi_version="$(openssl version 2>/dev/null)" || return 1
  case "${oi_version}" in
    "OpenSSL "[3-9].* | "OpenSSL "[1-9][0-9]*.*) return 0 ;;
  esac
  return 1
}

pick_verifier() {
  if have minisign; then
    echo minisign
  elif have openssl && openssl_is_3; then
    echo openssl
  else
    echo none
  fi
}

b64_to_file() {
  printf '%s' "$1" | base64 -d >"$2" 2>/dev/null
}

byte_count() {
  wc -c <"$1" | tr -d ' '
}

# Prints <count> bytes of <file> from <offset> on.
slice() {
  dd if="$1" bs=1 skip="$2" count="$3" 2>/dev/null
}

hex_slice() {
  slice "$1" "$2" "$3" | od -An -tx1 | tr -d ' \n'
}

# Checks a minisign signature the way `minisign -V` does: the key ids match,
# Ed25519 verifies the signature over the file's BLAKE2b-512 digest (minisign's
# prehashed "ED" mode, which the release signs with), and Ed25519 verifies the
# global signature over the signature and the trusted comment.
verify_openssl() {
  vo_file="$1"
  vo_sig="$2"
  vo_work="$3"
  mkdir -p "${vo_work}"

  b64_to_file "${MINISIGN_PUBKEY}" "${vo_work}/pk.bin" || return 1
  [ "$(byte_count "${vo_work}/pk.bin")" = 42 ] || return 1
  [ "$(hex_slice "${vo_work}/pk.bin" 0 2)" = "4564" ] || return 1
  # SubjectPublicKeyInfo DER prefix for an Ed25519 key, then the 32 key bytes.
  {
    printf '\060\052\060\005\006\003\053\145\160\003\041\000'
    slice "${vo_work}/pk.bin" 10 32
  } >"${vo_work}/pk.der"

  vo_sig_line="$(sed -n 2p "${vo_sig}")"
  vo_trusted_line="$(sed -n 3p "${vo_sig}")"
  vo_global_line="$(sed -n 4p "${vo_sig}")"
  case "${vo_trusted_line}" in
    "trusted comment: "*) ;;
    *) return 1 ;;
  esac

  b64_to_file "${vo_sig_line}" "${vo_work}/sig.bin" || return 1
  [ "$(byte_count "${vo_work}/sig.bin")" = 74 ] || return 1
  [ "$(hex_slice "${vo_work}/sig.bin" 0 2)" = "4544" ] || return 1
  [ "$(hex_slice "${vo_work}/sig.bin" 2 8)" = "$(hex_slice "${vo_work}/pk.bin" 2 8)" ] || return 1
  slice "${vo_work}/sig.bin" 10 64 >"${vo_work}/sig.raw"

  openssl dgst -blake2b512 -binary -out "${vo_work}/digest" "${vo_file}" >/dev/null 2>&1 || return 1
  openssl pkeyutl -verify -pubin -inkey "${vo_work}/pk.der" -keyform DER -rawin \
    -in "${vo_work}/digest" -sigfile "${vo_work}/sig.raw" >/dev/null 2>&1 || return 1

  b64_to_file "${vo_global_line}" "${vo_work}/global.raw" || return 1
  [ "$(byte_count "${vo_work}/global.raw")" = 64 ] || return 1
  {
    cat "${vo_work}/sig.raw"
    printf '%s' "${vo_trusted_line#trusted comment: }"
  } >"${vo_work}/global.msg"
  openssl pkeyutl -verify -pubin -inkey "${vo_work}/pk.der" -keyform DER -rawin \
    -in "${vo_work}/global.msg" -sigfile "${vo_work}/global.raw" >/dev/null 2>&1
}

# verify_signature <verifier> <file> <minisign signature> <work dir>
verify_signature() {
  case "$1" in
    minisign) verify_minisign "$2" "$3" ;;
    openssl) verify_openssl "$2" "$3" "$4" ;;
    *) return 1 ;;
  esac
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

  VERIFIER="$(pick_verifier)"
  if [ "${VERIFIER}" = none ]; then
    if [ "${WRIT_INSTALL_SKIP_SIGNATURE:-}" != 1 ]; then
      fail "cannot check the release signature: install minisign or OpenSSL 3.0 or later and run this again. To install without checking the signature, run it with WRIT_INSTALL_SKIP_SIGNATURE=1."
    fi
    echo "writ install: WRIT_INSTALL_SKIP_SIGNATURE=1 is set, so the release signature is not checked." >&2
  else
    need base64
  fi

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

  if [ "${VERIFIER}" != none ]; then
    curl -fsSL -o "${TMP}/SHA256SUMS.txt.minisig" "${BASE_URL}/SHA256SUMS.txt.minisig" ||
      fail "could not download SHA256SUMS.txt.minisig. Check your connection and retry."
    verify_signature "${VERIFIER}" "${TMP}/SHA256SUMS.txt" "${TMP}/SHA256SUMS.txt.minisig" "${TMP}/verify-sums" ||
      fail "the signature on SHA256SUMS.txt does not match Writ's release key. Nothing was installed."
  fi

  SHA_RC=0
  verify_sha256 "${TMP}" "${ASSET}" "${TMP}/SHA256SUMS.txt" || SHA_RC=$?
  case "${SHA_RC}" in
    0) ;;
    2) fail "${ASSET} is not listed in SHA256SUMS.txt. Nothing was installed." ;;
    *) fail "checksum mismatch for ${ASSET}. The download is corrupt or was tampered with. Nothing was installed." ;;
  esac

  if [ "${VERIFIER}" != none ]; then
    curl -fsSL -o "${DOWNLOAD}.sig" "${BASE_URL}/${ASSET}.sig" ||
      fail "could not download ${ASSET}.sig. Check your connection and retry."
    decode_sig "${DOWNLOAD}.sig" "${DOWNLOAD}.minisig"
    verify_signature "${VERIFIER}" "${DOWNLOAD}" "${DOWNLOAD}.minisig" "${TMP}/verify-asset" ||
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
