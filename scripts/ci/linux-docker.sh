#!/bin/bash
# Linux dry run in Docker (M1 plan §6.4): builds and tests the whole workspace on the host's
# native arch (aarch64 on the M3: a dry run, not the x86_64 CI target) in an ubuntu:24.04 image
# with the CI job's packages. The UI is built on the host first (ui/dist is arch-independent).
#   scripts/ci/linux-docker.sh            image (cached) + clippy + tests
#   SC_DOCKER_REBUILD=1 ...               rebuild the image
# Cargo registry, target dir and rustup live in named volumes, so reruns are incremental.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
IMG=sc-ci-linux:1
command -v docker >/dev/null && docker info >/dev/null 2>&1 || { echo "linux-docker: Docker is not running" >&2; exit 1; }

if [ "${SC_DOCKER_REBUILD:-0}" = 1 ] || ! docker image inspect "$IMG" >/dev/null 2>&1; then
    docker build -t "$IMG" -f scripts/ci/linux/Dockerfile scripts/ci/linux
fi

[ -f ui/dist/index.html ] || pnpm --dir ui build

docker run --rm -i \
    -v "$ROOT":/work \
    -v sc-ci-cargo-registry:/opt/cargo/registry \
    -v sc-ci-target:/target \
    -e CARGO_TARGET_DIR=/target \
    -e SC_REQUIRE_GPU=1 \
    -w /work "$IMG" bash -euo pipefail -s <<'IN'
uname -m
rustc --version
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
IN
echo "linux-docker: ok"
