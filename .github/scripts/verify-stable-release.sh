#!/usr/bin/env bash

set -euo pipefail
unset FORCE_COLOR GH_FORCE_TTY CLICOLOR_FORCE
export NO_COLOR=1

usage() {
  cat <<'EOF'
Usage: verify-release.sh [options] vX.Y.Z

Download and verify a Arca macOS stable Release.

Options:
  --repo OWNER/REPO    GitHub repository (default: Scoheart/arca)
  --source-root PATH   Arca source checkout used for updater-key verification
  --wait               Wait for the Draft to become a complete published Release
  --timeout SECONDS    Maximum --wait duration (default: 900)
  --keep               Preserve the temporary verification evidence folder

This verifier is read-only apart from downloading temporary evidence.
EOF
}

REPOSITORY="Scoheart/arca"
SOURCE_ROOT=""
KEEP=0
WAIT_FOR_RELEASE=0
WAIT_TIMEOUT=900
TAG=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --repo)
      [[ $# -ge 2 ]] || { usage >&2; exit 2; }
      REPOSITORY=$2
      shift 2
      ;;
    --source-root)
      [[ $# -ge 2 ]] || { usage >&2; exit 2; }
      SOURCE_ROOT=$2
      shift 2
      ;;
    --keep)
      KEEP=1
      shift
      ;;
    --wait)
      WAIT_FOR_RELEASE=1
      shift
      ;;
    --timeout)
      [[ $# -ge 2 ]] || { usage >&2; exit 2; }
      WAIT_TIMEOUT=$2
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    v[0-9]*.[0-9]*.[0-9]*)
      [[ -z "$TAG" ]] || { printf '%s\n' 'Only one tag may be supplied.' >&2; exit 2; }
      TAG=$1
      shift
      ;;
    *)
      printf 'Unknown argument: %s\n' "$1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

