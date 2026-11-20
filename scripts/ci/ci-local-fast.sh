#!/bin/bash
# Merge check (M1 plan §0.2 rule 4): fmt check, clippy and tests for the workspace, Windows and
# Linux cross-target clippy (windows-check.sh), UI check and tests. The Tauri app needs ui/dist
# to compile, so the UI is built first when it is missing.
set -euo pipefail
cd "$(dirname "$0")/../.."
[ -f ui/dist/index.html ] || pnpm --dir ui build
scripts/ci/lint.sh
scripts/ci/windows-check.sh
cargo test --locked --workspace
cargo test --locked -p simple-capture-record --features test-hooks --test kill_synth
pnpm --dir ui check
pnpm --dir ui test
