#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/bin" "$TMP/state"

cat >"$TMP/bin/gh" <<'FAKE_GH'
#!/usr/bin/env bash
set -euo pipefail

echo "$1 $2" >>"$FAKE_STATE/log"

if [[ "$1" == api && "$2" == repos/*/commits/main ]]; then
  echo "$FAKE_MAIN_SHA"
elif [[ "$1" == issue && "$2" == list ]]; then
  requested_state=
  while [[ $# -gt 0 ]]; do
    if [[ "$1" == --state ]]; then
      requested_state=$2
      break
    fi
    shift
  done
  if [[ -f "$FAKE_STATE/issue-state" ]]; then
    state=$(cat "$FAKE_STATE/issue-state")
    if [[ "$requested_state" == all || "${requested_state^^}" == "$state" ]]; then
      printf '17\t%s\n' "$state"
    fi
  fi
elif [[ "$1" == issue && "$2" == create ]]; then
  echo OPEN >"$FAKE_STATE/issue-state"
  printf '%s\n' "$*" >"$FAKE_STATE/create-args"
  echo "https://github.com/Scoheart/mux/issues/17"
elif [[ "$1" == issue && "$2" == edit ]]; then
  printf '%s\n' "$*" >"$FAKE_STATE/edit-args"
elif [[ "$1" == issue && "$2" == reopen ]]; then
  echo OPEN >"$FAKE_STATE/issue-state"
elif [[ "$1" == issue && "$2" == close ]]; then
  echo CLOSED >"$FAKE_STATE/issue-state"
else
  echo "unexpected fake gh invocation: $*" >&2
  exit 64
fi
FAKE_GH
chmod +x "$TMP/bin/gh"

export PATH="$TMP/bin:$PATH"
export FAKE_STATE="$TMP/state"
export FAKE_MAIN_SHA="0000000000000000000000000000000000000002"
export GITHUB_REPOSITORY="Scoheart/arca"
export GITHUB_RUN_ID=1234
export GITHUB_SERVER_URL="https://github.com"
export GITHUB_EVENT_NAME=push
export GITHUB_REF=refs/heads/main
export FAILURE_ISSUE_TITLE="[CI] Automated verification is failing"
export AUTOFIX_CONFIGURED=false
export GITHUB_OUTPUT="$TMP/output"

report() {
  : >"$GITHUB_OUTPUT"
  bash "$ROOT/.github/scripts/quality-failure-monitor.sh" report
}

recover() {
  bash "$ROOT/.github/scripts/quality-failure-monitor.sh" recover
}

# A superseded failure has no Issue side effect.
GITHUB_SHA="0000000000000000000000000000000000000001" report
grep -q '^activated=false$' "$GITHUB_OUTPUT"
! grep -q '^issue create$' "$FAKE_STATE/log"

# The current head creates the sticky Issue once and records inactive repair in its body.
GITHUB_SHA="$FAKE_MAIN_SHA" report
grep -q '^issue_number=17$' "$GITHUB_OUTPUT"
grep -q '^activated=true$' "$GITHUB_OUTPUT"
grep -q 'COPILOT_PAT.*not configured' "$FAKE_STATE/create-args"

# Repeated failure while open edits the body without comments or another repair activation.
GITHUB_SHA="$FAKE_MAIN_SHA" report
grep -q '^activated=false$' "$GITHUB_OUTPUT"
[[ $(grep -c '^issue create$' "$FAKE_STATE/log") -eq 1 ]]
! grep -q '^issue comment$' "$FAKE_STATE/log"

# Recovery closes silently; the next real failure reopens the same Issue number.
recover
[[ $(cat "$FAKE_STATE/issue-state") == CLOSED ]]
GITHUB_SHA="$FAKE_MAIN_SHA" report
grep -q '^issue_number=17$' "$GITHUB_OUTPUT"
grep -q '^activated=true$' "$GITHUB_OUTPUT"
[[ $(grep -c '^issue create$' "$FAKE_STATE/log") -eq 1 ]]
[[ $(grep -c '^issue reopen$' "$FAKE_STATE/log") -eq 1 ]]
! grep -q '^issue comment$' "$FAKE_STATE/log"

echo "Quality failure monitor tests passed."
