#!/usr/bin/env bash
# Captures the stills the site uses, from a release build of the current
# branch, driven by keystrokes posted to that one process and captured by its
# CGWindowID. Nothing here touches ~/.writ or ~/Writ; the instance runs on a
# scratch copy of scripts/capture/fixtures.
#
#   scripts/capture/run.sh --all
#   scripts/capture/run.sh --scene hero-window --scene search --theme light
#   scripts/capture/run.sh --scene hero-window --shell win
#   scripts/capture/run.sh --scene loop-markdown
#   scripts/capture/run.sh --report
#
# Flags
#   --scene <name>    one scene (repeatable); names are listed under SCENES,
#                     LOOP_SCENES and REPORT_SCENES
#   --all             every scene in SCENES and LOOP_SCENES, the two shell
#                     heroes included (REPORT_SCENES only run when named)
#   --theme           light | dark | both (default both)
#   --shell           mac | win | linux (default mac; win and linux run a dev
#                     instance built with VITE_WRIT_PLATFORM, so they are slower)
#   --size            1280x800 | 1440x900 (default 1280x800; hero-window is
#                     1440x900 unless --size is given)
#   --no-build        reuse the bundle from the last build
#   --report          print each loop and README GIF against its size limit,
#                     then exit (1 when any file is over)
#
# Every key and click waits until this instance is the frontmost app and the
# machine has been idle for 45 s; the run refuses to start while any other
# Writ process exists.
#
# Every scene's config switches on only the apps that scene shows (apps_on);
# the rest are off, as in a fresh config.
#
# The chat scene and every loop-<name> scene record the window while it is
# driven and encode each take to site/public/media/<name>-<theme>.mp4 and
# .webm, the pair Loop.astro plays with the still as the poster. A run of
# loop-markdown also encodes its takes to docs/media/hero-<theme>.gif for the
# README and removes the hero PNGs those GIFs replace.
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
OUT="$ROOT/site/src/assets/captures"
SHOTS="$ROOT/.status/v2/shots"
PROGRAMS_OUT="${CAPTURE_PROGRAMS_OUT:-$ROOT/.status/reports/programs-section}"
FIXTURE="$HERE/fixtures/home/Notes"
VERSIONS="$HERE/fixtures/versions"
DRIVE="$HERE/.bin/drive"
WORK="${CAPTURE_WORK:-${TMPDIR:-/tmp}/writ-capture-$$}"
IDLE_FLOOR="${CAPTURE_IDLE_FLOOR:-45}"
MEDIA_OUT="${CAPTURE_MEDIA_OUT:-$ROOT/site/public/media}"
README_OUT="${CAPTURE_README_OUT:-$ROOT/docs/media}"
STUB_PORT=8791
DEV_PORT=1450
WIN_X=120
WIN_Y=100
MP4_LIMIT=1258291
WEBM_LIMIT=838860
GIF_LIMIT=1200000

SCENES=(hero-window text-file markdown-inline search apps first-run notes-folder connections graph-folder graph-local preview-rich chat versions activity settings-appearance obsidian-folder tags)
# Recorded takes, no stills: loop-<name> writes <name>-<theme>.mp4 and .webm.
LOOP_SCENES=(loop-any-file loop-markdown loop-search loop-apps loop-versions)
# Named only: report stills, not site assets.
REPORT_SCENES=(settings-programs)
SHELL_SCENES=(hero-window-win hero-window-linux)

