#!/bin/bash
# cargo-deny (licenses, bans, advisories, sources) plus graph checks cargo-deny cannot express.
# docs/LICENSES.md §2.1, M1 plan wp-ci. Needs cargo-deny 0.22.1 (`cargo install --locked
# cargo-deny@0.22.1`). Advisories need network for the RustSec DB unless SC_DENY_OFFLINE=1.
set -euo pipefail
cd "$(dirname "$0")/../.."

fail=0
DESKTOP_TARGETS=(aarch64-apple-darwin x86_64-apple-darwin x86_64-pc-windows-msvc x86_64-unknown-linux-gnu)
HTTP_CRATES=(reqwest hyper ureq isahc curl curl-sys attohttpc surf)

command -v cargo-deny >/dev/null || { echo "deny.sh: cargo-deny missing (cargo install --locked cargo-deny@0.22.1)" >&2; exit 1; }

offline=()
[ "${SC_DENY_OFFLINE:-0}" = 1 ] && offline=(--offline)
cargo deny --locked ${offline[@]+"${offline[@]}"} check || fail=1

# ffmpeg-next / ffmpeg-sys-next are planned for Linux only (LICENSES §3.3) and absent in M1.
if grep -E '^name = "ffmpeg-(sys-)?next"$' Cargo.lock; then
    echo "deny.sh: ffmpeg-next found in Cargo.lock (not allowed in M1)" >&2
    fail=1
fi

# The HTTP-client bans hold per desktop target: Tauri names reqwest/hyper only for Android/iOS.
# Only crates present in Cargo.lock are queried, so a cargo-tree error is a real failure.
[ ${#offline[@]} = 0 ] && { cargo fetch --locked -q || fail=1; }
for c in "${HTTP_CRATES[@]}"; do
    grep -q "^name = \"$c\"$" Cargo.lock || continue
    for t in "${DESKTOP_TARGETS[@]}"; do
        if ! out=$(cargo tree --locked --offline --target "$t" -e normal,build -i "$c" 2>/dev/null); then
            echo "deny.sh: cargo tree --target $t -i $c failed" >&2
            fail=1
        elif [ -n "$out" ]; then
            echo "deny.sh: $c is built for $t:" >&2
            printf '%s\n' "$out" >&2
            fail=1
        else
            echo "deny.sh: $c in Cargo.lock but not built for $t"
        fi
    done
done

[ $fail = 0 ] && echo "deny.sh: ok" || echo "deny.sh: FAILED" >&2
exit $fail
