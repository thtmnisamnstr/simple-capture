#!/usr/bin/env bash
# Checks a Mac App Store build before upload: signatures, the exact entitlements of every
# executable, the embedded provisioning profile, Info.plist, LGPL-only helpers, the license set
# and the installer package. Never runs anything from the bundle.
#   scripts/mas-check.sh [--adhoc] PATH.app [PATH.pkg]
#     --adhoc   signed ad hoc (scripts/mas-build.sh --adhoc): skips identity, team, certificate
#               and package-signature checks
#   scripts/mas-check.sh --profile PATH.provisionprofile [IDENTITY]
#     before a build: the profile is a current Mac App Store profile for the identifier in
#     app/macos/entitlements.mas.plist; with IDENTITY (a codesigning identity's name or SHA-1),
#     one of its valid certificates is in the profile, and that certificate's SHA-1 is printed
set -euo pipefail
adhoc=0
profile_only=""
identity=""
case "${1:-}" in
  --adhoc)
    adhoc=1
    shift
    ;;
  --profile)
    profile_only=${2:?usage: scripts/mas-check.sh --profile PATH.provisionprofile [IDENTITY]}
    identity=${3:-}
    [ -f "$profile_only" ] || { echo "mas-check: no provisioning profile at $profile_only" >&2; exit 1; }
    profile_only="$(cd "$(dirname "$profile_only")" && pwd)/$(basename "$profile_only")"
    ;;
esac
if [ -z "$profile_only" ]; then
  APP=${1:?usage: scripts/mas-check.sh [--adhoc] PATH.app [PATH.pkg]}
  PKG=${2:-}
  APP="$(cd "$(dirname "$APP")" && pwd)/$(basename "${APP%/}")"
  [ -z "$PKG" ] || PKG="$(cd "$(dirname "$PKG")" && pwd)/$(basename "$PKG")"
fi
cd "$(dirname "$0")/.."
TEAM=K9LZ5QLRFQ
BUNDLE_ID=com.simplecapture.desktop
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

if [ -n "$profile_only" ]; then
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
  plist_check profile "$tmp/profile.plist" "$TEAM" "$BUNDLE_ID" app/macos/entitlements.mas.plist ${signers[@]+"${signers[@]}"}
  exit
fi

codesign --verify --deep --strict --verbose=2 "$APP" 2>"$tmp/verify" || {
  cat "$tmp/verify" >&2
  bad "codesign --verify --deep --strict failed"
}

info="$APP/Contents/Info.plist"
plist_check info "$info" "$BUNDLE_ID" || fail=1
main=$(plutil -extract CFBundleExecutable raw "$info")

n=0
while IFS= read -r -d '' f; do
  kind=$(file -b "$f")
  case "$kind" in Mach-O*) ;; *) continue ;; esac
  rel=${f#"$APP"/}
  name=$(basename "$f")
  if [ "$rel" = "Contents/MacOS/$main" ]; then
    want_ents=app/macos/entitlements.mas.plist
    want_id=$BUNDLE_ID
  elif [ "$(dirname "$rel")" = Contents/MacOS ] && [[ $kind == *executable* ]]; then
    want_ents=app/macos/helper.mas.entitlements.plist
    want_id=$BUNDLE_ID.$name
  else
    bad "$rel: unexpected Mach-O; mas-build.sh signs only Contents/MacOS executables"
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
  if [ $adhoc = 0 ]; then
    grep -qx "TeamIdentifier=$TEAM" "$tmp/sig$n" || bad "$rel is not signed by team $TEAM"
    grep -qE "^Authority=(Apple Distribution|3rd Party Mac Developer Application): .*\($TEAM\)$" "$tmp/sig$n" ||
      bad "$rel is not signed with an Apple Distribution identity"
  fi
  strings -a "$f" >"$tmp/strings"
  if [ "$want_ents" = app/macos/entitlements.mas.plist ]; then
    ! grep -q '_setPreventsActivation' "$tmp/strings" || bad "$rel uses the private _setPreventsActivation: (build with --features mas)"
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
if grep -q -- '--check-mas' scripts/ci/notices.sh; then
  scripts/ci/notices.sh --check-mas "$APP/Contents/Resources/licenses" >/dev/null || bad "the bundled licenses are not the Mac App Store set"
else
  bad "scripts/ci/notices.sh has no --check-mas"
fi

profile="$APP/Contents/embedded.provisionprofile"
if [ -f "$profile" ]; then
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
echo "mas-check: $APP$([ -n "$PKG" ] && echo " and $PKG") pass$([ $adhoc = 1 ] && echo ' (ad hoc)')"
