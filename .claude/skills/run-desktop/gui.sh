#!/usr/bin/env bash
# Headless launcher / driver for the Arca Tauri desktop app.
# Runs the GUI on a virtual X display (Xvfb), captures screenshots (scrot),
# and simulates mouse/keyboard input (xdotool) — so the app can be driven and
# visually verified in a headless container with no real display.
#
# Usage:
#   gui.sh start            Ensure Xvfb + launch `tauri dev` detached, wait until ready
#   gui.sh shot [path]      Screenshot the virtual display to PNG (default: timestamped tmp file)
#   gui.sh click X Y        Move mouse to (X,Y) and left-click
#   gui.sh type "text"      Type text into the focused field
#   gui.sh key <keyspec>    Press a key (e.g. Return, Escape, ctrl+a)
#   gui.sh status           Show Xvfb / app PIDs and readiness
#   gui.sh stop             Kill the app + vite (leaves Xvfb running)
#   gui.sh restart          stop + start (use after editing data/ or Rust — forces rebuild)
#
# Env overrides: DISPLAY (default :99), SCREEN (default 1280x900x24)
set -uo pipefail

DISPLAY_NUM="${DISPLAY:-:99}"
SCREEN="${SCREEN:-1280x900x24}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# repo root = three levels up from .claude/skills/run-desktop/
REPO_DIR="$(cd "$SCRIPT_DIR/../../.." && pwd)"
DESKTOP_DIR="$REPO_DIR/desktop"
LOG="${ARCA_GUI_LOG:-/tmp/arca-gui.log}"

export DISPLAY="$DISPLAY_NUM"

log() { printf '[gui] %s\n' "$*" >&2; }

ensure_xvfb() {
  if pgrep -af "Xvfb $DISPLAY_NUM" >/dev/null 2>&1; then return 0; fi
  log "starting Xvfb on $DISPLAY_NUM ($SCREEN)"
  setsid bash -c "Xvfb $DISPLAY_NUM -screen 0 $SCREEN >/tmp/arca-xvfb.log 2>&1" </dev/null &
  disown
  for _ in $(seq 1 20); do
    xdotool getdisplaygeometry >/dev/null 2>&1 && return 0
    sleep 0.5
  done
  log "WARN: Xvfb did not come up in time"
}

app_pid() { pgrep -f "target/debug/desktop" | grep -v "$$" | head -1; }

cmd_start() {
  ensure_xvfb
  if [ -n "$(app_pid)" ]; then log "app already running (pid $(app_pid)) — use 'restart' to rebuild"; return 0; fi
  log "launching tauri dev (logs: $LOG)"
  : > "$LOG"
  cd "$DESKTOP_DIR" || exit 1
  # WebKit flags required for software rendering inside the container
  export WEBKIT_DISABLE_COMPOSITING_MODE=1 WEBKIT_DISABLE_DMABUF_RENDERER=1 LIBGL_ALWAYS_SOFTWARE=1
  setsid bash -c "npm run tauri dev >>'$LOG' 2>&1" </dev/null &
  disown
  log "waiting for window (first build can take a few minutes)…"
  for _ in $(seq 1 120); do
    if grep -qaE "Running.*target/debug/desktop" "$LOG" 2>/dev/null; then
      sleep 4; log "READY (app pid $(app_pid))"; return 0
    fi
    if grep -qaE "error\[E[0-9]|could not compile|panicked at" "$LOG" 2>/dev/null; then
      log "BUILD FAILED — tail of $LOG:"; tail -20 "$LOG" >&2; return 1
    fi
    sleep 5
  done
  log "TIMEOUT waiting for app; tail of $LOG:"; tail -20 "$LOG" >&2; return 1
}

cmd_shot() {
  local out="${1:-/tmp/arca-shot-$(date +%H%M%S).png}"
  scrot -o "$out" && echo "$out"
}

cmd_click() { xdotool mousemove "$1" "$2" click 1; log "clicked ($1,$2)"; }
cmd_type()  { xdotool type --delay 40 "$1"; log "typed"; }
cmd_key()   { xdotool key "$1"; log "key $1"; }

cmd_status() {
  printf 'DISPLAY=%s\n' "$DISPLAY_NUM"
  pgrep -af "Xvfb $DISPLAY_NUM" | grep -v pgrep || echo "Xvfb: not running"
  local p; p="$(app_pid)"
  [ -n "$p" ] && echo "app: pid $p" || echo "app: not running"
}

cmd_stop() {
  for p in $(pgrep -f "tauri dev|/vite|target/debug/desktop" | grep -v "$$"); do kill -9 "$p" 2>/dev/null; done
  sleep 1; log "stopped app (Xvfb left running)"
}

case "${1:-}" in
  start)   cmd_start ;;
  shot)    cmd_shot "${2:-}" ;;
  click)   cmd_click "$2" "$3" ;;
  type)    cmd_type "$2" ;;
  key)     cmd_key "$2" ;;
  status)  cmd_status ;;
  stop)    cmd_stop ;;
  restart) cmd_stop; cmd_start ;;
  *) sed -n '2,30p' "${BASH_SOURCE[0]}"; exit 1 ;;
esac
