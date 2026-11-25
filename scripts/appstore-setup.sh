#!/usr/bin/env bash
# Sets up the Mac App Store edition (docs/RELEASING.md). With an Admin App Store Connect API key it
# registers the bundle ID, creates an Apple Distribution and a Mac Installer Distribution
# certificate and a Mac App Store profile, and stores them, the CI's App Manager key and the app's
# Apple ID as the app-store workflow's GitHub secrets. Run it yourself: secret values go from your
# files and App Store Connect straight to `gh secret set`, and are never printed or kept.
#   scripts/appstore-setup.sh [--dry-run] [--renew-certs] AuthKey_ADMIN.p8 [AuthKey_APPMANAGER.p8]
# With one key, CI gets that key too. Running it again keeps the stored certificates.
set -euo pipefail
usage="usage: scripts/appstore-setup.sh [--dry-run] [--renew-certs] ADMIN.p8 [CI.p8]"
flags=()
args=()
dry=false
renew=false
for a in "$@"; do
  case "$a" in
    --dry-run) dry=true; flags+=("$a") ;;
    --renew-certs) renew=true; flags+=("$a") ;;
    -h | --help) echo "$usage"; exit 0 ;;
    -*) echo "$usage" >&2; exit 2 ;;
    *) args+=("$a") ;;
  esac
done
if [ "${#args[@]}" -lt 1 ] || [ "${#args[@]}" -gt 2 ]; then
  echo "$usage" >&2
  exit 2
fi
keys=()
for f in "${args[@]}"; do
  [ -f "$f" ] || { echo "no file $f" >&2; exit 1; }
  keys+=("$(cd "$(dirname "$f")" && pwd -P)/$(basename "$f")")
done
cd "$(dirname "$0")/.."
for tool in gh openssl python3; do
  command -v "$tool" >/dev/null || { echo "$tool is needed" >&2; exit 1; }
done
gh auth status >/dev/null
repo=$(gh repo view --json nameWithOwner -q .nameWithOwner)
[ -n "$repo" ] || { echo "run this inside the simple-capture checkout" >&2; exit 1; }

guess() { basename "$1" .p8 | sed -n 's/^AuthKey_//p'; }
admin_guess=$(guess "${keys[0]}")
read -r -p "Key ID of ${args[0]} (the Admin key) [${admin_guess}]: " admin_id
admin_id="${admin_id:-$admin_guess}"
read -r -p "Issuer ID (shown above the keys list in App Store Connect): " issuer
if [ -z "$admin_id" ] || [ -z "$issuer" ]; then
  echo "Key ID and Issuer ID are both needed." >&2
  exit 1
fi
ci_args=(--ci-same-key)
if [ "${#keys[@]}" = 2 ]; then
  ci_guess=$(guess "${keys[1]}")
  read -r -p "Key ID of ${args[1]} (the App Manager key for CI) [${ci_guess}]: " ci_id
  ci_id="${ci_id:-$ci_guess}"
  [ -n "$ci_id" ] || { echo "The CI key's Key ID is needed." >&2; exit 1; }
  ci_args=(--ci-key-file "${keys[1]}" --ci-key-id "$ci_id")
fi

echo
if [ "$dry" = true ]; then
  echo "Dry run: App Store Connect is only read, and no secret is set. Otherwise:"
fi
echo "This registers com.simplecapture.desktop for macOS, creates an Apple Distribution and a Mac"
echo "Installer Distribution certificate and a Mac App Store profile (unless $repo has them"
echo "already, or --renew-certs), and sets the app-store workflow's secrets on $repo."
read -r -p "Go ahead? [y/N] " ok
case "$ok" in
  y | Y | yes) ;;
  *) echo "Nothing done."; exit 1 ;;
esac

rc=0
python3 -I scripts/asc/asc.py setup --repo "$repo" --key-file "${keys[0]}" \
  --key-id "$admin_id" --issuer-id "$issuer" "${ci_args[@]}" ${flags[@]+"${flags[@]}"} || rc=$?
if [ "$rc" != 0 ] && [ "$rc" != 3 ]; then
  exit "$rc"
fi
if [ "$dry" = true ]; then
  echo "Dry run: nothing was changed."
  exit 0
fi
echo
gh secret list --repo "$repo" | grep -E '^(ASC|MAS)_' || true

cat <<EOF

Next steps (App Store Connect, https://appstoreconnect.apple.com):
EOF
if [ "$rc" = 3 ]; then
  cat <<'EOF'
  * Apps > + > New App: platform macOS, name Simple Capture, primary language English (U.S.),
    bundle ID com.simplecapture.desktop, SKU simple-capture-mac. Then run this script again:
    it keeps the certificates and adds ASC_APP_ID.
EOF
fi
cat <<'EOF'
  * App Privacy > Get Started > "No, we do not collect data from this app" > Publish.
  * Business: accept any pending agreements, and declare your EU trader status (Digital
    Services Act), or the app is not offered in the EU.
  * App Review contact (kept out of the repository): run
      gh secret set ASC_REVIEW_CONTACT_FIRST_NAME   (and _LAST_NAME, _PHONE, _EMAIL)
    each asks for its value; or enter them once under the version's App Review Information.
EOF
if [ "${#keys[@]}" = 2 ] && [ "$rc" = 3 ]; then
  cat <<'EOF'
  * Keep both .p8 files for now: the run after the app record exists needs the Admin key
    again. Once that run has set ASC_APP_ID, revoke the Admin key (Users and Access >
    Integrations > App Store Connect API) and delete its .p8.
EOF
elif [ "${#keys[@]}" = 2 ]; then
  cat <<'EOF'
  * CI uses the App Manager key, so you can revoke the Admin key now (Users and Access >
    Integrations > App Store Connect API) and delete its .p8. Keep the App Manager key's .p8
    outside the repository: renewing the certificates needs it again.
EOF
else
  cat <<'EOF'
  * CI uses this Admin key (ASC_KEY_P8). Before you revoke it, run this script again with an
    App Manager key as the second argument, so CI gets that key instead.
EOF
fi
cat <<'EOF'
  * The certificates and the profile last a year. When app-store fails on them, run this
    script again with --renew-certs (a new Admin key first, the App Manager key second).
EOF
if [ "$renew" = false ]; then
  cat <<'EOF'
  * First build without submitting:
      gh workflow run app-store.yml -f tag=v0.1.0 -f submit=false
    Then install it from TestFlight (add yourself to an internal group), allow Screen
    Recording, Camera and Microphone once, and try a recording, an iPhone source and an
    export to a chosen folder. Publishing a GitHub release, or running the workflow with
    submit=true, submits that version for review.
EOF
fi
