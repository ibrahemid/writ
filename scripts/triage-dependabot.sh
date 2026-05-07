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
  printf "%-6s %-12s %-8s %s\n" "#" "TIER" "CI" "TITLE"
  list_prs | jq -r '.[] | [.number, .title] | @tsv' | while IFS=$'\t' read -r num title; do
    if is_tier1 "$title"; then tier="1-safe"
    elif is_tier5 "$title"; then tier="5-major"
    else tier="3-review"
    fi
    ci=$(pr_ci_state "$num")
    printf "%-6s %-12s %-8s %s\n" "$num" "$tier" "$ci" "$title"
  done
}

pr_ci_state() {
  gh pr checks "$1" -R "$REPO" --json state --jq \
    '[.[].state] | if length == 0 then "NONE"
                   elif any(. == "FAILURE") then "FAIL"
                   elif any(. == "PENDING" or . == "IN_PROGRESS" or . == "QUEUED" or . == "EXPECTED") then "PEND"
                   elif all(. == "SUCCESS" or . == "NEUTRAL" or . == "SKIPPED") then "GREEN"
                   else "?" end' 2>/dev/null || echo "?"
}

# Merge ANY dependabot PR whose CI is green, regardless of tier.
# CI is the gate. If you don't trust CI, fix CI - don't keep PRs open.
cmd_merge_green() {
  list_prs | jq -r '.[] | [.number, .title] | @tsv' | while IFS=$'\t' read -r num title; do
    state=$(pr_ci_state "$num")
    case "$state" in
      GREEN)
        echo "MERGE  #$num  $title"
        gh pr merge "$num" -R "$REPO" --squash --delete-branch --auto || true
        ;;
      FAIL) echo "skip   #$num  CI FAIL  $title" ;;
      PEND) echo "skip   #$num  CI pending  $title" ;;
      *)    echo "skip   #$num  CI=$state  $title" ;;
    esac
  done
}

case "${1:-status}" in
  rebase) cmd_rebase ;;
  automerge-safe) cmd_automerge_safe ;;
  merge-green) cmd_merge_green ;;
  status) cmd_status ;;
  all) cmd_rebase; sleep 2; cmd_automerge_safe; cmd_status ;;
  weekly) cmd_rebase; sleep 5; cmd_merge_green; cmd_status ;;
  *) echo "usage: $0 {rebase|automerge-safe|merge-green|status|all|weekly}"; exit 2 ;;
esac
