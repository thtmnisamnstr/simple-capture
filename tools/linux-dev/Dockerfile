# Linux build and test box for the Linux backend (Ubuntu 24.04, like CI). Headless sway,
# PipeWire, WirePlumber and xdg-desktop-portal-wlr let the portal ScreenCast path run without a
# display. Build: tools/linux-dev/run.sh build. Never used for releases.
FROM ubuntu:24.04
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update -q && apt-get install -y --no-install-recommends \
      ca-certificates curl git build-essential pkg-config clang libclang-dev \
      libpipewire-0.3-dev libspa-0.2-dev libasound2-dev \
      pipewire pipewire-pulse wireplumber dbus dbus-user-session \
      sway xdg-desktop-portal xdg-desktop-portal-wlr \
      ffmpeg python3 \
      gstreamer1.0-tools gstreamer1.0-pipewire gstreamer1.0-plugins-base \
      libgstreamer1.0-dev libgstreamer-plugins-base1.0-dev gstreamer1.0-plugins-good \
      gstreamer1.0-plugins-bad gstreamer1.0-plugins-ugly \
      nasm cmake ninja-build meson autoconf automake libtool \
      mingw-w64 \
    && rm -rf /var/lib/apt/lists/*
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain 1.95.0 \
    && /root/.cargo/bin/rustup component add clippy rustfmt
ENV PATH=/root/.cargo/bin:$PATH
WORKDIR /src
