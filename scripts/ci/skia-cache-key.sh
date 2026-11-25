#!/bin/bash
# Usage: scripts/ci/skia-cache-key.sh PROFILE (prints version= and key= for GITHUB_OUTPUT)
# The key covers everything that gives the skia-bindings build script a new output directory,
# so a changed toolchain or build dependency never restores a directory Cargo will not use.
set -euo pipefail
cd "$(dirname "$0")/../.."
profile=${1:?usage: skia-cache-key.sh PROFILE}

h12() { git hash-object --stdin | cut -c1-12; }
lf() { tr -d '\r' < "$1"; }

closure=$(awk '
    function unquote(s) { return substr(s, 2, length(s) - 2) }
    /^\[\[package\]\]$/ { inpkg = 1; name = ""; next }
    /^\[/ { inpkg = 0; next }
    !inpkg { next }
    /^name = / { name = unquote($3); next }
    /^version = / {
        id = name " " unquote($3)
        versions[name] = (name in versions) ? versions[name] " " unquote($3) : unquote($3)
        next
    }
    /^source = / { source[id] = $3; next }
    /^checksum = / { checksum[id] = $3; next }
    /^dependencies = \[$/ { indeps = 1; next }
    indeps && /^\]$/ { indeps = 0; next }
    indeps { gsub(/[",]/, ""); deps[id] = deps[id] "\t" $1 (NF > 1 ? " " $2 : ""); next }
    END {
        if (!("skia-bindings" in versions) || index(versions["skia-bindings"], " ")) {
            print "skia-cache-key: expected exactly one skia-bindings in Cargo.lock" > "/dev/stderr"
            exit 1
        }
        head = 1; tail = 1
        queue[1] = "skia-bindings " versions["skia-bindings"]; seen[queue[1]] = 1
        while (head <= tail) {
            id = queue[head++]
            print id, source[id], checksum[id]
            n = split(deps[id], list, "\t")
            for (i = 1; i <= n; i++) {
                if (list[i] == "") continue
                dep = index(list[i], " ") ? list[i] : list[i] " " versions[list[i]]
                if (!(dep in seen)) { seen[dep] = 1; queue[++tail] = dep }
            }
        }
    }
' <(lf Cargo.lock) | LC_ALL=C sort)

version=$(printf '%s\n' "$closure" | awk '$1 == "skia-bindings" { print $2 }')
features=$(lf Cargo.toml | grep -E '^skia-safe *=' | h12)
profiles=$(lf Cargo.toml | awk '/^\[/ { p = /^\[profile[.]/ } p' | h12)
config=$(lf .cargo/config.toml | h12)
lock=$(printf '%s\n' "$closure" | h12)
rustc=$(rustc -vV | h12)

echo "version=$version"
echo "key=skia-$RUNNER_OS-$RUNNER_ARCH-${ImageOS:-unknown}-$profile-$version-$features-$profiles-$config-$lock-$rustc"
