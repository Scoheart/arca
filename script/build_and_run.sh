#!/usr/bin/env bash
set -euo pipefail

MODE="${1:-run}"
APP_BUNDLE_ID="com.scoheart.arca.dev"
DEV_LOG_FILE="/tmp/com.scoheart.arca-dev.log"
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DESKTOP_DIR="$ROOT_DIR/desktop"

stop_previous_run() {
  python3 "$DESKTOP_DIR/scripts/dev-processes.py" stop
}

run_tauri() {
  cd "$DESKTOP_DIR"
  npm run tauri dev
}

wait_for_app() {
  python3 "$DESKTOP_DIR/scripts/dev-processes.py" wait
}

stop_previous_run

case "$MODE" in
  run)
    cd "$DESKTOP_DIR"
    exec npm run tauri dev
    ;;
  --debug|debug)
    export RUST_BACKTRACE=1
    export RUST_LOG=debug
    cd "$DESKTOP_DIR"
    exec npm run tauri dev
    ;;
  --logs|logs)
    run_tauri >"$DEV_LOG_FILE" 2>&1 &
    dev_pid="$(wait_for_app)"
    /usr/bin/log stream --info --style compact --predicate "processID == $dev_pid"
    ;;
  --telemetry|telemetry)
    run_tauri >"$DEV_LOG_FILE" 2>&1 &
    wait_for_app
    /usr/bin/log stream --info --style compact --predicate "subsystem == \"$APP_BUNDLE_ID\""
    ;;
  --verify|verify)
    run_tauri >"$DEV_LOG_FILE" 2>&1 &
    if dev_pid="$(wait_for_app)"; then
      echo "Arca Dev is running (PID $dev_pid)."
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