# report_media: every loop and README GIF against its limit; 1 when any is over.
report_media() {
  local file limit size seconds state over=0
  for file in "$MEDIA_OUT"/*.webm "$MEDIA_OUT"/*.mp4 "$README_OUT"/*.gif; do
    [ -f "$file" ] || continue
    case "$file" in
      *.webm) limit=$WEBM_LIMIT ;;
      *.mp4) limit=$MP4_LIMIT ;;
      *) limit=$GIF_LIMIT ;;
    esac
    size=$(stat -f%z "$file")
    seconds=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$file" 2>/dev/null || true)
    if [ -n "$seconds" ]; then seconds=$(printf '%.1fs' "$seconds"); else seconds="-"; fi
    if [ "$size" -le "$limit" ]; then state=ok; else state=OVER; over=1; fi
    printf '%-4s  %8d of %8d bytes  %6s  %s\n' "$state" "$size" "$limit" "$seconds" "${file#"$ROOT"/}"
  done
  return "$over"
}

# ---------------------------------------------------------------- flags ----

WANTED=()
THEMES="light dark"
SHELL_KIND=mac
SIZE=1280x800
SIZE_GIVEN=0
NO_BUILD=0
REPORT_ONLY=0
while [ $# -gt 0 ]; do
  case "$1" in
    --scene) WANTED+=("$2"); shift 2 ;;
    --all) WANTED=("${SCENES[@]}" "${LOOP_SCENES[@]}" "${SHELL_SCENES[@]}"); shift ;;
    --theme)
      case "$2" in
        light) THEMES="light" ;;
        dark) THEMES="dark" ;;
        both) THEMES="light dark" ;;
        *) echo "--theme takes light, dark or both" >&2; exit 2 ;;
      esac
      shift 2 ;;
    --shell)
      case "$2" in mac|win|linux) SHELL_KIND="$2" ;; *) echo "--shell takes mac, win or linux" >&2; exit 2 ;; esac
      shift 2 ;;
    --size)
      case "$2" in 1280x800|1440x900) SIZE="$2"; SIZE_GIVEN=1 ;; *) echo "--size takes 1280x800 or 1440x900" >&2; exit 2 ;; esac
      shift 2 ;;
    --no-build) NO_BUILD=1; shift ;;
    --report) REPORT_ONLY=1; shift ;;
    -h|--help) sed -n '2,38p' "$0"; exit 0 ;;
    *) echo "unknown flag $1" >&2; exit 2 ;;
  esac
done
if [ "$REPORT_ONLY" -eq 1 ]; then
  report_media
  exit
fi
if [ ${#WANTED[@]} -eq 0 ]; then
  echo "nothing to do: pass --scene <name>, --all or --report" >&2
  exit 2
fi
for scene in "${WANTED[@]}"; do
  case " ${SCENES[*]} ${LOOP_SCENES[*]} ${SHELL_SCENES[*]} ${REPORT_SCENES[*]} " in
    *" $scene "*) ;;
    *) echo "unknown scene $scene" >&2; exit 2 ;;
  esac
done

log() { printf '%s  %s\n' "$(date +%H:%M:%S)" "$*"; }

# ------------------------------------------------------------- preflight ----

APP_PID=""
DEV_PID=""
STUB_PID=""
REC_PID=""
REC_WATCH_PID=""
REC_LIVE=0
REC_T_STOP=0
REC_T_OPEN=0
REC_T_END=0
FINDER_WINDOW=""
PATH_BAR_HIDDEN=0
SYSTEM_DARK_BEFORE=""
HIDPI_PID=""
CAPTURED=()

cleanup() {
  local status=$?
  record_stop || true
  quit_app || true
  stop_stub || true
  close_finder || true
  restore_system_appearance || true
  stop_hidpi || true
  if [ "$status" -ne 0 ]; then
    log "failed (exit $status); scratch kept at $WORK"
  fi
}
trap cleanup EXIT

preflight() {
  for tool in cargo node sqlite3 swiftc screencapture ffmpeg ffprobe osascript shasum; do
    command -v "$tool" >/dev/null || { echo "missing tool: $tool" >&2; exit 1; }
  done
  # The Tauri CLI comes from cargo-tauri where it is installed, else from the
  # project's own @tauri-apps/cli.
  if cargo tauri --version >/dev/null 2>&1; then TAURI=(cargo tauri)
  elif (cd "$ROOT" && npx --no-install tauri --version) >/dev/null 2>&1; then TAURI=(npx --no-install tauri)
  else echo "missing tool: the Tauri CLI (cargo-tauri or node_modules/.bin/tauri)" >&2; exit 1
  fi
  case "$WORK" in
    "$HOME/.writ"*|"$HOME/Writ"*) echo "scratch dir must not be under ~/.writ or ~/Writ" >&2; exit 1 ;;
  esac
  if pgrep -x writ-tauri >/dev/null 2>&1; then
    echo "refusing to run: another Writ process exists" >&2
    pgrep -xl writ-tauri >&2
    exit 1
  fi
  if [ ! -x "$DRIVE" ] || [ "$HERE/drive.swift" -nt "$DRIVE" ]; then
    mkdir -p "$HERE/.bin"
    log "compiling the driver"
    swiftc -O -o "$DRIVE" "$HERE/drive.swift" 2>&1 | grep -i ' error' && exit 1
  fi
  mkdir -p "$OUT" "$SHOTS" "$PROGRAMS_OUT" "$WORK"
  # macOS TMPDIR sits under /var, a link to /private/var. The app's config
  # watcher matches the event path against the config path it was given, so
  # both have to be the resolved one or a theme switch is never seen.
  WORK=$(cd "$WORK" && pwd -P)
  start_hidpi
  wait_idle
}

# Stills are 2x. On a machine whose main display is 1x (the headless M1's
# fallback display is one), the run adds a display drawn at 2x for its own
# length and puts every window on it. CAPTURE_HIDPI=0 keeps the display as it is.
HIDPI_SIZE=(1680 1050)
start_hidpi() {
  local x y w h waited=0
  [ "${CAPTURE_HIDPI:-1}" != 0 ] || return 0
  [ "$("$DRIVE" scale)" = 1 ] || return 0
  "$DRIVE" hidpi "${HIDPI_SIZE[@]}" >"$WORK/hidpi.out" 2>"$WORK/hidpi.err" &
  HIDPI_PID=$!
  until [ -s "$WORK/hidpi.out" ]; do
    kill -0 "$HIDPI_PID" 2>/dev/null || { echo "the 2x display did not start: $(cat "$WORK/hidpi.err")" >&2; exit 1; }
    [ "$waited" -lt 60 ] || { echo "the 2x display did not come on" >&2; exit 1; }
    sleep 0.5; waited=$((waited + 1))
  done
  read -r x y w h <"$WORK/hidpi.out"
  WIN_X=$((x + WIN_X)); WIN_Y=$((y + WIN_Y))
  log "2x display ${w}x${h} at $x,$y for this run"
}
stop_hidpi() {
  [ -n "$HIDPI_PID" ] || return 0
  kill "$HIDPI_PID" 2>/dev/null || true
  wait "$HIDPI_PID" 2>/dev/null || true
  HIDPI_PID=""
}

wait_idle() {
  local reported=0
  while ! "$DRIVE" away; do
    if [ "$reported" -eq 0 ]; then log "waiting for $IDLE_FLOOR s of idle time (now $("$DRIVE" idle | cut -d. -f1) s)"; reported=1; fi
    sleep 5
  done
}

# ----------------------------------------------------------------- build ----

TARGET_DIR=$(cd "$ROOT" && cargo metadata --format-version 1 --no-deps 2>/dev/null | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')
TRIPLE=$(rustc -vV | sed -n 's/^host: //p')
APP="$TARGET_DIR/release/bundle/macos/Writ.app"
APP_BIN="$APP/Contents/MacOS/writ-tauri"
CLI_BIN="$APP/Contents/MacOS/writ"

build_release() {
  if [ "$NO_BUILD" -eq 1 ] && [ -x "$APP_BIN" ]; then
    log "reusing $APP"
    return
  fi
  log "building the CLI sidecar"
  (cd "$ROOT" && CARGO_PROFILE_RELEASE_STRIP=false cargo build --release -p writ-cli >"$WORK/build-cli.log" 2>&1)
  mkdir -p "$ROOT/src-tauri/binaries"
  /bin/cp -f "$TARGET_DIR/release/writ" "$ROOT/src-tauri/binaries/writ-$TRIPLE"
  log "building the release app"
  (cd "$ROOT" && CARGO_PROFILE_RELEASE_STRIP=false "${TAURI[@]}" build --bundles app \
      --config '{"bundle":{"createUpdaterArtifacts":false}}' >"$WORK/build-app.log" 2>&1) \
    || { echo "release build failed, see $WORK/build-app.log" >&2; exit 1; }
  [ -x "$APP_BIN" ] || { echo "no app at $APP after the build" >&2; exit 1; }
  log "bundle: $APP"
  log "sidecar: $(cat "$ROOT/src-tauri/binaries/writ-$TRIPLE" | shasum -a 256 | cut -c1-12) $CLI_BIN"
}

# ----------------------------------------------------------- the instance ----

DATA=""
NOTES=""
W=1280
H=800
POLARITY=light
SIDEBAR_OPEN=true
COLLAPSED='[]'
PANEL_OPEN=false
CHAT_OPEN=false
LAYOUT=inline
DEFAULT_EXTENSION=txt
APP_CHAT=false
APP_REWRITE=false
APP_PROGRAMS=false
APP_CONNECTIONS=false
APP_GRAPH=false
APP_TAGS=false
AI_PROVIDER=ollama
AI_BASE_URL=""
AI_MODEL=""
EXTRA_CONFIG=""
APPEARANCE_EXTRA=""
SEED_CONFIG=1
EMPTY_NOTES=0
PRESEED=0
BLANK_TAB=0
WORKSPACE=1

reset_state() {
  W=${SIZE%x*}; H=${SIZE#*x}
  set -- $THEMES; POLARITY=$1
  SIDEBAR_OPEN=true; COLLAPSED='[]'; PANEL_OPEN=false; CHAT_OPEN=false; LAYOUT=inline
  DEFAULT_EXTENSION=txt
  APP_CHAT=false; APP_REWRITE=false; APP_PROGRAMS=false
  APP_CONNECTIONS=false; APP_GRAPH=false; APP_TAGS=false
  AI_PROVIDER=ollama; AI_BASE_URL=""; AI_MODEL=""
  EXTRA_CONFIG=""; APPEARANCE_EXTRA=""; SEED_CONFIG=1; EMPTY_NOTES=0; PRESEED=0; WORKSPACE=1
}

# apps_on <app>...: the apps a scene shows, by their Settings ids (chat,
# rewrite, programs, connections, graph, tags). Every other app stays off.
apps_on() {
  local app
  for app in "$@"; do
    case "$app" in
      chat) APP_CHAT=true ;;
      rewrite) APP_REWRITE=true ;;
      programs) APP_PROGRAMS=true ;;
      connections) APP_CONNECTIONS=true ;;
      graph) APP_GRAPH=true ;;
      tags) APP_TAGS=true ;;
      *) echo "apps_on: unknown app $app" >&2; exit 2 ;;
    esac
  done
}

write_config() {
  local workspace=""
  # The sidebar's folder section is the open workspace; pointing it at the
  # notes folder is what puts the notes tree on screen.
  if [ "$WORKSPACE" -eq 1 ]; then workspace="root = \"$NOTES\""; fi
  cat >"$DATA/config.toml" <<CFG
[appearance]
polarity = "$POLARITY"
accent = "pine"
$APPEARANCE_EXTRA

[theme]
preset = "writ-$POLARITY"

[window]
width = $W
height = $H
x = $WIN_X
y = $WIN_Y

[workspace]
$workspace

[sidebar]
open = $SIDEBAR_OPEN
width = 240
collapsed = $COLLAPSED
hidden = ["recent"]

[panel]
open = $PANEL_OPEN
width = 300

[chat_panel]
open = $CHAT_OPEN
width = 380

[preview]
default_layout_markdown = "$LAYOUT"

[files]
default_extension = "$DEFAULT_EXTENSION"

[apps]
connections = $APP_CONNECTIONS
graph = $APP_GRAPH
tags = $APP_TAGS

[ai]
provider = "$AI_PROVIDER"
base_url = "$AI_BASE_URL"
model = "$AI_MODEL"
consented_hosts = []

[ai.rewrite]
enabled = $APP_REWRITE

[ai.chat]
enabled = $APP_CHAT
model = ""

[mcp]
enabled = $APP_PROGRAMS

[spelling]
enabled = false

[first_run]
hint_dismissed = true

[updater]
auto_check = false
$EXTRA_CONFIG
CFG
}

# Modification times a folder of files would carry, so Finder and the
# sidebar show a week of writing rather than one copy.
date_fixture() {
  local root=$1
  while IFS='|' read -r stamp path; do
    [ -e "$root/$path" ] && touch -t "$stamp" "$root/$path"
  done <<'STAMPS'
202609122110|Lisbon in October.md
202609122040|Reading list.md
202609011905|Recipes/Shakshuka.md
202609061530|Recipes/Lemon olive oil cake.md
202609111805|Garden/Garden plan.md
202609091930|Garden/Seed order.md
202609102100|Garden/Volunteer rota.md
202609102015|Garden committee 10 Sep.md
202608301845|Weekly/2026-W35.md
202609061910|Weekly/2026-W36.md
202609122200|Weekly/2026-W37.md
202609121600|Sourdough notes.md
202609110830|Newsletter draft.md
202609081215|Birthday ideas.md
202609071120|From Obsidian/Moving house.md
202609112230|From Obsidian/Packing list.md
202609121930|To do.txt
202609051745|Camping kit.txt
202609120831|Server log.txt
202609011905|Recipes
202609111805|Garden
202609122200|Weekly
202609112230|From Obsidian
STAMPS
}

# begin <scene>: a fresh data dir and notes copy, config written, app up.
begin() {
  local scene=$1
  local dir="$WORK/$scene"
  if [ "$PRESEED" -eq 0 ]; then /bin/rm -rf "$dir"; fi
  DATA="$dir/data"; NOTES="$dir/Notes"
  mkdir -p "$DATA" "$NOTES"
  if [ "$EMPTY_NOTES" -eq 0 ]; then
    /bin/cp -R "$FIXTURE/." "$NOTES/"
    date_fixture "$NOTES"
  fi
  if [ "$SEED_CONFIG" -eq 1 ]; then write_config; fi
  log "$scene: launching ($SHELL_KIND shell, ${W}x${H})"
  wait_idle
  if [ "$SHELL_KIND" = mac ]; then
    WRIT_DATA_DIR="$DATA" WRIT_NOTES_DIR="$NOTES" "$APP_BIN" >"$dir/app.log" 2>&1 &
    APP_PID=$!
  else
    set -m
    (cd "$ROOT" && WRIT_DATA_DIR="$DATA" WRIT_NOTES_DIR="$NOTES" VITE_WRIT_PLATFORM="$SHELL_KIND" \
      WRIT_DEV_PORT="$DEV_PORT" "${TAURI[@]}" dev --no-watch \
      --config "{\"build\":{\"devUrl\":\"http://localhost:$DEV_PORT\"}}") >"$dir/app.log" 2>&1 &
    DEV_PID=$!
    set +m
    for _ in $(seq 1 1200); do
      APP_PID=$(pgrep -x writ-tauri | head -1 || true)
      [ -n "$APP_PID" ] && break
      sleep 1
    done
    [ -n "$APP_PID" ] || { echo "dev instance did not start, see $dir/app.log" >&2; exit 1; }
  fi
  for _ in $(seq 1 120); do
    if "$DRIVE" windows "$APP_PID" 2>/dev/null | grep -q .; then break; fi
    kill -0 "$APP_PID" 2>/dev/null || { echo "the app exited, see $dir/app.log" >&2; exit 1; }
    sleep 0.5
  done
  sleep 2
  "$DRIVE" activate "$APP_PID" >/dev/null
  # A seeded config restores no tabs, so the frontend opens one blank note
  # titled with the date; the first open_note closes it, and the sidebar's
  # recently-closed section stays hidden so the blank note never lists.
  BLANK_TAB=$SEED_CONFIG
}

quit_app() {
  if [ -n "$APP_PID" ]; then
    # A signal leaves the session unfinished, and the next launch reopens the
    # tab marked recovered; the menu quit closes it.
    CAPTURE_WAIT_LIMIT=20 "$DRIVE" key "$APP_PID" q cmd || true
    for _ in $(seq 1 20); do
      kill -0 "$APP_PID" 2>/dev/null || break
      sleep 0.5
    done
    kill "$APP_PID" 2>/dev/null || true
    wait "$APP_PID" 2>/dev/null || true
    APP_PID=""
  fi
  if [ -n "$DEV_PID" ]; then
    kill -- -"$DEV_PID" 2>/dev/null || kill "$DEV_PID" 2>/dev/null || true
    wait "$DEV_PID" 2>/dev/null || true
    DEV_PID=""
    pkill -x writ-tauri 2>/dev/null || true
  fi
  sleep 0.5
}

key() { "$DRIVE" key "$APP_PID" "$@"; }
typetext() { "$DRIVE" type "$APP_PID" "$1"; }
window_bounds() { "$DRIVE" windows "$APP_PID" | head -1 | cut -f2; }

# wait_for_element <role> <name> <seconds>: the element's "x y w h", once it
# is on screen.
wait_for_element() {
  local role=$1 name=$2 limit=$3 rect
  for _ in $(seq 1 $(( limit * 2 ))); do
    if rect=$("$DRIVE" find "$APP_PID" "$role" "$name" 2>/dev/null); then
      printf '%s\n' "$rect"
      return 0
    fi
    sleep 0.5
  done
  return 1
}

click_element() {
  local rect=$1 x y w h
  [ -n "$rect" ] || { echo "click_element: no rect" >&2; exit 1; }
  read -r x y w h <<<"$rect"
  "$DRIVE" click "$APP_PID" $(( x + w / 2 )) $(( y + h / 2 ))
}

open_note() {
  key o cmd,shift; sleep 0.5
  typetext "$1"; sleep 0.8
  key return; sleep 1.2
  if [ "$BLANK_TAB" -eq 1 ]; then
    key "[" cmd; sleep 0.4
    key w cmd; sleep 0.6
    BLANK_TAB=0
  fi
}
run_command() {
  key f cmd,shift; sleep 0.5
  typetext "> $1"; sleep 0.8
  key return; sleep 1.2
}
# go_to_line <n>: the cursor to the start of line n of the active file.
go_to_line() {
  key f cmd,shift; sleep 0.5
  typetext ":$1"; sleep 0.8
  key return; sleep 1
}
open_setting() {
  key f cmd,shift; sleep 0.5
  typetext "$1"; sleep 0.8
  key return; sleep 1.2
}

# type_human <text>: one key per character, a beat longer after a space, so a
# recorded take shows the text arriving at a typing pace.
type_human() {
  local text=$1 i ch
  for (( i = 0; i < ${#text}; i++ )); do
    ch=${text:i:1}
    typetext "$ch"
    if [ "$ch" = " " ]; then sleep 0.06; fi
  done
}

# pointer_away: the pointer just right of the window, out of a take's frame.
pointer_away() {
  local x y w h
  read -r x y w h <<<"$(window_bounds)"
  "$DRIVE" move "$APP_PID" $(( x + w + 40 )) $(( y + h / 2 ))
}

set_polarity() {
  [ "$POLARITY" = "$1" ] && return
  POLARITY=$1
  sed -i '' -e "s/^polarity = .*/polarity = \"$1\"/" -e "s/^preset = .*/preset = \"writ-$1\"/" "$DATA/config.toml"
  sleep 2.5
}

