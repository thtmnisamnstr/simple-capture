TP_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
TP_VERSIONS="$TP_ROOT/versions.toml"
TP_BUILD="$TP_ROOT/build"
TP_DOWNLOADS="$TP_BUILD/downloads"
TP_OUT="$TP_ROOT/out"

die() { echo "error: $*" >&2; exit 1; }
log() { echo "==> $*" >&2; }

pin() {
    local v
    v=$(awk -v sec="[$1]" -v key="$2" '
        /^\[/ { insec = ($0 == sec); next }
        insec {
            k = $0; sub(/[ \t]*=.*/, "", k)
            if (k == key) { v = $0; sub(/^[^=]*=[ \t]*"/, "", v); sub(/"[ \t]*$/, "", v); print v; exit }
        }' "$TP_VERSIONS")
    [ -n "$v" ] || die "versions.toml: missing [$1] $2"
    printf '%s\n' "$v"
}

sha256_of() {
    if command -v sha256sum >/dev/null; then sha256sum "$1" | awk '{print $1}'
    else shasum -a 256 "$1" | awk '{print $1}'; fi
}

fetch() {
    local sec=$1 url file sha dest got
    url=$(pin "$sec" "${2:-url}")
    file=$(pin "$sec" "${3:-file}")
    sha=$(pin "$sec" "${4:-sha256}")
    dest="$TP_DOWNLOADS/$file"
    mkdir -p "$TP_DOWNLOADS"
    if [ ! -f "$dest" ]; then
        log "download $sec: $url"
        curl -fL --retry 3 --proto '=https' --tlsv1.2 -o "$dest.part" "$url" || die "download failed: $url"
        mv "$dest.part" "$dest"
    fi
    got=$(sha256_of "$dest")
    if [ "$got" != "$sha" ]; then
        mv "$dest" "$dest.bad"
        die "sha256 mismatch for $file: expected $sha, got $got (moved to $file.bad)"
    fi
    log "verified $file"
    printf '%s\n' "$dest"
}

unpack() {
    rm -rf "$2"
    mkdir -p "$2"
    tar -xf "$1" -C "$2" --strip-components 1
}

ncpu() { nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4; }
