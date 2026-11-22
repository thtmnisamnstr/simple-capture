#!/usr/bin/env bash
# Usage: scripts/ci/version-check.sh [VERSION]
# The workspace, the Tauri app and the UI must carry one version (and VERSION, when given).
set -euo pipefail
cd "$(dirname "$0")/../.."
cargo=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version *= *"\(.*\)"/\1/p' Cargo.toml)
tauri=$(python3 -c 'import json; print(json.load(open("app/tauri.conf.json"))["version"])')
ui=$(python3 -c 'import json; print(json.load(open("ui/package.json"))["version"])')
echo "Cargo $cargo, Tauri $tauri, UI $ui${1:+, tag $1}"
for v in "$tauri" "$ui" ${1:+"$1"}; do
  if [ "$v" != "$cargo" ]; then
    echo "version mismatch: expected $cargo everywhere" >&2
    exit 1
  fi
done
