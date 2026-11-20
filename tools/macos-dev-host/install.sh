#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")"
APP="$HOME/Applications/Simple Capture Dev.app"
# Rebuilding changes the ad-hoc signature, which makes macOS re-ask for every permission.
if [ -d "$APP" ] && [ "${1:-}" != "--force" ]; then
  echo "already installed: $APP (not rebuilding; grants stay valid)"
  exit 0
fi
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS"
clang -O2 -Wall -mmacosx-version-min=14.0 -o "$APP/Contents/MacOS/sc-dev-host" host.c
cp Info.plist "$APP/Contents/Info.plist"
codesign --force -s - --identifier com.simplecapture.dev "$APP"
codesign -dr - "$APP" 2>&1 | tail -1
echo "installed: $APP"