[[ "$TAG" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || {
  printf '%s\n' 'A stable tag in vX.Y.Z form is required.' >&2
  exit 2
}

[[ "$WAIT_TIMEOUT" =~ ^[1-9][0-9]*$ ]] || {
  printf '%s\n' '--timeout must be a positive integer.' >&2
  exit 2
}

[[ "$(uname -s)" == "Darwin" ]] || {
  printf '%s\n' 'Full Arca desktop verification requires macOS.' >&2
  exit 1
}

for command in gh jq shasum tar hdiutil codesign lipo node; do
  command -v "$command" >/dev/null 2>&1 || {
    printf 'Missing required command: %s\n' "$command" >&2
    exit 1
  }
done

gh auth status >/dev/null

if [[ -z "$SOURCE_ROOT" ]]; then
  SOURCE_ROOT=$(git rev-parse --show-toplevel 2>/dev/null || true)
fi

[[ -n "$SOURCE_ROOT" && -f "$SOURCE_ROOT/desktop/src-tauri/tauri.conf.json" ]] || {
  printf '%s\n' 'Provide --source-root pointing to the Arca repository.' >&2
  exit 1
}

VERSION=${TAG#v}
ARCH_LABEL="Apple-Silicon"
UPDATER="Arca-Desktop-AutoUpdate-${TAG}-macOS-${ARCH_LABEL}.app.tar.gz"
INSTALLER="Arca-Desktop-Installer-${TAG}-macOS-${ARCH_LABEL}.dmg"
CLI="arca_${TAG}_aarch64-apple-darwin.tar.gz"
LATEST="latest.json"
LEGACY_CLI="mux_${TAG}_aarch64-apple-darwin.tar.gz"
EXPECTED_ASSETS=("$LATEST" "$UPDATER" "$INSTALLER" "$CLI" "$LEGACY_CLI")

SCRIPT_STARTED=$SECONDS
WORK=$(mktemp -d "${TMPDIR:-/tmp}/arca-${TAG}-verify.XXXXXX")
MOUNT=""
cleanup() {
  if [[ -n "$MOUNT" && -d "$MOUNT" ]]; then
    hdiutil detach "$MOUNT" >/dev/null 2>&1 || true
    rmdir "$MOUNT" >/dev/null 2>&1 || true
  fi
  if [[ "$KEEP" -eq 0 ]]; then
    rm -rf "$WORK"
  else
    printf 'evidence: %s\n' "$WORK"
  fi
}
trap cleanup EXIT

RELEASE_JSON="$WORK/release.json"
WAIT_STARTED=$SECONDS
if [[ "$WAIT_FOR_RELEASE" -eq 1 ]]; then
  WAIT_DEADLINE=$((SECONDS + WAIT_TIMEOUT))
  while true; do
    CANDIDATE="$WORK/release.candidate.json"
    if gh release view "$TAG" --repo "$REPOSITORY" \
      --json tagName,name,isDraft,isPrerelease,isImmutable,publishedAt,targetCommitish,url,assets \
      >"$CANDIDATE" 2>/dev/null &&
      jq -e '
        .isDraft == false and
        .isPrerelease == false and
        (.publishedAt | type == "string" and length > 0) and
        (.assets | length == 5)
      ' "$CANDIDATE" >/dev/null; then
      mv "$CANDIDATE" "$RELEASE_JSON"
      break
    fi
    if [[ "$SECONDS" -ge "$WAIT_DEADLINE" ]]; then
      printf 'Timed out after %s seconds waiting for %s to publish.\n' \
        "$WAIT_TIMEOUT" "$TAG" >&2
      exit 1
    fi
    sleep 5
  done
else
  gh release view "$TAG" --repo "$REPOSITORY" \
    --json tagName,name,isDraft,isPrerelease,isImmutable,publishedAt,targetCommitish,url,assets \
    >"$RELEASE_JSON"
fi
WAIT_SECONDS=$((SECONDS - WAIT_STARTED))

jq -e --arg tag "$TAG" '
  .tagName == $tag and
  .isDraft == false and
  .isPrerelease == false and
  (.publishedAt | type == "string" and length > 0) and
  (.targetCommitish | test("^[0-9a-f]{40}$"))
' "$RELEASE_JSON" >/dev/null

TARGET=$(jq -r .targetCommitish "$RELEASE_JSON")
TAG_TYPE=$(gh api "repos/${REPOSITORY}/git/ref/tags/${TAG}" >"$WORK/tag.json"; jq -r .object.type "$WORK/tag.json")
TAG_SHA=$(jq -r .object.sha "$WORK/tag.json")
[[ "$TAG_TYPE" == "commit" && "$TAG_SHA" == "$TARGET" ]] || {
  printf 'Tag/Release target mismatch: type=%s tag=%s release=%s\n' \
    "$TAG_TYPE" "$TAG_SHA" "$TARGET" >&2
  exit 1
}

[[ $(jq '.assets | length' "$RELEASE_JSON") -eq ${#EXPECTED_ASSETS[@]} ]] || {
  printf '%s\n' 'Stable Release does not contain the expected five assets:' >&2
  jq -r '.assets[].name' "$RELEASE_JSON" >&2
  exit 1
}

verify_digest() {
  local name=$1 expected actual count
  count=$(jq --arg name "$name" '[.assets[] | select(.name == $name)] | length' "$RELEASE_JSON")
  [[ "$count" -eq 1 ]] || {
    printf 'Expected exactly one asset named %s, got %s.\n' "$name" "$count" >&2
    exit 1
  }
  expected=$(jq -r --arg name "$name" '.assets[] | select(.name == $name) | .digest' "$RELEASE_JSON")
  [[ "$expected" =~ ^sha256:[0-9a-f]{64}$ ]] || {
    printf 'Missing GitHub SHA-256 digest for %s.\n' "$name" >&2
    exit 1
  }
  [[ -f "$WORK/assets/$name" ]] || {
    printf 'Downloaded asset is missing: %s\n' "$name" >&2
    exit 1
  }
  actual="sha256:$(shasum -a 256 "$WORK/assets/$name" | awk '{print $1}')"
  [[ "$actual" == "$expected" ]] || {
    printf 'Digest mismatch for %s: expected %s, got %s.\n' "$name" "$expected" "$actual" >&2
    exit 1
  }
  printf '%s  %s\n' "${actual#sha256:}" "$name" >>"$WORK/SHA256SUMS"
}

mkdir -p "$WORK/assets"
DOWNLOAD_STARTED=$SECONDS
download_pids=()
for asset in "${EXPECTED_ASSETS[@]}"; do
  asset_id=$(jq -er --arg name "$asset" '.assets[] | select(.name == $name) | .apiUrl | split("/") | last | tonumber' "$RELEASE_JSON")
  gh api "repos/$REPOSITORY/releases/assets/$asset_id" -H 'Accept: application/octet-stream' >"$WORK/assets/$asset" &
  download_pids+=("$!")
done
download_failed=0
for pid in "${download_pids[@]}"; do
  wait "$pid" || download_failed=1
done
[[ "$download_failed" -eq 0 ]] || { echo 'Release download failed.' >&2; exit 1; }
DOWNLOAD_SECONDS=$((SECONDS - DOWNLOAD_STARTED))

for asset in "${EXPECTED_ASSETS[@]}"; do
  verify_digest "$asset"
done

VERIFY_STARTED=$SECONDS

EXPECTED_URL="https://github.com/${REPOSITORY}/releases/download/${TAG}/${UPDATER}"
jq -e --arg version "$VERSION" --arg url "$EXPECTED_URL" '
  .version == $version and
  (.pub_date | type == "string" and length > 0) and
  (.platforms | keys == ["darwin-aarch64"]) and
  .platforms["darwin-aarch64"].url == $url and
  (.platforms["darwin-aarch64"].signature | type == "string" and length > 100)
' "$WORK/assets/$LATEST" >/dev/null

# Read the signing key and product identity from the immutable released source.
gh api "repos/$REPOSITORY/contents/desktop/src-tauri/tauri.conf.json?ref=$TARGET" --jq .content \
  | base64 --decode >"$WORK/tauri.conf.json"
jq -e --arg version "$VERSION" '.version == $version and .identifier == "com.scoheart.arca"' "$WORK/tauri.conf.json" >/dev/null
node "$SOURCE_ROOT/.github/scripts/verify-updater-signature.mjs" \
  "$WORK/assets/$LATEST" "$WORK/tauri.conf.json" "$WORK/assets/$UPDATER"

verify_arca_app() {
  local app=$1 short build identifier executable desktop_executable arch
  [[ -d "$app" ]] || { printf 'Missing app: %s\n' "$app" >&2; exit 1; }
  short=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$app/Contents/Info.plist")
  build=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleVersion' "$app/Contents/Info.plist")
  identifier=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$app/Contents/Info.plist")
  executable="$app/Contents/MacOS/arca"
  [[ "$short" == "$VERSION" && "$build" == "$VERSION" ]] || {
    printf 'App version mismatch: short=%s build=%s expected=%s\n' "$short" "$build" "$VERSION" >&2
    exit 1
  }
  [[ "$identifier" == "com.scoheart.arca" ]] || {
    printf 'Unexpected bundle identifier: %s\n' "$identifier" >&2
    exit 1
  }
  [[ -x "$executable" ]] || { printf 'Missing bundled arca CLI: %s\n' "$executable" >&2; exit 1; }
  arch=$(lipo -archs "$executable")
  [[ " $arch " == *" arm64 "* ]] || { printf 'Bundled CLI lacks arm64: %s\n' "$arch" >&2; exit 1; }
  [[ $("$executable" --version) == "arca $VERSION" ]] || {
    printf '%s\n' 'Bundled arca CLI version mismatch.' >&2
    exit 1
  }
  desktop_executable=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$app/Contents/Info.plist")
  [[ "$desktop_executable" == "desktop" ]]
  arch=$(lipo -archs "$app/Contents/MacOS/$desktop_executable")
  [[ " $arch " == *" arm64 "* ]] || { echo 'Desktop executable lacks arm64.' >&2; exit 1; }
  codesign --verify --deep --strict --verbose=2 "$app"
}

mkdir -p "$WORK/updater"
tar -xzf "$WORK/assets/$UPDATER" -C "$WORK/updater"
UPDATER_APP=$(find "$WORK/updater" -maxdepth 2 -type d -name '*.app' -print -quit)
[[ -n "$UPDATER_APP" ]] || { printf '%s\n' 'Updater archive contains no app.' >&2; exit 1; }
verify_arca_app "$UPDATER_APP"

hdiutil verify "$WORK/assets/$INSTALLER"
MOUNT=$(mktemp -d "${TMPDIR:-/tmp}/arca-${TAG}-mount.XXXXXX")
hdiutil attach -readonly -nobrowse -noautoopen -mountpoint "$MOUNT" "$WORK/assets/$INSTALLER" >/dev/null
DMG_APP=$(find "$MOUNT" -maxdepth 1 -type d -name '*.app' -print -quit)
[[ -n "$DMG_APP" ]] || { printf '%s\n' 'DMG contains no app.' >&2; exit 1; }
verify_arca_app "$DMG_APP"
hdiutil detach "$MOUNT" >/dev/null
rmdir "$MOUNT"
MOUNT=""

mkdir -p "$WORK/cli"
tar -xzf "$WORK/assets/$CLI" -C "$WORK/cli"
CLI_BIN=$(find "$WORK/cli" -maxdepth 2 -type f -name arca -print -quit)
[[ -n "$CLI_BIN" && -x "$CLI_BIN" ]] || { printf '%s\n' 'CLI archive contains no executable arca.' >&2; exit 1; }
CLI_ARCHS=$(lipo -archs "$CLI_BIN")
[[ " $CLI_ARCHS " == *" arm64 "* ]] || { printf 'CLI lacks arm64: %s\n' "$CLI_ARCHS" >&2; exit 1; }
[[ $("$CLI_BIN" --version) == "arca $VERSION" ]] || { printf '%s\n' 'Standalone CLI version mismatch.' >&2; exit 1; }

# The transport bridge must contain the same executable bytes, not a stale CLI.
mkdir -p "$WORK/legacy-cli"
tar -xzf "$WORK/assets/$LEGACY_CLI" -C "$WORK/legacy-cli"
test -x "$WORK/legacy-cli/mux"
cmp "$CLI_BIN" "$WORK/legacy-cli/mux"
latest_tag=$(gh api "repos/$REPOSITORY/releases/latest" --jq .tag_name)
[[ "$latest_tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]
expected_latest=$(gh api --paginate --slurp "repos/$REPOSITORY/releases?per_page=100" | jq -er '[.[][] | select(.draft == false and .prerelease == false and (.tag_name | test("^v[0-9]+\\.[0-9]+\\.[0-9]+$")))] | sort_by(.tag_name | ltrimstr("v") | split(".") | map(tonumber)) | last.tag_name')
[[ "$latest_tag" == "$expected_latest" ]] || { echo 'Latest channel does not point at greatest stable version.' >&2; exit 1; }

VERIFY_SECONDS=$((SECONDS - VERIFY_STARTED))
TOTAL_SECONDS=$((SECONDS - SCRIPT_STARTED))
printf '\nArca stable release verification: PASS\n'
printf 'release: %s\ntag: %s\ntarget: %s\nversion: %s\n' \
  "$(jq -r .url "$RELEASE_JSON")" "$TAG" "$TARGET" "$VERSION"
cat "$WORK/SHA256SUMS"
printf 'timing.wait_seconds: %s\n' "$WAIT_SECONDS"
printf 'timing.download_seconds: %s\n' "$DOWNLOAD_SECONDS"
printf 'timing.verify_seconds: %s\n' "$VERIFY_SECONDS"
printf 'timing.total_seconds: %s\n' "$TOTAL_SECONDS"
