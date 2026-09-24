#!/usr/bin/env bash
# Captures the stills the site uses, from a release build of the current
# branch, driven by keystrokes posted to that one process and captured by its
# CGWindowID. Nothing here touches ~/.writ or ~/Writ; the instance runs on a
# scratch copy of scripts/capture/fixtures.
#
#   scripts/capture/run.sh --all
#   scripts/capture/run.sh --scene hero-window --scene search --theme light
#   scripts/capture/run.sh --scene hero-window --shell win
#
# Flags
#   --scene <name>    one scene (repeatable); names are listed under SCENES
#                     and REPORT_SCENES
#   --all             every scene in SCENES, the two shell heroes included
#                     (REPORT_SCENES only run when named)
#   --theme           light | dark | both (default both)
#   --shell           mac | win | linux (default mac; win and linux run a dev
#                     instance built with VITE_WRIT_PLATFORM, so they are slower)
#   --size            1280x800 | 1440x900 (default 1280x800; hero-window is
#                     1440x900 unless --size is given)
#   --no-build        reuse the bundle from the last build
#
# Every key and click waits until this instance is the frontmost app and the
# machine has been idle for 45 s; the run refuses to start while any other
# Writ process exists.
#
# Every scene's config switches on only the apps that scene shows (apps_on);
# the rest are off, as in a fresh config.
#
# The chat scene also records its window while the pane is driven and encodes
# the take to site/public/media/chat-<theme>.mp4 and .webm, the pair
# Loop.astro plays with the still as the poster.
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
STUB_PORT=8791
DEV_PORT=1450
WIN_X=120
WIN_Y=100

SCENES=(hero-window notes-folder connections graph-folder graph-local search preview-rich chat versions activity settings-appearance obsidian-folder today tags)
# Named only: report stills, not site assets.
REPORT_SCENES=(settings-programs)
SHELL_SCENES=(hero-window-win hero-window-linux)

# ---------------------------------------------------------------- flags ----

WANTED=()
THEMES="light dark"
SHELL_KIND=mac
SIZE=1280x800
SIZE_GIVEN=0
NO_BUILD=0
while [ $# -gt 0 ]; do
  case "$1" in
    --scene) WANTED+=("$2"); shift 2 ;;
    --all) WANTED=("${SCENES[@]}" "${SHELL_SCENES[@]}"); shift ;;
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
    -h|--help) sed -n '2,32p' "$0"; exit 0 ;;
    *) echo "unknown flag $1" >&2; exit 2 ;;
  esac
