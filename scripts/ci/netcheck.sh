#!/bin/bash
# No networking code (M1 exit checklist item 8, wp-ci):
#  1. Source grep of crates/ app/ cli/ tools/ for socket and fetch APIs.
#  2. ui/src: no WebSocket, XMLHttpRequest, EventSource, sendBeacon, no remote http(s) URLs.
#  3. Tauri CSP (tauri.conf.json, tauri.bundle.conf.json): connect-src and every other directive
#     allow only local schemes and *.localhost (ipc, the sc: protocol).
#  4. The FFmpeg helper is configured with --disable-network (build script and, when present,
#     the recorded buildconf of a built helper).
set -uo pipefail
cd "$(dirname "$0")/../.." || exit 1
fail=0

GREP_EXCLUDES=(--exclude-dir=target --exclude-dir=node_modules --exclude-dir=gen --exclude-dir=dist
    --exclude-dir=binaries --exclude-dir=icons)

rust_hits=$(grep -rnI -E 'std::net|TcpStream|UdpSocket|TcpListener|tokio::net|fetch\(|XMLHttpRequest|WebSocket' \
    "${GREP_EXCLUDES[@]}" crates app cli tools 2>/dev/null)
if [ -n "$rust_hits" ]; then
    echo "netcheck: networking API in crates/ app/ cli/ tools/:" >&2
    printf '%s\n' "$rust_hits" >&2
    fail=1
fi

ui_hits=$(grep -rnI -E 'WebSocket|XMLHttpRequest|EventSource|sendBeacon|https?://' ui/src 2>/dev/null \
    | grep -v -E 'https?://(ipc|sc)\.localhost|http://www\.w3\.org/')
if [ -n "$ui_hits" ]; then
    echo "netcheck: network API or remote URL in ui/src:" >&2
    printf '%s\n' "$ui_hits" >&2
    fail=1
fi

python3 -I - app/tauri.conf.json app/tauri.bundle.conf.json <<'PY' || fail=1
import json, re, sys

LOCAL_SCHEMES = {"'self'", "'none'", "'unsafe-inline'", "ipc:", "sc:", "data:", "blob:", "asset:"}
LOCAL_HOST = re.compile(r"^https?://[a-z0-9-]+\.localhost$")
bad = 0
seen = 0
for path in sys.argv[1:]:
    try:
        with open(path) as f:
            conf = json.load(f)
    except FileNotFoundError:
        continue
    csp = conf.get("app", {}).get("security", {}).get("csp")
    if csp is None:
        if path.endswith("tauri.conf.json"):
            print(f"netcheck: {path}: no CSP", file=sys.stderr)
            bad += 1
        continue
    seen += 1
    if isinstance(csp, dict):
        directives = {k: (v if isinstance(v, list) else str(v).split()) for k, v in csp.items()}
    else:
        directives = {}
        for part in str(csp).split(";"):
            words = part.split()
            if words:
                directives[words[0]] = words[1:]
    if "connect-src" not in directives and "default-src" not in directives:
        print(f"netcheck: {path}: CSP has neither connect-src nor default-src", file=sys.stderr)
        bad += 1
    for name, sources in directives.items():
        for s in sources:
            if s in LOCAL_SCHEMES or LOCAL_HOST.match(s):
                continue
            print(f"netcheck: {path}: CSP {name} allows non-local source {s!r}", file=sys.stderr)
            bad += 1
if seen == 0:
    print("netcheck: no Tauri CSP found", file=sys.stderr)
    bad += 1
sys.exit(1 if bad else 0)
PY

if ! grep -q -- '--disable-network' third_party/ffmpeg/build-macos.sh; then
    echo "netcheck: third_party/ffmpeg/build-macos.sh lacks --disable-network" >&2
    fail=1
fi
for bc in third_party/out/*/ffmpeg.buildconf.txt; do
    [ -f "$bc" ] || continue
    grep -q -- '--disable-network' "$bc" || { echo "netcheck: $bc lacks --disable-network" >&2; fail=1; }
done

[ $fail = 0 ] && echo "netcheck: ok" || echo "netcheck: FAILED" >&2
exit $fail