capture_window() {
  local file=$1 wid
  "$DRIVE" wait "$APP_PID"
  wid=$("$DRIVE" windows "$APP_PID" | head -1 | cut -f1)
  [ -n "$wid" ] || { echo "no window for pid $APP_PID" >&2; exit 1; }
  screencapture -l "$wid" -x -o -t png "$file"
  node "$HERE/shrink.mjs" "$file"
  CAPTURED+=("$file")
}

# shoot <name>: one file per theme, switching the theme through the config
# the instance watches.
shoot() {
  local name=$1 theme
  for theme in $THEMES; do
    set_polarity "$theme"
    capture_window "$OUT/$name-$theme.png"
  done
}

# ------------------------------------------------ system appearance stills ----

system_dark() { osascript -e 'tell application "System Events" to tell appearance preferences to get dark mode'; }
set_system_dark() {
  osascript -e "tell application \"System Events\" to tell appearance preferences to set dark mode to $1"
  "$DRIVE" stamp
}
remember_system_appearance() { [ -n "$SYSTEM_DARK_BEFORE" ] || SYSTEM_DARK_BEFORE=$(system_dark); }
restore_system_appearance() {
  [ -n "$SYSTEM_DARK_BEFORE" ] || return 0
  set_system_dark "$SYSTEM_DARK_BEFORE"
  SYSTEM_DARK_BEFORE=""
}

