#!/usr/bin/env bash
# Installs simple-capture from a repository site that build.sh made (signed), in clean Ubuntu,
# Debian and Fedora containers, so a dependency name that a distro lacks or a bad signature
# fails here instead of on a user's machine. Needs docker; runs this machine's architecture.
#   scripts/linux-repos/verify.sh SITE_DIR
set -euo pipefail
SITE=$(cd "$1" && pwd)
apt_check() {
  docker run --rm -v "$SITE:/site:ro" "$1" bash -euxc '
    sed "s#^URIs: .*#URIs: file:///site/apt#; s#^Signed-By: .*#Signed-By: /site/simple-capture.gpg#" \
      /site/simple-capture.sources > /etc/apt/sources.list.d/simple-capture.sources
    apt-get update -q
    DEBIAN_FRONTEND=noninteractive apt-get install -y -q --no-install-recommends simple-capture
    test -x /usr/bin/simple-capture-app && test -x /usr/bin/sc-ffmpeg
    /usr/bin/sc-ffmpeg -hide_banner -version | head -n 1
    ldd /usr/bin/simple-capture-app | grep "not found" && exit 1 || true'
}
dnf_check() {
  docker run --rm -v "$SITE:/site:ro" "$1" bash -euxc '
    sed "s#^baseurl=.*#baseurl=file:///site/rpm/\$basearch#; s#^gpgkey=.*#gpgkey=file:///site/simple-capture.asc#" \
      /site/simple-capture.repo > /etc/yum.repos.d/simple-capture.repo
    dnf install -y -q simple-capture
    rpm -K "$(ls /site/rpm/$(uname -m)/*.rpm)"
    test -x /usr/bin/simple-capture-app && test -x /usr/bin/sc-ffmpeg
    /usr/bin/sc-ffmpeg -hide_banner -version | head -n 1
    ldd /usr/bin/simple-capture-app | grep "not found" && exit 1 || true'
}
apt_check ubuntu:24.04
apt_check debian:trixie
dnf_check fedora:latest
echo "verify.sh: installed and started the helpers on Ubuntu 24.04, Debian 13 and the latest Fedora"
