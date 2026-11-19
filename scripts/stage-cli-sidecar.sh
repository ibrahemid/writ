#!/usr/bin/env bash
set -euo pipefail

# Copies one build of the command to src-tauri/binaries under every name the
# app bundles it as (bundle.externalBin in src-tauri/tauri.conf.json), for one
# target triple. Tauri resolves each sidecar as <name>-<triple>[.exe].
#
# Usage: scripts/stage-cli-sidecar.sh <built-command> <target-triple>

if [[ $# -ne 2 ]]; then
  echo "usage: $0 <built-command> <target-triple>" >&2
  exit 2
fi

built="$1"
triple="$2"
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
conf="${root}/src-tauri/tauri.conf.json"

if [[ ! -f "${built}" ]]; then
  echo "stage-cli-sidecar: no build at ${built}" >&2
  exit 1
fi
if [[ ! -f "${conf}" ]]; then
  echo "stage-cli-sidecar: no ${conf}" >&2
  exit 1
fi

names="$(
  tr -d '\r\n' < "${conf}" |
    grep -o '"externalBin"[[:space:]]*:[[:space:]]*\[[^]]*\]' |
    sed 's/^"externalBin"[[:space:]]*:[[:space:]]*\[//; s/\]$//' |
    grep -o '"[^"]*"' |
    tr -d '"' || true
)"
if [[ -z "${names}" ]]; then
  echo "stage-cli-sidecar: ${conf} lists no bundle.externalBin entries" >&2
  exit 1
fi

suffix=""
case "${triple}" in *windows*) suffix=".exe" ;; esac

while IFS= read -r name; do
  destination="${root}/src-tauri/${name}-${triple}${suffix}"
  mkdir -p "$(dirname "${destination}")"
  cp "${built}" "${destination}"
  chmod 755 "${destination}"
  echo "staged ${destination#"${root}/"}"
done <<< "${names}"
