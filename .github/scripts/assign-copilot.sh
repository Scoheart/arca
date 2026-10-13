#!/usr/bin/env bash
set -euo pipefail

: "${COPILOT_PAT:?COPILOT_PAT is required}"
: "${ISSUE_NUMBER:?ISSUE_NUMBER is required}"
: "${REPOSITORY:?REPOSITORY is required}"

BASE_BRANCH=${BASE_BRANCH:-main}
OWNER=${REPOSITORY%%/*}
API="https://api.github.com/repos/${REPOSITORY}"

issue=$(curl --fail-with-body --silent --show-error \
  -H "Accept: application/vnd.github+json" \
  -H "Authorization: Bearer ${COPILOT_PAT}" \
  -H "X-GitHub-Api-Version: 2022-11-28" \
  "${API}/issues/${ISSUE_NUMBER}")

if [[ $(jq -r '.state' <<<"$issue") != "open" ]]; then
  echo "Issue #${ISSUE_NUMBER} is not open; skipping."
  exit 0
fi

if jq -e '.assignees[]? | select(.login == "copilot-swe-agent[bot]" or .login == "copilot-swe-agent")' \
  >/dev/null <<<"$issue"; then
  echo "Issue #${ISSUE_NUMBER} is already assigned to Copilot."
  GH_TOKEN=$COPILOT_PAT gh issue edit "$ISSUE_NUMBER" \
    --repo "$REPOSITORY" \
    --add-label fix-in-progress \
    --remove-label autofix
  exit 0
fi

instructions=$(cat <<EOF
Implement the reported issue in ${REPOSITORY}. Add or update focused tests and run the relevant Rust, desktop frontend, and documentation checks. Keep Arca global-config-only. Do not merge the pull request, publish a release, or weaken tests. Open a pull request against ${BASE_BRANCH} and request review from @${OWNER}.
EOF
)

payload=$(jq -n \
  --arg repo "$REPOSITORY" \
  --arg branch "$BASE_BRANCH" \
  --arg instructions "$instructions" \
  '{
    assignees: ["copilot-swe-agent[bot]"],
    agent_assignment: {
      target_repo: $repo,
      base_branch: $branch,
      custom_instructions: $instructions,
      custom_agent: "",
      model: ""
    }
  }')

response=$(mktemp)
trap 'rm -f "$response"' EXIT
status=$(curl --silent --show-error \
  --output "$response" \
  --write-out '%{http_code}' \
  --request POST \
  -H "Accept: application/vnd.github+json" \
  -H "Authorization: Bearer ${COPILOT_PAT}" \
  -H "X-GitHub-Api-Version: 2022-11-28" \
  "${API}/issues/${ISSUE_NUMBER}/assignees" \
  --data "$payload")

if [[ "$status" != "200" && "$status" != "201" ]]; then
  message=$(jq -r '.message // "unknown API error"' "$response" 2>/dev/null || true)
  echo "Copilot assignment failed with HTTP ${status}: ${message}" >&2
  exit 1
fi

GH_TOKEN=$COPILOT_PAT gh issue edit "$ISSUE_NUMBER" \
  --repo "$REPOSITORY" \
  --add-label fix-in-progress \
  --remove-label autofix
GH_TOKEN=$COPILOT_PAT gh issue edit "$ISSUE_NUMBER" \
  --repo "$REPOSITORY" \
  --remove-label autofix-failed || true
GH_TOKEN=$COPILOT_PAT gh issue comment "$ISSUE_NUMBER" \
  --repo "$REPOSITORY" \
  --body "已派发给 Copilot cloud agent。它会创建修复 PR，并在完成后请求 @${OWNER} 检验。"
