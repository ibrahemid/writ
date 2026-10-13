#!/bin/bash
# Quiet quality gate: one line per check, failure detail only on failure.
# Full logs: $WRIT_GATE_LOG_DIR (default .status/gate-logs, gitignored).
# Usage: scripts/gate.sh [fmt|test|clippy|tsc|build|vitest|quick]...   (default: all)
# quick: fmt, clippy, tsc, the tests of the crates changed against origin/main,
#        and the vitest files related to the changed TypeScript.

cd "${CLAUDE_PROJECT_DIR:-$(git rev-parse --show-toplevel 2>/dev/null || pwd)}" || exit 1
LOG_DIR="${WRIT_GATE_LOG_DIR:-.status/gate-logs}"
mkdir -p "$LOG_DIR"
TAIL="${WRIT_GATE_TAIL:-25}"
STATUS=0

run_gate() {
  local name="$1"; shift
  local log="$LOG_DIR/$name.log"
  local start=$SECONDS
  if "$@" >"$log" 2>&1; then
    printf 'PASS %-7s %3ds\n' "$name" $((SECONDS - start))
  else
    STATUS=1
    printf 'FAIL %-7s %3ds  log: %s\n' "$name" $((SECONDS - start)) "$log"
    case "$name" in
      test|ctest) grep -E '^(test .* FAILED|failures:|---- .* stdout ----|thread .* panicked|error(\[E[0-9]+\])?:)' "$log" | head -n "$TAIL" ;;
      vitest|vrel) grep -E '(FAIL|✗|×|AssertionError|Error:)' "$log" | head -n "$TAIL" ;;
      clippy|build) grep -E -A4 '^(error|warning)(\[E[0-9]+\])?:' "$log" | head -n "$TAIL" ;;
      tsc)    grep -E 'error TS[0-9]+' "$log" | head -n "$TAIL" ;;
      *)      tail -n "$TAIL" "$log" ;;
    esac
  fi
}

changed_files() {
  local base
  base=$(git merge-base origin/main HEAD 2>/dev/null || git rev-parse HEAD)
  {
    git diff --name-only "$base" HEAD 2>/dev/null
    git diff --name-only HEAD 2>/dev/null
    git ls-files --others --exclude-standard 2>/dev/null
  } | sort -u
}

quick_gate() {
  local files pkgs ts
  files=$(changed_files)
  run_gate fmt cargo fmt --all --check
  run_gate clippy cargo clippy --workspace --all-targets --quiet -- -D warnings
  run_gate tsc npx tsc --noEmit
  if printf '%s\n' "$files" | grep -qE '^(Cargo\.(toml|lock)|rust-toolchain\.toml)$'; then
    run_gate ctest cargo test --workspace --quiet
  else
    names=""
    while IFS= read -r f; do
      case "$f" in
        crates/*/*) c=${f#crates/}; names="$names ${c%%/*}" ;;
        src-tauri/*) names="$names writ-tauri" ;;
      esac
    done <<<"$files"
    pkgs=""
    for c in $(printf '%s\n' $names | sort -u); do pkgs="$pkgs -p $c"; done
    if [ -n "$pkgs" ]; then
      # shellcheck disable=SC2086
      run_gate ctest cargo test --quiet $pkgs
    else
      printf 'SKIP ctest   no Rust crate changed\n'
    fi
  fi
  ts=$(printf '%s\n' "$files" | grep -E '^(src|demo|scripts)/.*\.(ts|tsx|mts)$' | grep -v '\.d\.ts$' | tr '\n' ' ')
  if [ -n "$(printf '%s' "$ts" | tr -d ' ')" ]; then
    # shellcheck disable=SC2086
    run_gate vrel npx vitest related --run --reporter=dot $ts
  else
    printf 'SKIP vrel    no TypeScript changed\n'
  fi
}

GATES=("$@")
[ ${#GATES[@]} -eq 0 ] && GATES=(fmt test clippy tsc build)

for g in "${GATES[@]}"; do
  case "$g" in
    fmt)    run_gate fmt cargo fmt --all --check ;;
    test)   run_gate test cargo test --workspace --quiet ;;
    clippy) run_gate clippy cargo clippy --workspace --all-targets --quiet -- -D warnings ;;
    tsc)    run_gate tsc npx tsc --noEmit ;;
    build)  run_gate build pnpm build ;;
    vitest) run_gate vitest npx vitest run --reporter=dot ;;
    quick)  quick_gate ;;
    *) echo "unknown gate: $g" >&2; exit 2 ;;
  esac
done
exit $STATUS
