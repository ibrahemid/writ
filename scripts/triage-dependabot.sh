#!/usr/bin/env bash
# Triage open dependabot PRs:
#   - rebase all open dependabot PRs onto current main
#   - enable auto-merge on Tier 1 (patches + safe minors)
#   - print the rest for manual review
#
# Usage:
#   scripts/triage-dependabot.sh rebase           # comment @dependabot rebase on every open PR
#   scripts/triage-dependabot.sh automerge-safe   # turn on auto-merge for Tier 1 PRs
#   scripts/triage-dependabot.sh status           # print PR table grouped by tier
#   scripts/triage-dependabot.sh all              # rebase, then automerge-safe, then status
set -euo pipefail

REPO="${REPO:-ibrahemid/writ}"

list_prs() {
  gh pr list -R "$REPO" --author "app/dependabot" --state open \
    --json number,title,headRefName --limit 50
}

# Tier 1: safe to auto-merge once CI is green.
#   - any GitHub Actions bump
#   - any patch bump (semver pre-major) for npm dev/test deps
#   - tracing-appender and similar Rust patches
is_tier1() {
  local title="$1"
  case "$title" in
    *"actions/checkout"*|*"actions/cache"*|*"actions/upload-pages-artifact"*|\
    *"actions/configure-pages"*|*"actions/deploy-pages"*|*"actions/setup-node"*) return 0 ;;
    *"jsdom"*|*"vitest"*|*"vite-plugin-solid"*|*"tracing-appender"*|*"@codemirror/search"*) return 0 ;;
  esac
  return 1
}

# Tier 5: do NOT auto-merge. Big blast radius.
is_tier5() {
  local title="$1"
  case "$title" in
    *"rusqlite"*|*"vite from "*|*"notify from"*|*"notify-debouncer-mini"*|*"toml from"*|\
    *"pnpm/action-setup"*|*"peter-evans/create-pull-request"*) return 0 ;;
  esac
  return 1
}

cmd_rebase() {
  list_prs | jq -r '.[] | [.number, .title] | @tsv' | while IFS=$'\t' read -r num title; do
    echo "rebasing #$num — $title"
    gh pr comment "$num" -R "$REPO" --body "@dependabot rebase" >/dev/null
  done
}

cmd_automerge_safe() {
  list_prs | jq -r '.[] | [.number, .title] | @tsv' | while IFS=$'\t' read -r num title; do
    if is_tier1 "$title"; then
      echo "auto-merge ON  #$num  $title"
      gh pr merge "$num" -R "$REPO" --auto --squash || echo "  (auto-merge failed, check branch protection)"
    elif is_tier5 "$title"; then
      echo "MANUAL REVIEW  #$num  $title"
    else
      echo "review needed  #$num  $title"
    fi
  done
}

cmd_status() {
  printf "%-6s %-12s %s\n" "#" "TIER" "TITLE"
  list_prs | jq -r '.[] | [.number, .title] | @tsv' | while IFS=$'\t' read -r num title; do
    if is_tier1 "$title"; then tier="1-safe"
    elif is_tier5 "$title"; then tier="5-major"
    else tier="3-review"
    fi
    printf "%-6s %-12s %s\n" "$num" "$tier" "$title"
  done
}

case "${1:-status}" in
  rebase) cmd_rebase ;;
  automerge-safe) cmd_automerge_safe ;;
  status) cmd_status ;;
  all) cmd_rebase; sleep 2; cmd_automerge_safe; cmd_status ;;
  *) echo "usage: $0 {rebase|automerge-safe|status|all}"; exit 2 ;;
esac
