#!/usr/bin/env bash
# Writ public launch automation.
#
# Phases run sequentially by default; pass --phase to run a single phase.
# Each phase prints what it will do, then prompts for confirmation unless --yes.
#
#   scripts/launch-public.sh --version 0.1.0
#   scripts/launch-public.sh --version 0.1.0 --phase tag
#   scripts/launch-public.sh --version 0.1.0 --phase flip --yes
#
# Phases:
#   preflight  Run quality gates and sanity-check repo state
#   tag        Bump versions, update CHANGELOG, tag, push (triggers matrix CI)
#   wait       Poll CI for the release workflow on the new tag
#   publish    Edit the draft release: not draft, not pre-release (for stable)
#   flip       Repo public + Pages enable + branch protection (single bundled step)
#   postflip   Dispatch site deploy, verify install.sh and latest.json
#   all        Run every phase in order (default)
#
# Requirements:
#   gh, git, jq, cargo, pnpm, python3, curl
#   GH_TOKEN with admin scope on the repo (gh auth status)
#   working tree clean on main, up to date with origin

set -euo pipefail

REPO="ibrahemid/writ"
VERSION=""
PHASE="all"
ASSUME_YES="false"

usage() {
  sed -n '2,28p' "$0"
  exit 1
}

while [ $# -gt 0 ]; do
  case "$1" in
    --version) VERSION="$2"; shift 2 ;;
    --phase) PHASE="$2"; shift 2 ;;
    --yes|-y) ASSUME_YES="true"; shift ;;
    -h|--help) usage ;;
    *) echo "Unknown arg: $1" >&2; usage ;;
  esac
done

[ -n "$VERSION" ] || { echo "--version is required" >&2; usage; }

case "$VERSION" in
  *.*.*) ;;
  *) echo "Version must be semver, got: $VERSION" >&2; exit 1 ;;
esac

TAG="v${VERSION}"
IS_PRERELEASE="false"
if [[ "$VERSION" == *-* ]]; then IS_PRERELEASE="true"; fi

step() { printf '\n\033[1;36m==> %s\033[0m\n' "$*"; }
note() { printf '   %s\n' "$*"; }
warn() { printf '\033[1;33m!! %s\033[0m\n' "$*"; }
fail() { printf '\033[1;31mxx %s\033[0m\n' "$*" >&2; exit 1; }

confirm() {
  if [ "$ASSUME_YES" = "true" ]; then return 0; fi
  printf '\033[1;33mProceed? [y/N] \033[0m'
  read -r ans
  case "$ans" in y|Y|yes|YES) return 0 ;; *) fail "aborted by user" ;; esac
}

phase_preflight() {
  step "preflight: repo state and quality gates"

  [ "$(git rev-parse --abbrev-ref HEAD)" = "main" ] || fail "not on main"
  [ -z "$(git status --porcelain)" ] || fail "working tree dirty"
  git fetch origin main --quiet
  [ "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)" ] || fail "main not in sync with origin/main"

  note "cargo test --workspace"
  cargo test --workspace --quiet
  note "cargo clippy --workspace -- -D warnings"
  cargo clippy --workspace -- -D warnings 2>&1 | tail -1
  note "npx tsc --noEmit"
  npx tsc --noEmit
  note "pnpm build"
  pnpm build > /dev/null
  note "pnpm --dir site build"
  pnpm --dir site build > /dev/null
  note "preflight passed"
}

phase_tag() {
  step "tag: bump to ${VERSION}, update CHANGELOG, tag ${TAG}, push"

  if git rev-parse "$TAG" >/dev/null 2>&1; then
    fail "tag ${TAG} already exists locally"
  fi
  if gh release view "$TAG" --repo "$REPO" >/dev/null 2>&1; then
    fail "release ${TAG} already exists on GitHub"
  fi

  note "this will edit Cargo.toml, package.json, src-tauri/tauri.conf.json, CHANGELOG.md and create a commit + tag"
  confirm

  python3 .github/scripts/bump_version.py --version "$VERSION"

  if grep -q "^## \[Unreleased\]" CHANGELOG.md; then
    DATE="$(date -u +%Y-%m-%d)"
    sed -i.bak "s/^## \[Unreleased\]/## [${VERSION}] - ${DATE}/" CHANGELOG.md
    rm -f CHANGELOG.md.bak
  else
    warn "no Unreleased section in CHANGELOG.md, skipping rename"
  fi

  git add Cargo.toml package.json src-tauri/tauri.conf.json CHANGELOG.md
  git commit -m "chore(release): ${VERSION}"
  git tag "$TAG"
  git push origin main
  git push origin "$TAG"

  note "tag pushed; matrix CI now building. use --phase wait to watch"
}

phase_wait() {
  step "wait: poll release workflow on ${TAG}"

  for _ in $(seq 1 30); do
    RUN_ID="$(gh run list --workflow=release.yml --branch "$TAG" --limit 1 --json databaseId,status,conclusion --jq '.[0].databaseId // empty' 2>/dev/null || true)"
    [ -n "$RUN_ID" ] && break
    note "waiting for release workflow run to register..."
    sleep 5
  done
  [ -n "$RUN_ID" ] || fail "no release workflow run found for ${TAG}"

  note "watching run ${RUN_ID}"
  gh run watch "$RUN_ID" --repo "$REPO" --exit-status

  note "release workflow finished"
}