# shoot_system <name> <capture fn>: for windows that follow the system look.
shoot_system() {
  local name=$1 fn=$2 theme
  remember_system_appearance
  for theme in $THEMES; do
    if [ "$theme" = dark ]; then set_system_dark true; else set_system_dark false; fi
    sleep 2.5
    "$fn" "$OUT/$name-$theme.png"
  done
  restore_system_appearance
}

# ------------------------------------------------------------------ stub ----

start_stub() {
  node "$HERE/stub-host.mjs" "$STUB_PORT" >"$WORK/stub.log" 2>&1 &
  STUB_PID=$!
  sleep 0.5
}
stop_stub() {
  if [ -n "$STUB_PID" ]; then kill "$STUB_PID" 2>/dev/null || true; STUB_PID=""; fi
}

# ------------------------------------------------------------- recording ----

now() { python3 -c 'import time; print(time.time())'; }

# record_start <take>: the window into $WORK/<take>.mov until record_stop.
# REC_T_OPEN marks the take's first action, after a pre-roll the cut drops.
REC_PREROLL=2.5
REC_CEILING=180
record_start() {
  local take=$1 bounds x y w h
  bounds=$(window_bounds)
  [ -n "$bounds" ] || { echo "record_start: no window" >&2; exit 1; }
  read -r x y w h <<<"$bounds"
  /bin/rm -f "$WORK/$take.mov" "$WORK/$take.cut"
  # screencapture ends a -v recording cleanly on SIGINT only without -V, and
  # only when SIGINT is at its default, which a script's background job gets
  # only under job control. Its stdin stays off the terminal, or the job is
  # stopped the first time it reads it.
  set -m
  screencapture -v -x -R "$x,$y,$w,$h" "$WORK/$take.mov" </dev/null >"$WORK/$take.rec.log" 2>&1 &
  REC_PID=$!
  set +m
  ( sleep "$REC_CEILING" && kill -INT "$REC_PID" ) 2>/dev/null &
  REC_WATCH_PID=$!
  sleep "$REC_PREROLL"
  REC_T_OPEN=$(now)
}

