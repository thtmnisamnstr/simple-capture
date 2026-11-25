#!/usr/bin/env bash
# Checks a macOS build: signatures with the hardened runtime, the exact entitlements of every
# executable, Info.plist, LGPL-only helpers, the license set, and for the Mac App Store edition
# the embedded provisioning profile and the installer package. Never runs anything from the
# bundle. The DMG and the Mac App Store edition are the same app; only the signature, the app's
# entitlements file and the profile differ.
#   scripts/mas-check.sh [--dmg] [--adhoc] PATH.app [PATH.pkg]
#     --dmg     the Developer ID (DMG) edition: app/macos/entitlements.plist, a Developer ID
#               signature with secure timestamps, no provisioning profile and no .pkg
#     --adhoc   signed ad hoc (or with a local certificate): skips identity, team, certificate,
#               timestamp and package-signature checks
#   scripts/mas-check.sh --profile PATH.provisionprofile [IDENTITY]
#     before a build: the profile is a current Mac App Store profile for the identifier in
#     app/macos/entitlements.appstore.plist; with IDENTITY (a codesigning identity's name or
#     SHA-1), one of its valid certificates is in the profile, and that certificate's SHA-1 is
#     printed
#   scripts/mas-check.sh --entitlements
#     only the entitlement files (any OS): entitlements.appstore.plist is entitlements.plist plus
#     exactly the application and team identifiers; helpers get exactly app-sandbox + inherit
set -euo pipefail
adhoc=0
dmg=0
profile_only=""
ents_only=0
identity=""
while [ $# -gt 0 ]; do
  case "$1" in
    --adhoc) adhoc=1 ;;
    --dmg) dmg=1 ;;
    *) break ;;
  esac
  shift
done
case "${1:-}" in
  --entitlements) ents_only=1 ;;
  --profile)
    profile_only=${2:?usage: scripts/mas-check.sh --profile PATH.provisionprofile [IDENTITY]}
    identity=${3:-}
    [ -f "$profile_only" ] || { echo "mas-check: no provisioning profile at $profile_only" >&2; exit 1; }
    profile_only="$(cd "$(dirname "$profile_only")" && pwd)/$(basename "$profile_only")"
    ;;
esac
if [ -z "$profile_only" ] && [ $ents_only = 0 ]; then
  APP=${1:?usage: scripts/mas-check.sh [--dmg] [--adhoc] PATH.app [PATH.pkg]}
  PKG=${2:-}
  [ $dmg = 0 ] || [ -z "$PKG" ] || { echo "mas-check: --dmg takes no .pkg" >&2; exit 2; }
  APP="$(cd "$(dirname "$APP")" && pwd)/$(basename "${APP%/}")"
  [ -z "$PKG" ] || PKG="$(cd "$(dirname "$PKG")" && pwd)/$(basename "$PKG")"
fi
cd "$(dirname "$0")/.."
TEAM=K9LZ5QLRFQ
BUNDLE_ID=com.simplecapture.desktop
APP_ENTS=app/macos/entitlements.plist
STORE_ENTS=app/macos/entitlements.appstore.plist
HELPER_ENTS=app/macos/helper.entitlements.plist
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
fail=0
bad() {
  echo "mas-check: $*" >&2
  fail=1
}

