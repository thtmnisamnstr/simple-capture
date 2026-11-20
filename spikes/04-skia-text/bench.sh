#!/bin/sh
set -eu
cd "$(dirname "$0")"
./fonts/fetch.sh
cargo build --release
./target/ft/release/skia-text-spike --reps 30 --label quiet
taskpolicy -c background ./target/ft/release/skia-text-spike --reps 15 --label ecore-proxy --no-png --threads 1,4
