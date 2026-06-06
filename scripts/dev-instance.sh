#!/usr/bin/env bash
# Run a Writ dev instance isolated from any other running instance, so several
# worktrees can run in parallel. Each instance gets its own vite port, matching
# Tauri devUrl, and its own data directory (separate SQLite database, buffers,
# and config).
#
# Usage:
#   scripts/dev-instance.sh [PORT]
#
# PORT defaults to $WRIT_DEV_PORT, then 1420. The data directory defaults to
# $WRIT_DATA_DIR, then ~/.writ-dev-<port>. Override either via the environment.
#
# Examples:
#   scripts/dev-instance.sh 1430
#   WRIT_DATA_DIR=~/.writ-feature scripts/dev-instance.sh 1430
set -euo pipefail

PORT="${1:-${WRIT_DEV_PORT:-1420}}"
DATA_DIR="${WRIT_DATA_DIR:-$HOME/.writ-dev-$PORT}"

export WRIT_DEV_PORT="$PORT"
export WRIT_DATA_DIR="$DATA_DIR"

echo "Writ dev instance: port=$PORT data=$DATA_DIR"

exec cargo tauri dev --config "{\"build\":{\"devUrl\":\"http://localhost:$PORT\"}}"