phase_publish() {
  step "publish: convert draft release ${TAG} to published"

  RELEASE_INFO="$(gh release view "$TAG" --repo "$REPO" --json isDraft,isPrerelease,assets || true)"
  [ -n "$RELEASE_INFO" ] || fail "release ${TAG} not found"

  IS_DRAFT="$(echo "$RELEASE_INFO" | jq -r .isDraft)"
  ASSET_COUNT="$(echo "$RELEASE_INFO" | jq -r '.assets | length')"

  note "draft: ${IS_DRAFT}, assets: ${ASSET_COUNT}, prerelease target: ${IS_PRERELEASE}"
  [ "$ASSET_COUNT" -ge 6 ] || warn "asset count looks low (${ASSET_COUNT}); expected mac pkg, win msi, linux deb+AppImage, SHA256SUMS, latest.json"

  note "smoke-test the artefacts on macOS, Windows, Linux BEFORE publishing. publishing makes them the auto-update target."
  confirm

  gh release edit "$TAG" --repo "$REPO" --draft=false --prerelease="$IS_PRERELEASE"
  note "release ${TAG} published"
}

phase_flip() {
  step "flip: repo public + Pages enable + branch protection (bundled)"

  CURRENT_VIS="$(gh repo view "$REPO" --json visibility --jq .visibility)"
  if [ "$CURRENT_VIS" = "PUBLIC" ]; then
    note "repo is already PUBLIC, skipping visibility flip"
  else
    note "repo will become PUBLIC. anyone on the internet can read it. THIS IS NOT REVERSIBLE WITHOUT EXTERNAL SUPPORT."
    confirm
    gh repo edit "$REPO" --visibility public --accept-visibility-change-consequences
    note "repo is now public"
  fi

  note "enable Pages with workflow source"
  if gh api -X POST "repos/${REPO}/pages" \
      -f build_type=workflow \
      -f 'source[branch]=main' \
      -f 'source[path]=/' >/dev/null 2>&1; then
    note "Pages enabled"
  else
    note "Pages already configured or POST failed; trying PUT"
    gh api -X PUT "repos/${REPO}/pages" -f build_type=workflow >/dev/null 2>&1 || warn "Pages config returned non-zero; check Settings > Pages manually"
  fi

  note "apply branch protection on main"
  gh api -X PUT "repos/${REPO}/branches/main/protection" --input - <<'JSON' >/dev/null
{
  "required_status_checks": {"strict": true, "contexts": ["Linux", "macOS", "Windows"]},
  "enforce_admins": false,
  "required_pull_request_reviews": null,
  "restrictions": null,
  "allow_force_pushes": false,
  "allow_deletions": false,
  "required_linear_history": true,
  "required_conversation_resolution": true
}
JSON
  note "branch protection applied"

  gh repo view "$REPO" --json visibility,description,homepageUrl --jq '"visibility: \(.visibility)\nhomepage: \(.homepageUrl)"'
}

phase_postflip() {
  step "postflip: dispatch site deploy, verify install.sh and latest.json"

  note "dispatch site workflow so it picks up the now-public release.json sync"
  gh workflow run site.yml --repo "$REPO" || warn "site.yml dispatch failed"

  note "wait for site workflow to register"
  sleep 8
  SITE_RUN="$(gh run list --workflow=site.yml --branch main --limit 1 --json databaseId --jq '.[0].databaseId // empty')"
  if [ -n "$SITE_RUN" ]; then
    gh run watch "$SITE_RUN" --repo "$REPO" --exit-status || warn "site workflow did not complete cleanly"
  fi

  note "verify latest.json is reachable"
  LATEST_URL="https://github.com/${REPO}/releases/latest/download/latest.json"
  curl -fsSL "$LATEST_URL" | jq . > /tmp/writ-latest.json && note "latest.json OK at ${LATEST_URL}" || warn "latest.json fetch failed"

  note "verify install.sh resolves the latest tag"
  if curl -fsSL "https://raw.githubusercontent.com/${REPO}/main/install.sh" | sh -n; then
    note "install.sh syntax OK on raw URL"
  else
    warn "install.sh raw fetch or syntax check failed"
  fi
}

phase_verify() {
  step "verify: post-launch site sanity"

  note "site home"; curl -fsSI "https://ibrahemid.github.io/writ/" | head -1
  note "site /download"; curl -fsSI "https://ibrahemid.github.io/writ/download/" | head -1
  note "site /changelog"; curl -fsSI "https://ibrahemid.github.io/writ/changelog/" | head -1
  note "release /latest"; curl -fsSI "https://github.com/${REPO}/releases/latest" | head -1
}

case "$PHASE" in
  preflight) phase_preflight ;;
  tag)       phase_preflight && phase_tag ;;
  wait)      phase_wait ;;
  publish)   phase_publish ;;
  flip)      phase_flip ;;
  postflip)  phase_postflip ;;
  verify)    phase_verify ;;
  all)
    phase_preflight
    phase_tag
    phase_wait
    phase_publish
    phase_flip
    phase_postflip
    phase_verify
    ;;
  *) fail "unknown phase: $PHASE" ;;
esac

step "done"
