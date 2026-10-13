#!/usr/bin/env bash
set -euo pipefail

MODE="${1:-run}"
APP_PROCESS="desktop"
APP_BUNDLE_ID="com.scoheart.arca"
DEV_PORT="1420"
DEV_PID_FILE="/tmp/com.scoheart.arca-dev.pid"
DEV_LOG_FILE="/tmp/com.scoheart.arca-dev.log"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DESKTOP_DIR="$ROOT_DIR/desktop"

stop_previous_run() {
  if [[ -f "$DEV_PID_FILE" ]]; then
    local previous_pid
    previous_pid="$(tr -dc '0-9' < "$DEV_PID_FILE")"
    if [[ -n "$previous_pid" ]] && kill -0 "$previous_pid" >/dev/null 2>&1; then
      kill "$previous_pid" >/dev/null 2>&1 || true
      for _ in {1..20}; do
        kill -0 "$previous_pid" >/dev/null 2>&1 || break
        sleep 0.1
      done
    fi
  fi

  /usr/bin/pkill -x "$APP_PROCESS" >/dev/null 2>&1 || true
  while IFS= read -r port_pid; do
    [[ "$port_pid" =~ ^[0-9]+$ ]] || continue
    kill "$port_pid" >/dev/null 2>&1 || true
  done < <(/usr/sbin/lsof -tiTCP:"$DEV_PORT" -sTCP:LISTEN 2>/dev/null || true)
}

run_tauri() {
  cd "$DESKTOP_DIR"
  npm run tauri dev
}

wait_for_app() {
  for _ in {1..180}; do
    if /usr/bin/pgrep -x "$APP_PROCESS" >/dev/null 2>&1; then
      return 0
    fi
    sleep 1
  done
  return 1
}

stop_previous_run

case "$MODE" in
  run)
    exec bash -c "cd \"$DESKTOP_DIR\" && npm run tauri dev"
    ;;
  --debug|debug)
    export RUST_BACKTRACE=1
    export RUST_LOG=debug
    exec bash -c "cd \"$DESKTOP_DIR\" && npm run tauri dev"
    ;;
  --logs|logs)
    run_tauri >"$DEV_LOG_FILE" 2>&1 &
    echo "$!" > "$DEV_PID_FILE"
    wait_for_app
    /usr/bin/log stream --info --style compact --predicate "process == \"$APP_PROCESS\""
    ;;
  --telemetry|telemetry)
    run_tauri >"$DEV_LOG_FILE" 2>&1 &
    echo "$!" > "$DEV_PID_FILE"
    wait_for_app
    /usr/bin/log stream --info --style compact --predicate "subsystem == \"$APP_BUNDLE_ID\""
    ;;
  --verify|verify)
    run_tauri >"$DEV_LOG_FILE" 2>&1 &
    echo "$!" > "$DEV_PID_FILE"
    if wait_for_app; then
      echo "Arca development app is running (PID $(/usr/bin/pgrep -x "$APP_PROCESS" | head -n 1))."
    else
      tail -n 80 "$DEV_LOG_FILE" >&2 || true
      exit 1
    fi
    ;;
  *)
    echo "usage: $0 [run|--debug|--logs|--telemetry|--verify]" >&2
    exit 2
    ;;
esac
