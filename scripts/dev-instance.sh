#!/usr/bin/env bash
# Run a Jotra dev instance isolated from any other running instance, so several
# worktrees can run in parallel. Each instance gets its own vite port, matching
# Tauri devUrl, and its own data directory (separate SQLite database, buffers,
# and config).
#
# Usage:
#   scripts/dev-instance.sh [PORT]
#
# PORT defaults to $JOTRA_DEV_PORT, then 1420. The data directory defaults to
# $JOTRA_DATA_DIR, then ~/.jotra-dev-<port>. Override either via the environment.
#
# Examples:
#   scripts/dev-instance.sh 1430
#   JOTRA_DATA_DIR=~/.jotra-feature scripts/dev-instance.sh 1430
set -euo pipefail

PORT="${1:-${JOTRA_DEV_PORT:-1420}}"
DATA_DIR="${JOTRA_DATA_DIR:-$HOME/.jotra-dev-$PORT}"

export JOTRA_DEV_PORT="$PORT"
export JOTRA_DATA_DIR="$DATA_DIR"

echo "Jotra dev instance: port=$PORT data=$DATA_DIR"

exec cargo tauri dev --config "{\"build\":{\"devUrl\":\"http://localhost:$PORT\"}}"