plist_check() {
  python3 -I - "$@" <<'PY'
import datetime, hashlib, plistlib, re, sys

mode, *args = sys.argv[1:]
load = lambda p: plistlib.load(open(p, "rb"))
problems = []

if mode == "same":
    want, got, what = load(args[0]), load(args[1]), args[2]
    for k in sorted(set(want) | set(got)):
        if want.get(k) != got.get(k):
            problems.append(f"{what}: {k} is {got.get(k)!r}, expected {want.get(k)!r}")
elif mode == "entitlements":
    base, store, helper, team, bundle_id = load(args[0]), load(args[1]), load(args[2]), args[3], args[4]
    ids = {
        "com.apple.application-identifier": f"{team}.{bundle_id}",
        "com.apple.developer.team-identifier": team,
    }
    if base.get("com.apple.security.app-sandbox") is not True:
        problems.append(f"{args[0]} does not turn on the App Sandbox")
    for k, v in ids.items():
        if k in base:
            problems.append(f"{args[0]} has {k}: a Developer ID app without a profile will not launch")
        if store.get(k) != v:
            problems.append(f"{args[1]} {k} is {store.get(k)!r}, expected {v!r}")
    rest = {k: v for k, v in store.items() if k not in ids}
    for k in sorted(set(base) | set(rest)):
        if base.get(k) != rest.get(k):
            problems.append(f"{k} is {base.get(k)!r} in {args[0]} but {rest.get(k)!r} in {args[1]}; keep the sandbox keys identical")
    want = {"com.apple.security.app-sandbox": True, "com.apple.security.inherit": True}
    if helper != want:
        problems.append(f"{args[2]} is {helper!r}, expected exactly {want!r}")
elif mode == "info":
    info, bundle_id = load(args[0]), args[1]
    version = re.compile(r"^[0-9]+(\.[0-9]+){0,2}$")
    expect = {
        "CFBundleIdentifier": bundle_id,
        "CFBundlePackageType": "APPL",
        "ITSAppUsesNonExemptEncryption": False,
    }
    for k, v in expect.items():
        if info.get(k) != v:
            problems.append(f"Info.plist {k} is {info.get(k)!r}, expected {v!r}")
    for k in ("CFBundleVersion", "CFBundleShortVersionString"):
        if not version.match(str(info.get(k, ""))):
            problems.append(f"Info.plist {k} {info.get(k)!r} is not 1-3 dot-separated integers")
    for k in ("LSApplicationCategoryType", "LSMinimumSystemVersion", "NSCameraUsageDescription",
              "NSMicrophoneUsageDescription"):
        if not info.get(k):
            problems.append(f"Info.plist has no {k}")
elif mode == "profile":
    prof, team, app_id, ent_path = load(args[0]), args[1], args[2], args[3]
    signers = [s.lower() for s in args[4:]]
    ents = prof.get("Entitlements", {})
    app_ents = load(ent_path)
    if team not in prof.get("TeamIdentifier", []):
        problems.append(f"profile team {prof.get('TeamIdentifier')} lacks {team}")
    for k in ("com.apple.application-identifier", "com.apple.developer.team-identifier"):
        if ents.get(k) != app_ents.get(k):
            problems.append(f"profile {k} is {ents.get(k)!r}, the app signs {app_ents.get(k)!r}")
    if ents.get("com.apple.application-identifier") != f"{team}.{app_id}":
        problems.append(f"profile is for {ents.get('com.apple.application-identifier')!r}, not {team}.{app_id}")
    if prof.get("ProvisionedDevices") or prof.get("ProvisionsAllDevices"):
        problems.append("profile is a development or Developer ID profile, not Mac App Store")
    if "OSX" not in prof.get("Platform", []):
        problems.append(f"profile platform is {prof.get('Platform')}, not OSX")
    expires = prof.get("ExpirationDate")
    if not expires or expires.replace(tzinfo=datetime.timezone.utc) <= datetime.datetime.now(datetime.timezone.utc):
        problems.append(f"profile expired {expires}")
    if signers:
        known = [hashlib.sha1(c).hexdigest() for c in prof.get("DeveloperCertificates", [])]
        usable = [s for s in signers if s in known]
        if usable:
            if not problems:
                print(usable[0].upper())
        else:
            problems.append(
                f"the signing certificate {' or '.join(signers)} is not in the profile, which has "
                f"{', '.join(known) or 'no certificates'}; renew it (scripts/appstore-setup.sh --renew-certs)"
            )
for p in problems:
    print(f"mas-check: {p}", file=sys.stderr)
sys.exit(1 if problems else 0)
PY
}

plist_check entitlements "$APP_ENTS" "$STORE_ENTS" "$HELPER_ENTS" "$TEAM" "$BUNDLE_ID" || fail=1
if [ $ents_only = 1 ]; then
  [ $fail = 0 ] || { echo "mas-check: FAILED" >&2; exit 1; }
  echo "mas-check: $APP_ENTS, $STORE_ENTS and $HELPER_ENTS agree"
  exit 0
