#!/usr/bin/env bash
# Independent preparation can overlap; bundling must wait for both to succeed.
set -euo pipefail
cd "$(dirname "$0")/.."
started=$SECONDS
npm run build &
frontend_pid=$!
bash scripts/prepare-sidecar.sh &
cli_pid=$!
status=0
wait "$frontend_pid" || status=1
wait "$cli_pid" || status=1
printf 'Frontend and CLI preparation: %s seconds\n' "$((SECONDS - started))"
exit "$status"
