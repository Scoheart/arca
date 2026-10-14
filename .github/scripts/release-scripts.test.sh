#!/usr/bin/env bash
set -euo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/bin" "$TMP/assets"

cat >"$TMP/bin/gh" <<'FAKE_GH'
#!/usr/bin/env bash
set -euo pipefail

asset_json() {
  jq -n \
    --arg n1 "$FAKE_ASSET_1" --arg n2 "$FAKE_ASSET_2" \
    --arg n3 "$FAKE_ASSET_3" --arg n4 "$FAKE_ASSET_4" \
    '[
      {id:1,name:$n1,label:"Desktop installer · Apple Silicon"},
      {id:2,name:$n2,label:"Command-line tool · Apple Silicon"},
      {id:3,name:$n3,label:"Desktop auto-update payload · no manual download"},
      {id:4,name:$n4,label:"Auto-update manifest · no manual download"}
    ]' | jq --arg scenario "$FAKE_SCENARIO" 'if $scenario == "publish-label-failure" then map(if .id == 2 then .label = "" else . end) else . end'
}

release_json() {
  local id=$1 tag=$2 draft=$3 assets=$4
  jq -n --argjson id "$id" --arg tag "$tag" --argjson draft "$draft" --argjson assets "$assets" \
    '{id:$id,tag_name:$tag,draft:$draft,prerelease:false,assets:$assets}'
}

current_release() {
  draft=true
  [[ -f "$FAKE_STATE/published" ]] && draft=false
  case "$FAKE_SCENARIO" in
    publish-no-draft) release_json 99 v1.2.18 false '[]' ;;
    publish-missing)
      if [[ -f "$FAKE_STATE/uploaded" ]]; then
        names=()
        while IFS= read -r name; do
          names+=("$name")
        done <"$FAKE_STATE/uploaded"
        FAKE_ASSET_1=${names[0]} FAKE_ASSET_2=${names[1]} \
          FAKE_ASSET_3=${names[2]} FAKE_ASSET_4=${names[3]} \
          release_json 99 v1.2.18 "$draft" "$(FAKE_ASSET_1=${names[0]} FAKE_ASSET_2=${names[1]} FAKE_ASSET_3=${names[2]} FAKE_ASSET_4=${names[3]} asset_json)"
      else
        release_json 99 v1.2.18 "$draft" '[]'
      fi
      ;;
    *) release_json 99 v1.2.18 "$draft" "$(asset_json)" ;;
  esac
}

all_releases() {
  printf '['
  current_release
  if [[ "$FAKE_SCENARIO" == publish-higher ]]; then
    printf ','
    release_json 100 v1.2.19 false '[]'
  fi
  printf ']\n'
}

if [[ "$1" == api && "$2" == --paginate && "$3" == --slurp && "$4" == repos/*/releases\?per_page=100 ]]; then
  printf '[['
  all_releases | sed '1s/^\[//;$s/\]$//'
  printf ']]\n'
elif [[ "$1" == api && "$2" == repos/*/releases/latest ]]; then
  if [[ "$FAKE_SCENARIO" == publish-higher ]]; then
    echo 'v1.2.19'
  else
    echo 'v1.2.18'
  fi
