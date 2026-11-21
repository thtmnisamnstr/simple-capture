#!/usr/bin/env bash
# tools/linux-dev/run.sh build            build the image
# tools/linux-dev/run.sh <command...>     run a command in /src (the repo, read-write) with a
#                                         separate target dir and cargo cache in volumes
set -euo pipefail
cd "$(dirname "$0")/../.."
IMAGE=sc-linux-dev
if [ "${1:-}" = build ]; then
  exec docker build -t "$IMAGE" tools/linux-dev
fi
exec docker run --rm -i \
  -v "$PWD":/src \
  -v sc-linux-target:/target \
  -v sc-linux-cargo:/root/.cargo/registry \
  -e CARGO_TARGET_DIR=/target \
  --cpus 4 \
  "$IMAGE" bash -lc "$*"