done
if [ ${#WANTED[@]} -eq 0 ]; then
  echo "nothing to do: pass --scene <name> or --all" >&2
  exit 2
fi
for scene in "${WANTED[@]}"; do
  case " ${SCENES[*]} ${SHELL_SCENES[*]} ${REPORT_SCENES[*]} " in
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
REC_T_STOP=0
REC_T_OPEN=0
REC_T_END=0
FINDER_WINDOW=""
PATH_BAR_HIDDEN=0
SYSTEM_DARK_BEFORE=""
CAPTURED=()

cleanup() {
  local status=$?
  record_stop || true
  quit_app || true
  stop_stub || true
  close_finder || true
  restore_system_appearance || true
  if [ "$status" -ne 0 ]; then
    log "failed (exit $status); scratch kept at $WORK"
  fi
}
trap cleanup EXIT

preflight() {
  for tool in cargo node sqlite3 swiftc screencapture ffmpeg ffprobe osascript shasum; do
    command -v "$tool" >/dev/null || { echo "missing tool: $tool" >&2; exit 1; }
  done
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
  wait_idle
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
  (cd "$ROOT" && CARGO_PROFILE_RELEASE_STRIP=false cargo tauri build --bundles app \
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
      WRIT_DEV_PORT="$DEV_PORT" cargo tauri dev --no-watch \
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
open_setting() {
  key f cmd,shift; sleep 0.5
  typetext "$1"; sleep 0.8
  key return; sleep 1.2
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

# record_start <file>: the window, until record_stop. The file is written
# when screencapture exits; -V 90 is the ceiling for a stop it does not take.
record_start() {
  local file=$1 bounds x y w h
  bounds=$(window_bounds)
  [ -n "$bounds" ] || { echo "record_start: no window" >&2; exit 1; }
  read -r x y w h <<<"$bounds"
  screencapture -v -x -R "$x,$y,$w,$h" -V 90 "$file" &
  REC_PID=$!
  sleep 1.5
}

# The take's clock runs backwards from the moment the recording stopped, so
# the markers do not depend on how long screencapture took to start.
record_stop() {
  [ -n "$REC_PID" ] || return 0
  kill -INT "$REC_PID" 2>/dev/null || true
  wait "$REC_PID" 2>/dev/null || true
  REC_PID=""
  REC_T_STOP=$(now)
}

MP4_LIMIT=1258291
WEBM_LIMIT=838860

seconds_of() { ffprobe -v error -show_entries format=duration -of csv=p=0 "$1"; }

# encode_loop <mov> <name>: the take cut to the markers, as the mp4 and webm
# pair the site plays.
encode_loop() {
  local mov=$1 name=$2 dir logfile point length taken crf size try
  dir="$ROOT/site/public/media"
  logfile="$WORK/encode-$name.log"
  mkdir -p "$dir"
  : >"$logfile"
  taken=$(seconds_of "$mov")
  if [ "$(python3 -c "print(1 if $REC_T_END > $REC_T_STOP + 0.2 else 0)")" = 1 ]; then
    echo "$name: the recording ended before the take did (${taken}s on disk)" >&2
    exit 1
  fi
  read -r point length <<<"$(python3 -c "s = $REC_T_STOP - $taken; i = max(0.0, $REC_T_OPEN - s - 0.6); o = $REC_T_END - s; print('%.3f %.3f' % (i, max(0.1, o - i)))")"

  crf=24
  for try in 1 2 3; do
    ffmpeg -y -ss "$point" -i "$mov" -t "$length" -an \
      -vf "scale=1320:-2:flags=lanczos,fps=30" \
      -c:v libx264 -preset slow -crf "$crf" -pix_fmt yuv420p -movflags +faststart \
      "$dir/$name.mp4" >>"$logfile" 2>&1
    size=$(stat -f%z "$dir/$name.mp4")
    [ "$size" -le "$MP4_LIMIT" ] && break
    [ "$try" -eq 3 ] && { echo "$name.mp4 is $size bytes at crf $crf, over $MP4_LIMIT; see $logfile" >&2; exit 1; }
    crf=$(( crf + 4 ))
  done
  log "$name: mp4 $(( size / 1024 )) KB at crf $crf, $(seconds_of "$dir/$name.mp4")s of ${length}s"

  crf=36
  for try in 1 2 3; do
    ffmpeg -y -ss "$point" -i "$mov" -t "$length" -an \
      -vf "scale=1320:-2:flags=lanczos,fps=30" \
      -c:v libvpx-vp9 -b:v 0 -crf "$crf" -row-mt 1 \
      "$dir/$name.webm" >>"$logfile" 2>&1
    size=$(stat -f%z "$dir/$name.webm")
    [ "$size" -le "$WEBM_LIMIT" ] && break
    [ "$try" -eq 3 ] && { echo "$name.webm is $size bytes at crf $crf, over $WEBM_LIMIT; see $logfile" >&2; exit 1; }
    crf=$(( crf + 4 ))
  done
  log "$name: webm $(( size / 1024 )) KB at crf $crf, $(seconds_of "$dir/$name.webm")s of ${length}s"
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

scene_hero_window() {
  reset_state
  if [ "$SIZE_GIVEN" -eq 0 ]; then W=1440; H=900; fi
  begin "hero-window$(hero_suffix)"
  open_note "Garden committee 10 Sep"
  shoot "hero-window$(hero_suffix)"
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
  # With the notes folder also open as the workspace, every hit lists twice
  # (once from the note index, once from the folder search).
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
    record_start "$WORK/chat-$theme.mov"
    REC_T_OPEN=$(now)
    key a cmd,shift
    if ! composer=$(wait_for_element AXTextArea Message 10); then
      key a cmd,shift
      composer=$(wait_for_element AXTextArea Message 10) \
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
    REC_T_END=$(now)
    record_stop
    quit_app
    encode_loop "$WORK/chat-$theme.mov" "chat-$theme"
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

scene_today() {
  reset_state
  SEED_CONFIG=0; EMPTY_NOTES=1
  remember_system_appearance
  set_system_dark false
  begin today
  "$DRIVE" place "$APP_PID" "$WIN_X" "$WIN_Y" "$W" "$H"
  sleep 1
  shoot_system today capture_window
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

run_scene() {
  case "$1" in
    hero-window) scene_hero_window ;;
    hero-window-win) SHELL_KIND=win scene_hero_window; SHELL_KIND=mac ;;
    hero-window-linux) SHELL_KIND=linux scene_hero_window; SHELL_KIND=mac ;;
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
    today) scene_today ;;
    tags) scene_tags ;;
  esac
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
log "done: ${#CAPTURED[@]} files in $OUT"
