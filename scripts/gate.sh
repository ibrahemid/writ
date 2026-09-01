#!/bin/bash
# Quiet quality gate: one line per check, failure detail only on failure.
# Full logs: $WRIT_GATE_LOG_DIR (default .status/gate-logs, gitignored).
# Usage: scripts/gate.sh [fmt|test|clippy|tsc|build|vitest]...   (default: all)

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
      test)   grep -E '^(test .* FAILED|failures:|---- .* stdout ----|thread .* panicked|error(\[E[0-9]+\])?:)' "$log" | head -n "$TAIL" ;;
      vitest) grep -E '(FAIL|✗|×|AssertionError|Error:)' "$log" | head -n "$TAIL" ;;
      clippy|build) grep -E -A4 '^(error|warning)(\[E[0-9]+\])?:' "$log" | head -n "$TAIL" ;;
      tsc)    grep -E 'error TS[0-9]+' "$log" | head -n "$TAIL" ;;
      *)      tail -n "$TAIL" "$log" ;;
    esac
  fi
}

GATES=("$@")
[ ${#GATES[@]} -eq 0 ] && GATES=(fmt test clippy tsc build)

for g in "${GATES[@]}"; do
  case "$g" in
    fmt)    run_gate fmt cargo fmt --all --check ;;
    test)   run_gate test cargo test --workspace --quiet ;;
    clippy) run_gate clippy cargo clippy --workspace --quiet -- -D warnings ;;
    tsc)    run_gate tsc npx tsc --noEmit ;;
    build)  run_gate build pnpm build ;;
    vitest) run_gate vitest npx vitest run --reporter=dot ;;
    *) echo "unknown gate: $g" >&2; exit 2 ;;
  esac
done
exit $STATUS
