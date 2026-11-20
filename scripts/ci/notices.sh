#!/bin/bash
# Third-party notices for the Rust crates in the app (docs/LICENSES.md §2.2), via cargo-about
# 0.9.3 (`cargo install --locked cargo-about@0.9.3 --features cli`). Release artifact, never
# checked in. Usage: scripts/ci/notices.sh [OUT] (default target/notices/THIRD_PARTY_RUST.html).
set -euo pipefail
cd "$(dirname "$0")/../.."
OUT=${1:-target/notices/THIRD_PARTY_RUST.html}
command -v cargo-about >/dev/null || { echo "notices.sh: cargo-about missing (cargo install --locked cargo-about@0.9.3 --features cli)" >&2; exit 1; }
mkdir -p "$(dirname "$OUT")"
cargo about generate --locked --fail --workspace -o "$OUT" scripts/ci/about.hbs
echo "notices.sh: wrote $OUT"
