#!/usr/bin/env bash
# Signing, notarization and disk-image steps of the macOS release. The sign-macos
# job in .github/workflows/release.yml runs them on the app the build job made,
# so no build tool runs beside the Developer ID certificate.
#
# Usage:
#   scripts/macos-release.sh extract-app <built.dmg> <dest/Writ.app>
#   scripts/macos-release.sh sign-app <Writ.app> <identity> [keychain]
#   scripts/macos-release.sh notarize <Writ.app | file.pkg | file.dmg>
#       reads APPLE_ID, APPLE_PASSWORD and APPLE_TEAM_ID
#   scripts/macos-release.sh repack-dmg <built.dmg> <signed Writ.app> <out.dmg>

set -euo pipefail

MOUNTS=()
SCRATCH=()

fail() {
  echo "macos-release: $*" >&2
  exit 1
}

detach() {
  local mountpoint="$1" attempt
  for attempt in 1 2 3 4 5; do
    if hdiutil detach "$mountpoint" -quiet; then
      return 0
    fi
    sleep "$attempt"
  done
  hdiutil detach "$mountpoint" -force -quiet
}

cleanup() {
  local mountpoint dir
  for mountpoint in ${MOUNTS[@]+"${MOUNTS[@]}"}; do
    hdiutil detach "$mountpoint" -force -quiet >/dev/null 2>&1 || true
  done
  for dir in ${SCRATCH[@]+"${SCRATCH[@]}"}; do
    rm -rf "$dir"
  done
}
FINISHED=false

# bash 3.2, the macOS /bin/bash, hands an EXIT trap status 0 after an unbound
# variable error, so an exit before the command finished counts as a failure.
on_exit() {
  local status=$?
  cleanup
  if [ "$status" -eq 0 ] && [ "$FINISHED" != "true" ]; then
    status=1
  fi
  exit "$status"
}
trap on_exit EXIT

# Sets SCRATCH_DIR in this shell; a command substitution would lose the record
# the exit trap cleans up from.
new_scratch_dir() {
  SCRATCH_DIR="$(mktemp -d "${TMPDIR:-/tmp}/writ-macos-release.XXXXXX")"
  SCRATCH+=("$SCRATCH_DIR")
}

attach() {
  local image="$1" mountpoint="$2" mode="$3"
  hdiutil attach "$image" "$mode" -nobrowse -noautoopen -noverify -mountpoint "$mountpoint" -quiet
  MOUNTS+=("$mountpoint")
}

release_mount() {
  local mountpoint="$1" kept=() entry
  detach "$mountpoint"
  for entry in ${MOUNTS[@]+"${MOUNTS[@]}"}; do
    [ "$entry" = "$mountpoint" ] || kept+=("$entry")
  done
  MOUNTS=(${kept[@]+"${kept[@]}"})
}

bundle_executable() {
  python3 -c 'import plistlib, sys; print(plistlib.load(open(sys.argv[1], "rb"))["CFBundleExecutable"])' \
    "$1/Contents/Info.plist"
}

cdhash() {
  codesign -dvvv "$1" 2>&1 | sed -n 's/^CDHash=//p' | head -n 1
}