# The take's clock runs backwards from the moment the recording was told to
# stop, so the markers do not depend on how long screencapture took to start.
record_stop() {
  [ -n "$REC_PID" ] || return 0
  REC_T_END=$(now)
  REC_T_STOP=$REC_T_END
  if kill -INT "$REC_PID" 2>/dev/null; then REC_LIVE=1; else REC_LIVE=0; fi
  wait "$REC_PID" 2>/dev/null || true
  pkill -P "$REC_WATCH_PID" 2>/dev/null || true
  REC_PID=""
  REC_WATCH_PID=""
}

seconds_of() { ffprobe -v error -show_entries format=duration -of csv=p=0 "$1"; }

# encode_under <out> <limit> <scale> <fps> <crf> <mov> <point> <length> <log>
# <codec args>...: the cut under <limit> bytes, staged in WORK and moved to
# <out> only once it fits. The crf steps up by 4 twice, then the frame rate
# drops to 24 and the crf steps again; the width stays.
encode_under() {
  local out=$1 limit=$2 scale=$3 fps=$4 crf=$5 mov=$6 point=$7 length=$8 logfile=$9 rate step size=0
  local tmp
  shift 9
  tmp="$WORK/encode-$(basename "$out")"
  for rate in "$fps" 24; do
    for step in 0 4 8; do
      ffmpeg -y -ss "$point" -i "$mov" -t "$length" -an \
        -vf "scale=$scale:-2:flags=lanczos,fps=$rate" "$@" -crf $(( crf + step )) \
        "$tmp" >>"$logfile" 2>&1
      size=$(stat -f%z "$tmp")
      if [ "$size" -le "$limit" ]; then
        /bin/mv -f "$tmp" "$out"
        log "$(basename "$out"): $(( size / 1024 )) KB at crf $(( crf + step )), $rate fps, $(seconds_of "$out")s of ${length}s"
        return 0
      fi
    done
    [ "$rate" -gt 24 ] || break
  done
  echo "$(basename "$out") is $size bytes at crf $(( crf + 8 )) and $rate fps, over $limit; see $logfile" >&2
  exit 1
}

# encode_loop <take> [scale fps webm-crf mp4-crf max-seconds]: the take cut to
# its markers, as the mp4 and webm pair the site plays. The defaults are the
# chat loop's; max-seconds 0 is no limit. The cut stays beside the take as
# <take>.cut for the README GIF.
TAKES=""
encode_loop() {
  local take=$1 scale=${2:-1320} fps=${3:-30} webm_crf=${4:-36} mp4_crf=${5:-24} max=${6:-0}
  local mov="$WORK/$1.mov" logfile="$WORK/encode-$1.log" taken point length
  [ "$REC_LIVE" -eq 1 ] || { echo "$take: the recording ended before the take did, see $WORK/$take.rec.log" >&2; exit 1; }
  [ -s "$mov" ] || { echo "$take: no recording at $mov, see $WORK/$take.rec.log" >&2; exit 1; }
  taken=$(seconds_of "$mov")
  read -r point length <<<"$(python3 -c "s = $REC_T_STOP - $taken; lead = $REC_T_OPEN - s; i = max(0.0, lead - 0.6); print('late %.2f' % -lead if lead < 0 else '%.3f %.3f' % (i, max(0.1, $REC_T_END - s - i)))")"
  if [ "$point" = late ]; then
    echo "$take: the recording began ${length}s after the take's first action; run the scene again" >&2
    exit 1
  fi
  if [ "$max" != 0 ] && awk -v a="$length" -v b="$max" 'BEGIN { exit !(a > b) }'; then
    echo "$take: the take ran ${length}s, over ${max}s (a key waiting for idle time?); run the scene again" >&2
    exit 1
  fi
  printf '%s %s\n' "$point" "$length" >"$WORK/$take.cut"
  mkdir -p "$MEDIA_OUT"
  : >"$logfile"
  encode_under "$MEDIA_OUT/$take.mp4" "$MP4_LIMIT" "$scale" "$fps" "$mp4_crf" "$mov" "$point" "$length" "$logfile" \
    -c:v libx264 -preset slow -pix_fmt yuv420p -movflags +faststart
  encode_under "$MEDIA_OUT/$take.webm" "$WEBM_LIMIT" "$scale" "$fps" "$webm_crf" "$mov" "$point" "$length" "$logfile" \
    -c:v libvpx-vp9 -b:v 0 -row-mt 1
  TAKES="$TAKES $take"
}

# encode_gif <take> <out>: the take's own cut, from the raw recording, as a
# looping GIF under GIF_LIMIT. Two passes, palettegen then paletteuse without
# dithering, so text edges stay on palette colours. Lower frame rates come
# before narrower widths.
encode_gif() {
  local take=$1 out=$2 point length width rate size=0
  local tmp="$WORK/$1.gif" palette="$WORK/$1-palette.png" logfile="$WORK/encode-$1-gif.log"
  read -r point length <"$WORK/$take.cut"
  : >"$logfile"
  for width in 1320 1100 960; do
    for rate in 15 12 10; do
      ffmpeg -y -ss "$point" -t "$length" -i "$WORK/$take.mov" \
        -vf "fps=$rate,scale=$width:-2:flags=lanczos,palettegen=stats_mode=full" "$palette" >>"$logfile" 2>&1
      ffmpeg -y -ss "$point" -t "$length" -i "$WORK/$take.mov" -i "$palette" \
        -lavfi "fps=$rate,scale=$width:-2:flags=lanczos[x];[x][1:v]paletteuse=dither=none:diff_mode=rectangle" \
        "$tmp" >>"$logfile" 2>&1
      size=$(stat -f%z "$tmp")
      if [ "$size" -le "$GIF_LIMIT" ]; then
        mkdir -p "$(dirname "$out")"
        /bin/mv -f "$tmp" "$out"
        log "$(basename "$out"): $(( size / 1024 )) KB at ${width}px, $rate fps"
        return 0
      fi
    done
  done
  echo "$(basename "$out") is $size bytes at 960px and 10 fps, over $GIF_LIMIT; see $logfile" >&2
  exit 1
}

