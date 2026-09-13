#!/usr/bin/env bash
# Puts earlier versions of one note into the instance's history store, the
# way the app keeps them: a content-addressed blob per version under
# <data dir>/history/ and a row per version in <data dir>/history.db.
#
#   seed-history.sh <data dir> <note path relative to the notes folder> <version file>...
#
# Version files are given oldest first and land at spread-out times over the
# last week, so the panel shows a history rather than one run of saves.
set -euo pipefail

DATA_DIR="$1"
NOTE_PATH="$2"
shift 2
DB="$DATA_DIR/history.db"

sqlite3 "$DB" <<'SQL'
CREATE TABLE IF NOT EXISTS notes (id INTEGER PRIMARY KEY, path TEXT NOT NULL UNIQUE, identity TEXT, birth_ns TEXT);
CREATE TABLE IF NOT EXISTS versions (id INTEGER PRIMARY KEY, note_id INTEGER NOT NULL REFERENCES notes(id) ON DELETE CASCADE, at_ms INTEGER NOT NULL, bytes INTEGER NOT NULL, hash TEXT NOT NULL, merges INTEGER NOT NULL DEFAULT 0);
CREATE INDEX IF NOT EXISTS versions_by_note ON versions(note_id, at_ms DESC, id DESC);
CREATE INDEX IF NOT EXISTS versions_by_hash ON versions(hash);
CREATE UNIQUE INDEX IF NOT EXISTS notes_by_identity ON notes(identity) WHERE identity IS NOT NULL;
SQL

escaped=${NOTE_PATH//\'/\'\'}
sqlite3 "$DB" "INSERT OR IGNORE INTO notes (path) VALUES ('$escaped');"
note_id=$(sqlite3 "$DB" "SELECT id FROM notes WHERE path = '$escaped';")

now_ms=$(( $(date +%s) * 1000 ))
count=$#
i=0
# Oldest version six days back, the newest a couple of hours ago.
for file in "$@"; do
  hash=$(shasum -a 256 "$file" | cut -d' ' -f1)
  bytes=$(stat -f %z "$file")
  mkdir -p "$DATA_DIR/history/${hash:0:2}"
  /bin/cp -f "$file" "$DATA_DIR/history/${hash:0:2}/${hash:2}"
  remaining=$(( count - 1 - i ))
  if [ "$remaining" -eq 0 ]; then
    back_ms=$(( 2 * 3600 * 1000 + 17 * 60 * 1000 ))
  else
    back_ms=$(( remaining * 2 * 24 * 3600 * 1000 - 3 * 3600 * 1000 * i ))
  fi
  at_ms=$(( now_ms - back_ms ))
  sqlite3 "$DB" "INSERT INTO versions (note_id, at_ms, bytes, hash, merges) VALUES ($note_id, $at_ms, $bytes, '$hash', 0);"
  i=$(( i + 1 ))
done
echo "seeded $count versions of $NOTE_PATH into $DB"