extract_app() {
  local dmg="$1" dest="$2" work mountpoint entry apps=()
  [ -f "$dmg" ] || fail "no disk image at $dmg"
  [ ! -e "$dest" ] || fail "$dest already exists"
  new_scratch_dir
  work="$SCRATCH_DIR"
  mountpoint="$work/mnt"
  mkdir "$mountpoint"
  attach "$dmg" "$mountpoint" -readonly
  for entry in "$mountpoint"/*.app; do
    [ -d "$entry" ] && apps+=("$entry")
  done
  [ "${#apps[@]}" -eq 1 ] || fail "expected one .app at the root of $dmg, found ${#apps[@]}"
  ditto "${apps[0]}" "$dest"
  release_mount "$mountpoint"
  echo "extracted $(basename "${apps[0]}") from $dmg"
}

require_hardened() {
  local path="$1" identity="$2" details
  details="$(codesign -dvvv "$path" 2>&1)"
  case "$details" in
    *"runtime)"*) ;;
    *) fail "$path is not signed with the hardened runtime" ;;
  esac
  if [ "$identity" != "-" ]; then
    case "$details" in
      *"Timestamp="*) ;;
      *) fail "$path carries no secure timestamp" ;;
    esac
  fi
}

sign_app() {
  local app="$1" identity="$2" keychain="${3:-}" main entry nested=() args
  [ -d "$app/Contents/MacOS" ] || fail "$app is not an app bundle"
  [ -n "$identity" ] || fail "no signing identity"
  main="$(bundle_executable "$app")"
  [ -f "$app/Contents/MacOS/$main" ] || fail "$app names $main as its executable, and it is missing"
  args=(--force --sign "$identity" --options runtime --timestamp)
  if [ -n "$keychain" ]; then
    args+=(--keychain "$keychain")
  fi
  # codesign refuses resource forks and Finder info on files it seals.
  xattr -cr "$app"
  # Inside out: everything nested is signed before the bundle that seals it.
  if [ -d "$app/Contents/Frameworks" ]; then
    while IFS= read -r -d '' entry; do
      nested+=("$entry")
    done < <(find "$app/Contents/Frameworks" -mindepth 1 -maxdepth 1 -print0 | sort -z)
  fi
  while IFS= read -r -d '' entry; do
    [ "$(basename "$entry")" = "$main" ] || nested+=("$entry")
  done < <(find "$app/Contents/MacOS" -mindepth 1 -maxdepth 1 -type f -print0 | sort -z)
  for entry in ${nested[@]+"${nested[@]}"}; do
    codesign "${args[@]}" "$entry"
  done
  codesign "${args[@]}" "$app"
  codesign --verify --strict --deep --verbose=2 "$app"
  require_hardened "$app" "$identity"
  for entry in ${nested[@]+"${nested[@]}"}; do
    require_hardened "$entry" "$identity"
  done
  echo "signed $(basename "$app") and ${#nested[@]} nested executables"
}

notarize() {
  local target="$1" submit work submission status name
  for name in APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID; do
    [ -n "${!name:-}" ] || fail "$name is not set"
  done
  [ -e "$target" ] || fail "nothing to notarize at $target"
  submit="$target"
  if [ -d "$target" ]; then
    new_scratch_dir
    work="$SCRATCH_DIR"
    submit="$work/$(basename "$target").zip"
    ditto -c -k --keepParent "$target" "$submit"
  fi
  # --wait has returned 0 on a rejected submission, so the status is read
  # rather than inferred from the exit code.
  submission="$(xcrun notarytool submit "$submit" \
    --apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" --team-id "$APPLE_TEAM_ID" \
    --wait --output-format json)"
  echo "$submission"
  status="$(printf '%s' "$submission" | jq -r '.status')"
  if [ "$status" != "Accepted" ]; then
    echo "notarization of $(basename "$target") came back $status" >&2
    xcrun notarytool log "$(printf '%s' "$submission" | jq -r '.id')" \
      --apple-id "$APPLE_ID" --password "$APPLE_PASSWORD" --team-id "$APPLE_TEAM_ID" >&2 || true
    exit 1
  fi
  xcrun stapler staple "$target"
  xcrun stapler validate "$target"
}

repack_dmg() {
  local built="$1" app="$2" out="$3" name work mountpoint want got
  [ -f "$built" ] || fail "no disk image at $built"
  [ -d "$app" ] || fail "no app at $app"
  [ ! -e "$out" ] || fail "$out already exists"
  name="$(basename "$app")"
  new_scratch_dir
  work="$SCRATCH_DIR"
  mountpoint="$work/mnt"
  mkdir "$mountpoint"
  # The build's image keeps its window layout, volume icon and Applications
  # link; only the bundle inside it is swapped for the signed one.
  hdiutil convert "$built" -format UDRW -o "$work/rw.dmg" -quiet
  attach "$work/rw.dmg" "$mountpoint" -readwrite
  [ -d "$mountpoint/$name" ] || fail "$built holds no $name to replace"
  rm -rf "${mountpoint:?}/$name"
  ditto "$app" "$mountpoint/$name"
  chmod -Rf go-w "$mountpoint" || true
  rm -rf "$mountpoint/.fseventsd"
  release_mount "$mountpoint"
  hdiutil convert "$work/rw.dmg" -format UDZO -imagekey zlib-level=9 -o "$out" -quiet
  attach "$out" "$mountpoint" -readonly
  want="$(cdhash "$app")"
  got="$(cdhash "$mountpoint/$name")"
  release_mount "$mountpoint"
  [ -n "$want" ] || fail "$app carries no code signature"
  [ "$want" = "$got" ] || fail "the $name in $out is not the signed one (CDHash $got, expected $want)"
  echo "wrote $out around $name (CDHash $want)"
}

command="${1:-}"
shift || true
case "$command" in
  extract-app)
    [ "$#" -eq 2 ] || fail "usage: extract-app <built.dmg> <dest/Writ.app>"
    extract_app "$@"
    ;;
  sign-app)
    [ "$#" -eq 2 ] || [ "$#" -eq 3 ] || fail "usage: sign-app <Writ.app> <identity> [keychain]"
    sign_app "$@"
    ;;
  notarize)
    [ "$#" -eq 1 ] || fail "usage: notarize <Writ.app | file.pkg | file.dmg>"
    notarize "$@"
    ;;
  repack-dmg)
    [ "$#" -eq 3 ] || fail "usage: repack-dmg <built.dmg> <signed Writ.app> <out.dmg>"
    repack_dmg "$@"
    ;;
  *)
    fail "unknown command '${command}'; expected extract-app, sign-app, notarize or repack-dmg"
    ;;
esac
FINISHED=true