elif [[ "$1" == api && "$2" == repos/*/releases/99 ]]; then
  current_release
elif [[ "$1" == api && "$2" == repos/*/releases/assets/* ]]; then
  id=${2##*/}
  if [[ "$FAKE_SCENARIO" == publish-different && "$id" == 1 ]]; then
    printf 'different bytes'
  else
    variable="FAKE_ASSET_PATH_${id}"
    cat "${!variable}"
  fi
elif [[ "$1" == api && "$2" == --method && "$3" == PATCH && "$4" == repos/*/releases/assets/* ]]; then
  id=${4##*/}
  echo "label:$id" >>"$FAKE_STATE/log"
  if [[ "$FAKE_SCENARIO" == publish-label-failure && "$id" == 2 ]]; then
    exit 1
  fi
elif [[ "$1" == api && "$2" == --method && "$3" == PATCH && "$4" == repos/*/releases/* ]]; then
  if [[ " $* " == *" draft=false "* ]]; then
    [[ " $* " == *" make_latest=legacy "* ]]
    touch "$FAKE_STATE/published"
    echo publish >>"$FAKE_STATE/log"
  fi
elif [[ "$1" == release && "$2" == upload ]]; then
  : >"$FAKE_STATE/uploaded"
  shift 3
  while [[ $# -gt 0 && "$1" != --repo ]]; do
    basename "${1%%#*}" >>"$FAKE_STATE/uploaded"
    echo "upload:$(basename "${1%%#*}")" >>"$FAKE_STATE/log"
    shift
  done
else
  echo "unexpected fake gh invocation: $*" >&2
  exit 64
fi
FAKE_GH
chmod +x "$TMP/bin/gh"

export PATH="$TMP/bin:$PATH"
export FAKE_STATE="$TMP/state"
export GITHUB_REPOSITORY="Scoheart/arca"
mkdir -p "$FAKE_STATE"

expect_failure() {
  if "$@" >/dev/null 2>&1; then
    echo "expected command to fail: $*" >&2
    exit 1
  fi
}

TAG=v1.2.18
FAKE_ASSET_1="Arca-Desktop-Installer-$TAG-macOS-Apple-Silicon.dmg"
FAKE_ASSET_2="arca_${TAG}_aarch64-apple-darwin.tar.gz"
FAKE_ASSET_3="Arca-Desktop-AutoUpdate-$TAG-macOS-Apple-Silicon.app.tar.gz"
FAKE_ASSET_4=latest.json
export FAKE_ASSET_1 FAKE_ASSET_2 FAKE_ASSET_3 FAKE_ASSET_4

for index in 1 2 3 4; do
  name_variable="FAKE_ASSET_${index}"
  path="$TMP/assets/${!name_variable}"
  printf 'asset %s\n' "$index" >"$path"
  export "FAKE_ASSET_PATH_${index}=$path"
done

assets=(
  "$FAKE_ASSET_PATH_1"
  "$FAKE_ASSET_PATH_2"
  "$FAKE_ASSET_PATH_3"
  "$FAKE_ASSET_PATH_4"
)

: >"$FAKE_STATE/log"
rm -f "$FAKE_STATE/published"
FAKE_SCENARIO=publish-missing \
  bash "$ROOT/.github/scripts/publish-release-assets.sh" "$TAG" "${assets[@]}"
[[ $(grep -c '^upload:' "$FAKE_STATE/log") -eq 4 ]]
! grep -q '^label:' "$FAKE_STATE/log"
[[ $(tail -n 1 "$FAKE_STATE/log") == publish ]]

: >"$FAKE_STATE/log"
rm -f "$FAKE_STATE/uploaded" "$FAKE_STATE/published"
FAKE_SCENARIO=publish-identical \
  bash "$ROOT/.github/scripts/publish-release-assets.sh" "$TAG" "${assets[@]}"
! grep -q '^upload:' "$FAKE_STATE/log"
[[ $(tail -n 1 "$FAKE_STATE/log") == publish ]]

rm -f "$FAKE_STATE/published"
FAKE_SCENARIO=publish-higher \
  bash "$ROOT/.github/scripts/publish-release-assets.sh" "$TAG" "${assets[@]}"

for scenario in publish-different publish-no-draft publish-label-failure; do
  : >"$FAKE_STATE/log"
  rm -f "$FAKE_STATE/published"
  expect_failure env FAKE_SCENARIO="$scenario" \
    bash "$ROOT/.github/scripts/publish-release-assets.sh" "$TAG" "${assets[@]}"
  ! grep -q '^publish$' "$FAKE_STATE/log"
done

echo "Release publisher tests passed."