fi

if [ -n "$profile_only" ]; then
  [ $fail = 0 ] || exit 1
  security cms -D -i "$profile_only" -o "$tmp/profile.plist" 2>/dev/null ||
    { echo "mas-check: $profile_only is not a signed provisioning profile" >&2; exit 1; }
  signers=()
  if [ -n "$identity" ]; then
    while IFS= read -r sha; do signers+=("$sha"); done < <(
      security find-identity -v -p codesigning | awk -v id="$identity" '
        $1 ~ /^[0-9]+\)$/ && length($2) == 40 {
          name = substr($0, index($0, "\"") + 1)
          sub(/"[^"]*$/, "", name)
          if ((name == id || toupper($2) == toupper(id)) && !seen[$2]++) print $2
        }'
    )
    [ ${#signers[@]} -gt 0 ] || { echo "mas-check: no valid codesigning identity \"$identity\" in the keychains" >&2; exit 1; }
  fi
  plist_check profile "$tmp/profile.plist" "$TEAM" "$BUNDLE_ID" "$STORE_ENTS" ${signers[@]+"${signers[@]}"}
  exit
fi

codesign --verify --deep --strict --verbose=2 "$APP" 2>"$tmp/verify" || {
  cat "$tmp/verify" >&2
  bad "codesign --verify --deep --strict failed"
}

info="$APP/Contents/Info.plist"
plist_check info "$info" "$BUNDLE_ID" || fail=1
main=$(plutil -extract CFBundleExecutable raw "$info")
if [ $dmg = 1 ]; then
  main_ents=$APP_ENTS
  authority="Developer ID Application"
else
  main_ents=$STORE_ENTS
  authority="(Apple Distribution|3rd Party Mac Developer Application)"
fi

n=0
while IFS= read -r -d '' f; do
  kind=$(file -b "$f")
  case "$kind" in Mach-O*) ;; *) continue ;; esac
  rel=${f#"$APP"/}
  name=$(basename "$f")
  if [ "$rel" = "Contents/MacOS/$main" ]; then
    want_ents=$main_ents
    want_id=$BUNDLE_ID
  elif [ "$(dirname "$rel")" = Contents/MacOS ] && [[ $kind == *executable* ]]; then
    want_ents=$HELPER_ENTS
    want_id=$BUNDLE_ID.$name
  else
    bad "$rel: unexpected Mach-O; macos-sign.sh signs only Contents/MacOS executables"
    continue
  fi
  n=$((n + 1))
  [[ " $(lipo -archs "$f") " == *" arm64 "* ]] || bad "$rel has no arm64 slice"
  codesign -d --entitlements - --xml "$f" >"$tmp/ents$n" 2>/dev/null || true
  if [ -s "$tmp/ents$n" ]; then
    plist_check same "$want_ents" "$tmp/ents$n" "$rel entitlements" || fail=1
  else
    bad "$rel is signed without entitlements"
  fi
  codesign -dvv "$f" >"$tmp/sig$n" 2>&1 || true
  grep -qx "Identifier=$want_id" "$tmp/sig$n" || bad "$rel identifier is $(sed -n 's/^Identifier=//p' "$tmp/sig$n"), expected $want_id"
  grep -qE '^CodeDirectory .*flags=0x[0-9a-f]+\([^)]*runtime' "$tmp/sig$n" || bad "$rel is not signed with the hardened runtime"
  if [ $adhoc = 0 ]; then
    grep -qx "TeamIdentifier=$TEAM" "$tmp/sig$n" || bad "$rel is not signed by team $TEAM"
    grep -qE "^Authority=$authority: .*\($TEAM\)$" "$tmp/sig$n" ||
      bad "$rel is not signed with a $authority identity"
    [ $dmg = 0 ] || grep -q '^Timestamp=' "$tmp/sig$n" || bad "$rel has no secure timestamp"
  fi
  strings -a "$f" >"$tmp/strings"
  if [ "$want_ents" = "$main_ents" ]; then
    ! grep -q '_setPreventsActivation' "$tmp/strings" || bad "$rel uses the private _setPreventsActivation:"
  else
    grep -qF 'LGPL version 2.1 or later' "$tmp/strings" || bad "$rel does not report LGPL 2.1"
    hits=$(grep -oE '(^|[^L])GPL version [0-9.]+|LGPL version 3|--enable-(gpl|version3|nonfree|libx264|libx265)|x264_encoder|x264_param|x265_api|[Mm]ulticoreware|imagequant|gifski' "$tmp/strings" | sort -u | head -6 || true)
    [ -z "$hits" ] || bad "$rel has GPL, x264, x265 or gifski code: $(echo "$hits" | tr '\n' ' ')"
  fi
done < <(find "$APP" -type f -print0)
[ $n -ge 3 ] || bad "found $n signed executables; expected the app, sc-ffmpeg and sc-ffprobe"
[ -f "$APP/Contents/MacOS/sc-ffmpeg" ] && [ -f "$APP/Contents/MacOS/sc-ffprobe" ] || bad "sc-ffmpeg or sc-ffprobe is missing"

copyleft=$(find "$APP" \( -iname '*gifski*' -o -iname '*x264*' -o -iname '*x265*' -o -iname '*imagequant*' \) -print)
[ -z "$copyleft" ] || bad "GPL/AGPL components in the bundle: $copyleft"
scripts/ci/notices.sh --check-macos "$APP/Contents/Resources/licenses" >/dev/null || bad "the bundled licenses are not the macOS set"

profile="$APP/Contents/embedded.provisionprofile"
if [ $dmg = 1 ]; then
  [ ! -e "$profile" ] || bad "the DMG edition holds embedded.provisionprofile"
elif [ -f "$profile" ]; then
  if security cms -D -i "$profile" -o "$tmp/profile.plist" 2>/dev/null; then
    codesign -d --entitlements - --xml "$APP" >"$tmp/app-ents" 2>/dev/null || true
    cert=()
    if [ $adhoc = 0 ]; then
      if codesign -d --extract-certificates="$tmp/cert" "$APP" 2>/dev/null && [ -f "$tmp/cert0" ]; then
        cert=("$(shasum -a 1 <"$tmp/cert0" | cut -d' ' -f1)")
      else
        bad "cannot read the app's signing certificate"
      fi
    fi
    plist_check profile "$tmp/profile.plist" "$TEAM" "$BUNDLE_ID" "$tmp/app-ents" ${cert[@]+"${cert[@]}"} >/dev/null || fail=1
  else
    bad "embedded.provisionprofile is not a signed profile"
  fi
elif [ $adhoc = 0 ]; then
  bad "no Contents/embedded.provisionprofile"
else
  echo "mas-check: no embedded.provisionprofile (ad hoc)"
fi

if [ -n "$PKG" ]; then
  pkgutil --check-signature "$PKG" >"$tmp/pkgsig" 2>&1 || true
  if [ $adhoc = 0 ]; then
    grep -qE "(3rd Party Mac Developer Installer|Mac Installer Distribution): .*\($TEAM\)" "$tmp/pkgsig" || {
      cat "$tmp/pkgsig" >&2
      bad "$PKG is not signed with the Mac Installer Distribution identity"
    }
  fi
  pkgutil --expand-full "$PKG" "$tmp/pkg" >/dev/null
  payload=$(find "$tmp/pkg" -maxdepth 3 -path '*/Payload/*.app' -type d | head -1)
  if [ -z "$payload" ]; then
    bad "$PKG has no app in its payload"
  else
    grep -q 'install-location="/Applications"' "$(dirname "$(dirname "$payload")")/PackageInfo" || bad "$PKG does not install into /Applications"
    cmp -s "$payload/Contents/Info.plist" "$info" || bad "$PKG holds a different build than $APP"
    codesign --verify --deep --strict "$payload" 2>/dev/null || bad "the app inside $PKG fails codesign --verify"
  fi
fi

[ $fail = 0 ] || { echo "mas-check: FAILED" >&2; exit 1; }
echo "mas-check: $APP$([ -n "$PKG" ] && echo " and $PKG") pass$([ $dmg = 1 ] && echo ' (DMG edition)')$([ $adhoc = 1 ] && echo ' (ad hoc)')"
