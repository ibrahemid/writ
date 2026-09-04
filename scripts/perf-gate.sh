#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "${ROOT}"

step() { printf '\n\033[1;36m==> %s\033[0m\n' "$*"; }
fail() { printf '\033[1;31mxx %s\033[0m\n' "$*" >&2; exit 1; }

RUN_BENCHES=0
for arg in "$@"; do
    case "${arg}" in
        --benches) RUN_BENCHES=1 ;;
        *) fail "unknown argument: ${arg}" ;;
    esac
done

step "perf gate: transform budget (writ-plugin, release)"
WRIT_PERF_GATE=1 cargo test \
    --release \
    -p writ-plugin \
    --test perf_budget \
    -- --nocapture

step "perf gate: builtin transform probe (writ-plugin, release)"
cargo test \
    --release \
    -p writ-plugin \
    --test builtin_perf_tests \
    -- --ignored --nocapture

step "perf gate: language detection input cap (frontend)"
PNPM="${PNPM:-pnpm}"
command -v "${PNPM}" >/dev/null 2>&1 || fail "pnpm not found; set PNPM to its path"
WRIT_PERF_GATE=1 "${PNPM}" exec vitest run \
    src/__tests__/services/language-detect.test.ts

step "perf gate: storage budget (writ-storage, release)"
WRIT_PERF_GATE=1 cargo test \
    --release \
    -p writ-storage \
    --test perf_budget \
    -- --nocapture

if [[ "${RUN_BENCHES}" -eq 1 ]]; then
    step "criterion benchmarks: writ-plugin transforms"
    cargo bench -p writ-plugin --bench transforms

    step "criterion benchmarks: writ-storage"
    cargo bench -p writ-storage --bench storage
fi

echo
echo "perf-gate: OK"
