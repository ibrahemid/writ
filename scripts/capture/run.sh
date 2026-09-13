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
#   --all             every scene, the two shell heroes included
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
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
ROOT=$(cd "$HERE/../.." && pwd)
OUT="$ROOT/site/src/assets/captures"
SHOTS="$ROOT/.status/v2/shots"
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
    -h|--help) sed -n '2,25p' "$0"; exit 0 ;;
    *) echo "unknown flag $1" >&2; exit 2 ;;
  esac
done
if [ ${#WANTED[@]} -eq 0 ]; then
  echo "nothing to do: pass --scene <name> or --all" >&2
  exit 2
fi
for scene in "${WANTED[@]}"; do
  case " ${SCENES[*]} ${SHELL_SCENES[*]} " in
    *" $scene "*) ;;
    *) echo "unknown scene $scene" >&2; exit 2 ;;
  esac
done

log() { printf '%s  %s\n' "$(date +%H:%M:%S)" "$*"; }

# ------------------------------------------------------------- preflight ----

APP_PID=""
DEV_PID=""
STUB_PID=""
FINDER_WINDOW=""
PATH_BAR_HIDDEN=0
SYSTEM_DARK_BEFORE=""
CAPTURED=()

cleanup() {
  local status=$?
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
  for tool in cargo node sqlite3 swiftc screencapture osascript shasum; do
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
  mkdir -p "$OUT" "$SHOTS" "$WORK"
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
LAYOUT=source
EXTRA_CONFIG=""
SEED_CONFIG=1
EMPTY_NOTES=0
PRESEED=0
BLANK_TAB=0
WORKSPACE=1

reset_state() {
  W=${SIZE%x*}; H=${SIZE#*x}
  set -- $THEMES; POLARITY=$1
  SIDEBAR_OPEN=true; COLLAPSED='[]'; PANEL_OPEN=false; CHAT_OPEN=false; LAYOUT=source
  EXTRA_CONFIG=""; SEED_CONFIG=1; EMPTY_NOTES=0; PRESEED=0; WORKSPACE=1
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

[spelling]
enabled = false

[first_run]
hint_dismissed = true

[updater]
auto_check = false
$EXTRA_CONFIG
CFG
}

# Modification times a folder of notes would carry, so Finder and the
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
  COLLAPSED='["tags"]'; PANEL_OPEN=true; LAYOUT=source
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
  SIDEBAR_OPEN=false; PANEL_OPEN=true
  begin connections
  open_note "Lisbon in October"
  shoot connections
  quit_app
}

scene_graph_folder() {
  reset_state
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
  SIDEBAR_OPEN=false; LAYOUT="split"
  begin preview-rich
  open_note "Sourdough notes"
  sleep 3
  shoot preview-rich
  quit_app
}

scene_chat() {
  reset_state
  EXTRA_CONFIG="
[ai]
enabled = true
preset = \"custom\"
base_url = \"http://127.0.0.1:$STUB_PORT/v1\"
model = \"local-model\"

[ai.chat]
enabled = true
provider = \"openai_compatible\"
base_url = \"http://127.0.0.1:$STUB_PORT/v1\"
model = \"local-model\"
"
  start_stub
  begin chat
  open_note "Birthday ideas"
  run_command "Chat"
  sleep 1
  local bounds x y w h
  bounds=$(window_bounds); read -r x y w h <<<"$bounds"
  "$DRIVE" click "$APP_PID" $(( x + w - 190 )) $(( y + h - 64 ))
  sleep 0.4
  typetext "Can you sort these so the cheap ones come first?"
  key return
  sleep 3
  shoot chat
  quit_app
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
  EXTRA_CONFIG='
[mcp]
enabled = true

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

scene_obsidian_folder() {
  reset_state
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
