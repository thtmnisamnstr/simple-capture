#!/bin/bash
# Cross-target clippy from macOS (M1 plan §6.4) of the CI fast job's crate set, for Windows and,
# when its rustup target is installed, Linux: catches code only the other OSes compile. Nothing is
# linked, so no MSVC or Linux toolchain is needed. Skia/Tauri crates are only proven on CI.
set -euo pipefail
cd "$(dirname "$0")/../.."
FAST=(-p simple-capture-geometry -p simple-capture-core -p simple-capture-platform
    -p simple-capture-capture-macos -p simple-capture-capture-windows -p simple-capture-capture-linux
    -p simple-capture-record -p simple-capture-decode -p simple-capture-render
    -p simple-capture-helpers -p simple-capture-tracks -p simple-capture-capture-common -p sc-devtools)
installed=$(rustup target list --installed)
W=x86_64-pc-windows-msvc
L=x86_64-unknown-linux-gnu
grep -qx "$W" <<<"$installed" || { echo "windows-check: rustup target add $W" >&2; exit 1; }
targets=("$W")
if grep -qx "$L" <<<"$installed"; then
    targets+=("$L")
else
    echo "windows-check: $L not installed, Linux cross-check SKIPPED (rustup target add $L)"
fi
# The Linux backend links PipeWire through pkg-config, which a macOS host cannot cross-resolve;
# scripts/ci/linux-docker.sh and tools/linux-dev cover it natively.
LINUX_NATIVE=(simple-capture-capture-linux simple-capture-capture-common simple-capture-cli)
for t in "${targets[@]}"; do
    echo "windows-check: $t"
    pkgs=("${FAST[@]}")
    if [ "$t" = "$L" ]; then
        pkgs=()
        for ((i = 0; i < ${#FAST[@]}; i += 2)); do
            [[ " ${LINUX_NATIVE[*]} " == *" ${FAST[i+1]} "* ]] || pkgs+=("${FAST[i]}" "${FAST[i+1]}")
        done
    fi
    cargo clippy --locked --target "$t" "${pkgs[@]}" --all-targets -- -D warnings
    [ "$t" = "$L" ] || cargo clippy --locked --target "$t" -p simple-capture-cli --no-default-features --all-targets -- -D warnings
done
echo "windows-check: ok (${targets[*]})"
