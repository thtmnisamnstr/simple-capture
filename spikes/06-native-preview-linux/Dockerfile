FROM ubuntu:24.04
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl build-essential pkg-config git \
    libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev \
    libwayland-dev libxkbcommon-dev libx11-dev libx11-xcb-dev \
    mesa-vulkan-drivers libvulkan1 vulkan-tools libegl1 libgl1-mesa-dri libegl-mesa0 mesa-utils \
    xvfb x11-apps x11-utils imagemagick xdotool \
    weston dbus-x11 at-spi2-core fonts-dejavu-core \
    && rm -rf /var/lib/apt/lists/*
ENV RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo PATH=/opt/cargo/bin:$PATH
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain 1.95.0
WORKDIR /work
