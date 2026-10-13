#!/bin/bash
# Stage the arca CLI as a Tauri sidecar (bundle.externalBin), so installing the
# desktop app also ships the command-line tool inside Arca.app/Contents/MacOS/.
# Tauri resolves `binaries/arca` to `binaries/arca-<target-triple>` at build time.
set -euo pipefail
cd "$(dirname "$0")/.." # desktop/

TRIPLE=$(rustc -vV | sed -n 's/^host: //p')
cargo build --release --locked -p arca-cli --manifest-path ../Cargo.toml
mkdir -p src-tauri/binaries
cp ../target/release/arca "src-tauri/binaries/arca-${TRIPLE}"
echo "sidecar staged: src-tauri/binaries/arca-${TRIPLE}"
