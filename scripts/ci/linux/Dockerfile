# Linux dry-run image for scripts/ci/linux-docker.sh: spike 6's ubuntu:24.04 base plus the CI
# job's build packages (Tauri: webkit2gtk-4.1, gtk3, ayatana-appindicator, rsvg, xdo; Skia:
# clang, ninja, python3, fontconfig; wgpu tests: mesa-vulkan-drivers for a software adapter).
FROM ubuntu:24.04@sha256:73b999393d07f2bf65a40373778a44dc9a6d54ad7718d29247d8ff9413c4d6a8
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl git build-essential pkg-config file \
    libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev \
    clang libclang-dev ninja-build python3 libfontconfig1-dev \
    libasound2-dev libpipewire-0.3-dev libspa-0.2-dev ffmpeg \
    mesa-vulkan-drivers libvulkan1 \
    && rm -rf /var/lib/apt/lists/*
ENV RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo PATH=/opt/cargo/bin:$PATH
RUN curl -sSf --proto '=https' --tlsv1.2 https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain 1.95.0 \
    && rustup component add clippy rustfmt \
    && rustup target add x86_64-pc-windows-msvc x86_64-unknown-linux-gnu
WORKDIR /work