# ---------------------------------------------------------------- finder ----

open_finder() {
  local folder=$1 right=$(( WIN_X + W )) bottom=$(( WIN_Y + H ))
  FINDER_WINDOW=$(osascript <<AS
tell application "Finder"
  activate
  set f to POSIX file "$folder" as alias
  set w to make new Finder window to f
  set current view of w to list view
  set toolbar visible of w to true
  set statusbar visible of w to false
  set sidebar width of w to 0
  set bounds of w to {$WIN_X, $WIN_Y, $right, $bottom}
  set opts to list view options of w
  set calculates folder sizes of opts to false
  set icon size of opts to small
  set sort column of opts to name column
  return id of w
end tell
AS
)
  sleep 1.5
  # The path bar would print the scratch path under the listing.
  if osascript -e 'tell application "System Events" to tell process "Finder" to click menu item "Hide Path Bar" of menu "View" of menu bar item "View" of menu bar 1' >/dev/null 2>&1; then
    PATH_BAR_HIDDEN=1
    "$DRIVE" stamp
    sleep 0.8
  fi
}
close_finder() {
  if [ "$PATH_BAR_HIDDEN" -eq 1 ]; then
    osascript -e 'tell application "System Events" to tell process "Finder" to click menu item "Show Path Bar" of menu "View" of menu bar item "View" of menu bar 1' >/dev/null 2>&1 || true
    "$DRIVE" stamp
    PATH_BAR_HIDDEN=0
  fi
  [ -n "$FINDER_WINDOW" ] || return 0
  osascript -e "tell application \"Finder\" to close (every window whose id is $FINDER_WINDOW)" >/dev/null 2>&1 || true
  FINDER_WINDOW=""
}
capture_finder() {
  local file=$1
  screencapture -l "$FINDER_WINDOW" -x -o -t png "$file"
  node "$HERE/shrink.mjs" "$file"
  CAPTURED+=("$file")
}

# ---------------------------------------------------------------- scenes ----

hero_suffix() { case "$SHELL_KIND" in mac) echo "" ;; *) echo "-$SHELL_KIND" ;; esac; }

# The file tree and search in the sidebar, no app, and the cursor on a list
# line with a link so that line shows its markup and the rest renders.
scene_hero_window() {
  reset_state
  if [ "$SIZE_GIVEN" -eq 0 ]; then W=1440; H=900; fi
  begin "hero-window$(hero_suffix)"
  open_note "Garden committee 10 Sep"
  go_to_line 14
  shoot "hero-window$(hero_suffix)"
  quit_app
}

scene_text_file() {
  reset_state
  begin text-file
  open_note "To do"
  shoot text-file
  quit_app
}

# The blank line under the title holds the cursor, so every block renders.
scene_markdown_inline() {
  reset_state
  begin markdown-inline
  open_note "Sourdough notes"
  go_to_line 6
  sleep 2
  shoot markdown-inline
  quit_app
}

scene_notes_folder() {
  reset_state
  local dir="$WORK/notes-folder"
  /bin/rm -rf "$dir"; mkdir -p "$dir"
  /bin/cp -R "$FIXTURE/." "$dir/Notes/"
  date_fixture "$dir/Notes"
  wait_idle
  open_finder "$dir/Notes"
  shoot_system notes-folder capture_finder
  close_finder
}

scene_connections() {
  reset_state
  apps_on connections
  SIDEBAR_OPEN=false; PANEL_OPEN=true
  begin connections
  open_note "Lisbon in October"
  shoot connections
  quit_app
}

scene_graph_folder() {
  reset_state
  apps_on graph
  begin graph-folder
  open_note "Garden plan"
  run_command "Open graph"
  sleep 2
  key tab; sleep 0.3
  typetext "Garden plan"; sleep 1.5
  shoot graph-folder
  quit_app
}

scene_graph_local() {
  reset_state
  apps_on connections graph
  PANEL_OPEN=true
  begin graph-local
  open_note "Garden plan"
  shoot graph-local
  quit_app
}

scene_search() {
  reset_state
  # With the folder also open as the workspace, every hit lists twice (once
  # from the file index, once from the folder search). Two .txt files and
  # eight .md files mention compost.
  SIDEBAR_OPEN=false; WORKSPACE=0
  begin search
  open_note "Garden plan"
  key f cmd,shift; sleep 0.5
  typetext "compost"; sleep 1.5
  shoot search
  quit_app
}

scene_preview_rich() {
  reset_state
  SIDEBAR_OPEN=false
  begin preview-rich
  open_note "Sourdough notes"
  sleep 3
  shoot preview-rich
  quit_app
}

# Settings, Apps with Graph on and the other five off. "nearby" is a search
# word only the Graph row carries, so the palette's first row opens it.
scene_apps() {
  reset_state
  apps_on graph
  begin apps
  open_note "Garden plan"
  open_setting "nearby"
  sleep 1
  shoot apps
  quit_app
}

scene_chat() {
  local theme bounds x y w h composer apply
  start_stub
  for theme in $THEMES; do
    reset_state
    apps_on chat
    AI_PROVIDER=custom; AI_BASE_URL="http://127.0.0.1:$STUB_PORT/v1"; AI_MODEL=local-model
    POLARITY="$theme"
    begin chat
    open_note "Birthday ideas"
    bounds=$(window_bounds); read -r x y w h <<<"$bounds"
    "$DRIVE" click "$APP_PID" $(( x + w / 2 )) $(( y + h / 2 ))
    sleep 0.5
    record_start "chat-$theme"
    key a cmd,shift
    # The composer is a textarea with role="combobox" (it lists @ mentions),
    # so the accessibility tree names it a combo box.
    if ! composer=$(wait_for_element AXComboBox Message 10); then
      key a cmd,shift
      composer=$(wait_for_element AXComboBox Message 10) \
        || { echo "the chat pane did not open" >&2; exit 1; }
    fi
    click_element "$composer"
    typetext "@Sour"
    sleep 0.8
    key return
    sleep 0.6
    typetext "Can you sort these so the cheap ones come first?"
    sleep 0.5
    key return
    apply=$(wait_for_element AXButton Apply 30) \
      || { echo "no proposal arrived, see $WORK/stub.log" >&2; exit 1; }
    sleep 1.2
    capture_window "$OUT/chat-$theme.png"
    click_element "$apply"
    sleep 2.5
    record_stop
    quit_app
    encode_loop "chat-$theme"
  done
  stop_stub
}

