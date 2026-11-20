#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/../.."
scripts/ci/lint.sh
cargo test --locked --workspace
pnpm --dir ui check
pnpm --dir ui test
