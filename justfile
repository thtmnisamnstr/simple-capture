set shell := ["bash", "-uc"]

target_dir := env_var_or_default("CARGO_TARGET_DIR", "target")

default:
    @just --list

setup:
    rustup show active-toolchain
    pnpm --dir ui install --frozen-lockfile
    @for t in cmake nasm ninja pkgconf meson python3; do command -v $t >/dev/null || echo "setup: missing $t (brew install $t)"; done

helpers:
    third_party/fetch.sh
    third_party/ffmpeg/build-macos.sh arm64
    mkdir -p app/binaries
    cp third_party/out/macos-arm64/ffmpeg app/binaries/ffmpeg-aarch64-apple-darwin
    cp third_party/out/macos-arm64/ffprobe app/binaries/ffprobe-aarch64-apple-darwin

build:
    pnpm --dir ui build
    cargo build --locked --workspace

run:
    pnpm --dir ui build
    cargo build --locked -p simple-capture-app
    scripts/lib/live.sh tools/macos-dev-host/run.sh {{target_dir}}/debug/simple-capture-app

app-driver SCRIPT:
    pnpm --dir ui build
    cargo build --locked -p simple-capture-app
    SC_APP_SCRIPT="$(cd "$(dirname '{{SCRIPT}}')" && pwd)/$(basename '{{SCRIPT}}')" scripts/lib/live.sh tools/macos-dev-host/run.sh {{target_dir}}/debug/simple-capture-app

bundle:
    pnpm --dir ui build
    cd app && ../ui/node_modules/.bin/tauri build --config tauri.bundle.conf.json -- --locked

test:
    cargo test --locked --workspace
    pnpm --dir ui test

test-live PKG PREFIX *FILTERS:
    scripts/test-live.sh {{PKG}} {{PREFIX}} {{FILTERS}}

e2e:
    scripts/e2e/run.sh

sync-test:
    scripts/sync/run.sh

soak N:
    scripts/soak/run.sh {{N}}

killtest:
    scripts/killtest/run.sh

lint:
    scripts/ci/lint.sh

deny:
    scripts/ci/deny.sh

licenses:
    scripts/ci/licenses.sh

netcheck:
    scripts/ci/netcheck.sh

ci-local:
    scripts/ci/ci-local.sh

ci-local-fast:
    scripts/ci/ci-local-fast.sh