scene_versions() {
  reset_state
  local dir="$WORK/versions"
  /bin/rm -rf "$dir"; mkdir -p "$dir/data"
  bash "$HERE/seed-history.sh" "$dir/data" "Newsletter draft.md" \
    "$VERSIONS/newsletter-draft-1.md" "$VERSIONS/newsletter-draft-2.md" "$VERSIONS/newsletter-draft-3.md"
  PRESEED=1
  begin versions
  open_note "Newsletter draft"
  run_command "Revert to"
  sleep 1.5
  shoot versions
  quit_app
}

scene_activity() {
  reset_state
  apps_on programs
  EXTRA_CONFIG='
[[mcp.approved_clients]]
name = "Scribe CLI"
first_seen = "2026-09-11T09:12:00Z"
read = true
write = false
'
  begin activity
  sleep 3
  node "$HERE/mcp-probe.mjs" "$CLI_BIN" "$DATA" "$NOTES" | sed 's/^/  probe: /'
  # A second program nobody approved, so the panel also shows one waiting.
  node "$HERE/mcp-probe.mjs" "$CLI_BIN" "$DATA" "$NOTES" "Desk helper" | sed 's/^/  probe: /'
  open_note "Garden plan"
  run_command "Activity"
  sleep 1.5
  shoot activity
  quit_app
}

scene_settings_appearance() {
  reset_state
  begin settings-appearance
  open_note "Garden plan"
  open_setting "Accent color"
  sleep 1
  shoot settings-appearance
  quit_app
}

# The programs row at three interface text sizes, plus the narrowest window the
# app allows, into .status rather than the site's assets.
scene_settings_programs() {
  local clients size theme
  clients='
[[mcp.approved_clients]]
name = "Scribe CLI"
first_seen = "2026-09-11T09:12:00Z"
read = true
write = true

[[mcp.approved_clients]]
name = "Desk helper"
first_seen = "2026-09-12T10:04:00Z"
read = true
write = false
'
  for size in 12 16 22; do
    reset_state
    apps_on programs
    APPEARANCE_EXTRA="interface_text_size = $size"
    EXTRA_CONFIG="$clients"
    begin "settings-programs-$size"
    open_note "Garden plan"
    open_setting "What a program can do"
    sleep 1
    for theme in $THEMES; do
      set_polarity "$theme"
      capture_window "$PROGRAMS_OUT/programs-$size-$theme.png"
    done
    quit_app
  done

  reset_state
  apps_on programs
  W=720; H=600
  APPEARANCE_EXTRA="interface_text_size = 22"
  EXTRA_CONFIG="$clients"
  begin settings-programs-22-narrow
  open_note "Garden plan"
  open_setting "What a program can do"
  sleep 1
  for theme in $THEMES; do
    set_polarity "$theme"
    capture_window "$PROGRAMS_OUT/programs-22-narrow-$theme.png"
  done
  quit_app
}

scene_obsidian_folder() {
  reset_state
  apps_on connections
  PANEL_OPEN=true
  begin obsidian-folder
  open_note "Moving house"
  # Unfold the folder in the tree: its row sits under the section head.
  local bounds x y w h
  bounds=$(window_bounds); read -r x y w h <<<"$bounds"
  "$DRIVE" click "$APP_PID" $(( x + 109 )) $(( y + 95 ))
  sleep 0.8
  shoot obsidian-folder
  quit_app
}

# No config file and an empty folder: the format step, before any file.
scene_first_run() {
  reset_state
  SEED_CONFIG=0; EMPTY_NOTES=1
  remember_system_appearance
  set_system_dark false
  begin first-run
  "$DRIVE" place "$APP_PID" "$WIN_X" "$WIN_Y" "$W" "$H"
  sleep 1
  shoot_system first-run capture_window
  quit_app
}

scene_tags() {
  reset_state
  apps_on tags
  begin tags
  open_note "Garden plan"
  shoot tags
  quit_app
}

# ----------------------------------------------------------------- loops ----

# loop_take <name> <setup fn> <take fn> <scale> <fps> <webm crf> <mp4 crf>
# <max seconds>: per theme, a fresh instance brought to the take's first frame
# by <setup fn> (given its scratch name), recorded while <take fn> runs, and
# encoded to <name>-<theme>.mp4 and .webm with those settings. The take ends
# on the frame the loop holds before it starts over.
loop_take() {
  local name=$1 setup=$2 take=$3 theme
  shift 3
  for theme in $THEMES; do
    reset_state
    POLARITY=$theme
    "$setup" "loop-$name-$theme"
    pointer_away
    sleep 0.8
    record_start "$name-$theme"
    "$take"
    record_stop
    quit_app
    encode_loop "$name-$theme" "$@"
  done
}

# take_palette <key> <text>: the Cmd+Shift+<key> palette, <text>, Return, at a
# take's pace rather than open_note's.
take_palette() {
  key "$1" cmd,shift; sleep 0.4
  typetext "$2"; sleep 0.6
  key return; sleep 0.7
}

loop_any_file_setup() {
  begin "$1"
  open_note "To do"
  go_to_line 9
  key end
}
loop_any_file_take() {
  key return; sleep 0.25
  type_human "- Take the recycling out on Tuesday"
  sleep 1.2
  take_palette o "Server log"
  sleep 1.2
}

