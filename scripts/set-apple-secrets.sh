#!/usr/bin/env bash
# Sets the five GitHub secrets the package workflow uses to sign and notarize macOS releases.
# Run it yourself; it reads the secret values from your files and hidden prompts, sends them
# straight to `gh secret set`, and never prints or keeps them.
#   scripts/set-apple-secrets.sh DEVID.p12 AuthKey_XXXXXXXXXX.p8
set -euo pipefail
cd "$(dirname "$0")/.."
P12="${1:?usage: scripts/set-apple-secrets.sh DEVID.p12 AuthKey_KEYID.p8}"
P8="${2:?usage: scripts/set-apple-secrets.sh DEVID.p12 AuthKey_KEYID.p8}"
[ -f "$P12" ] || { echo "no file $P12" >&2; exit 1; }
[ -f "$P8" ] || { echo "no file $P8" >&2; exit 1; }
gh auth status >/dev/null

read -r -s -p "Password you gave the .p12 export: " P12_PASSWORD
echo
export P12_PASSWORD
openssl pkcs12 -in "$P12" -passin env:P12_PASSWORD -nokeys -legacy 2>/dev/null |
  grep -q "Developer ID Application" ||
  openssl pkcs12 -in "$P12" -passin env:P12_PASSWORD -nokeys 2>/dev/null |
  grep -q "Developer ID Application" ||
  { echo "That .p12 does not open with this password, or holds no Developer ID Application certificate." >&2; exit 1; }

KEY_ID_GUESS=$(basename "$P8" .p8 | sed -n 's/^AuthKey_//p')
read -r -p "App Store Connect Key ID [${KEY_ID_GUESS}]: " KEY_ID
KEY_ID="${KEY_ID:-$KEY_ID_GUESS}"
read -r -p "App Store Connect Issuer ID: " ISSUER
[ -n "$KEY_ID" ] && [ -n "$ISSUER" ] || { echo "Key ID and Issuer ID are both needed." >&2; exit 1; }

base64 -i "$P12" | gh secret set APPLE_CERTIFICATE_P12
printf '%s' "$P12_PASSWORD" | gh secret set APPLE_CERTIFICATE_PASSWORD
gh secret set APPLE_API_KEY_P8 < "$P8"
printf '%s' "$KEY_ID" | gh secret set APPLE_API_KEY_ID
printf '%s' "$ISSUER" | gh secret set APPLE_API_ISSUER
unset P12_PASSWORD
gh secret list | grep '^APPLE_'
echo "Done. Delete $P12 now; keep $P8 somewhere safe (Apple lets you download it only once)."