LAYOUT_SOURCE=""
LAYOUT_INLINE=""
loop_markdown_setup() {
  begin "$1"
  open_note "Sourdough notes"
  key down cmd
  LAYOUT_SOURCE=$(wait_for_element AXRadioButton Source 5) || { echo "no Source switch in the status bar" >&2; exit 1; }
  LAYOUT_INLINE=$(wait_for_element AXRadioButton Inline 5) || { echo "no Inline switch in the status bar" >&2; exit 1; }
}
# Return continues a list item and its task box, and a second Return on the
# empty item ends the list, so the items after the first are typed bare.
loop_markdown_take() {
  local line
  key return
  for line in "## Saturday" "- 78% water" "Cold proof" "[ ] Buy rye"; do
    type_human "$line"; sleep 0.25
    key return
  done
  key return; sleep 0.4
  click_element "$LAYOUT_SOURCE"; sleep 1
  click_element "$LAYOUT_INLINE"
  pointer_away; sleep 1
}
scene_loop_markdown() {
  loop_take markdown loop_markdown_setup loop_markdown_take 1320 30 32 20 12
  readme_gifs
}

loop_search_setup() {
  SIDEBAR_OPEN=false; WORKSPACE=0
  begin "$1"
  open_note "Garden plan"
}
loop_search_take() {
  key f cmd,shift; sleep 0.6
  type_human "compost"; sleep 1.4
  for _ in 1 2 3; do key down; sleep 0.3; done
  key return; sleep 1.8
}

loop_apps_setup() {
  begin "$1"
  open_note "Garden plan"
}
loop_apps_take() {
  local rect
  take_palette f "nearby"; sleep 0.4
  if rect=$(wait_for_element AXCheckBox Graph 3); then
    click_element "$rect"
  else
    log "loop-apps: no Graph switch in the accessibility tree, switching it in the config"
    sed -i '' -e 's/^graph = false$/graph = true/' "$DATA/config.toml"
  fi
  sleep 1
  pointer_away
  key escape; sleep 1
  take_palette f "> Open graph"
  sleep 1.6
}

loop_versions_setup() {
  local dir="$WORK/$1"
  /bin/rm -rf "$dir"; mkdir -p "$dir/data"
  bash "$HERE/seed-history.sh" "$dir/data" "Newsletter draft.md" \
    "$VERSIONS/newsletter-draft-1.md" "$VERSIONS/newsletter-draft-2.md" "$VERSIONS/newsletter-draft-3.md"
  PRESEED=1
  begin "$1"
  open_note "Newsletter draft"
}
# The dialog opens with focus on its close button; Tab reaches the selected
# version and Down selects the next one in the list.
loop_versions_take() {
  local rect
  take_palette f "> Revert to"
  key tab; sleep 0.2
  key down; sleep 0.8
  rect=$(wait_for_element AXButton "Restore this version" 5) || { echo "no Restore this version button" >&2; exit 1; }
  click_element "$rect"; sleep 0.8
  rect=$(wait_for_element AXButton "Close versions" 5) || { echo "no Close versions button" >&2; exit 1; }
  click_element "$rect"
  pointer_away; sleep 1.2
}

run_scene() {
  case "$1" in
    hero-window) scene_hero_window ;;
    hero-window-win) SHELL_KIND=win scene_hero_window; SHELL_KIND=mac ;;
    hero-window-linux) SHELL_KIND=linux scene_hero_window; SHELL_KIND=mac ;;
    text-file) scene_text_file ;;
    markdown-inline) scene_markdown_inline ;;
    apps) scene_apps ;;
    notes-folder) scene_notes_folder ;;
    connections) scene_connections ;;
    graph-folder) scene_graph_folder ;;
    graph-local) scene_graph_local ;;
    search) scene_search ;;
    preview-rich) scene_preview_rich ;;
    chat) scene_chat ;;
    versions) scene_versions ;;
    activity) scene_activity ;;
    settings-appearance) scene_settings_appearance ;;
    settings-programs) scene_settings_programs ;;
    obsidian-folder) scene_obsidian_folder ;;
    first-run) scene_first_run ;;
    tags) scene_tags ;;
    loop-any-file) loop_take any-file loop_any_file_setup loop_any_file_take 1320 30 32 20 12 ;;
    loop-markdown) scene_loop_markdown ;;
    loop-search) loop_take search loop_search_setup loop_search_take 1320 30 32 20 12 ;;
    loop-apps) loop_take apps loop_apps_setup loop_apps_take 1320 30 32 20 12 ;;
    loop-versions) loop_take versions loop_versions_setup loop_versions_take 1320 30 32 20 12 ;;
    *) echo "no scene function for $1" >&2; exit 2 ;;
  esac
}

# ---------------------------------------------------------- readme media ----

# readme_gifs: this run's loop-markdown takes as the README's hero GIFs.
README_GIF_TAKE=markdown
README_GIFS_WRITTEN=0
readme_gifs() {
  local theme
  for theme in light dark; do
    case " $TAKES " in *" $README_GIF_TAKE-$theme "*) ;; *) continue ;; esac
    encode_gif "$README_GIF_TAKE-$theme" "$README_OUT/hero-$theme.gif"
    README_GIFS_WRITTEN=1
  done
}

# retire_hero_pngs: the README's hero PNGs go once this run wrote a hero GIF
# and both GIFs are on disk.
retire_hero_pngs() {
  [ "$README_GIFS_WRITTEN" -eq 1 ] || return 0
  if [ -f "$README_OUT/hero-light.gif" ] && [ -f "$README_OUT/hero-dark.gif" ]; then
    /bin/rm -f "$README_OUT/hero-light.png" "$README_OUT/hero-dark.png"
  fi
}

# ------------------------------------------------------------------ main ----

preflight
needs_release=0
for scene in "${WANTED[@]}"; do
  case "$scene" in hero-window-win|hero-window-linux|notes-folder) ;; *) needs_release=1 ;; esac
done
if [ "$SHELL_KIND" = mac ] && [ "$needs_release" -eq 1 ]; then build_release; fi
if [ "$SHELL_KIND" != mac ]; then
  for scene in "${WANTED[@]}"; do
    case "$scene" in hero-window) ;; *) echo "--shell $SHELL_KIND only applies to hero-window" >&2; exit 2 ;; esac
  done
fi

for scene in "${WANTED[@]}"; do
  run_scene "$scene"
done
retire_hero_pngs

# The contact sheet covers every scene on disk, not only this run's.
SHEET=()
for scene in "${SCENES[@]}" "${SHELL_SCENES[@]}"; do
  for theme in light dark; do
    [ -f "$OUT/$scene-$theme.png" ] && SHEET+=("$OUT/$scene-$theme.png")
  done
done
if [ ${#SHEET[@]} -gt 0 ]; then
  node "$HERE/contact.mjs" "$SHOTS/captures-contact.png" "${SHEET[@]}"
fi
report_media
log "done: ${#CAPTURED[@]} files in $OUT"
