#!/usr/bin/env bash
# Writes the AUR package (simple-capture-bin) for a release: PKGBUILD and .SRCINFO in OUT_DIR,
# with the checksums of the release's .deb files (downloaded, or read from DEB_DIR).
#   packaging/aur/update.sh VERSION OUT_DIR [DEB_DIR]
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
version=${1:?version, like 0.1.0}
out=${2:?output folder}
debs=${3:-}
mkdir -p "$out"
sha() {
  local arch=$1 file=simple-capture_${version}_$1.deb
  if [ -n "$debs" ]; then
    sha256sum "$debs/$file" | cut -d' ' -f1
  else
    curl -fsSL "https://github.com/thtmnisamnstr/simple-capture/releases/download/v$version/$file" | sha256sum | cut -d' ' -f1
  fi
}
amd64=$(sha amd64)
arm64=$(sha arm64)
sed -e "s/@VERSION@/$version/g" -e "s/@SHA_AMD64@/$amd64/" -e "s/@SHA_ARM64@/$arm64/" \
  "$here/PKGBUILD.in" > "$out/PKGBUILD"
# .SRCINFO without makepkg: the same fields `makepkg --printsrcinfo` writes for this PKGBUILD.
(
  # shellcheck source=/dev/null
  . "$out/PKGBUILD"
  printf 'pkgbase = %s\n\tpkgdesc = %s\n\tpkgver = %s\n\tpkgrel = %s\n\turl = %s\n' \
    "$pkgname" "$pkgdesc" "$pkgver" "$pkgrel" "$url"
  for a in "${arch[@]}"; do printf '\tarch = %s\n' "$a"; done
  for l in "${license[@]}"; do printf '\tlicense = %s\n' "$l"; done
  for d in "${depends[@]}"; do printf '\tdepends = %s\n' "$d"; done
  for d in "${optdepends[@]}"; do printf '\toptdepends = %s\n' "$d"; done
  for p in "${provides[@]}"; do printf '\tprovides = %s\n' "$p"; done
  for c in "${conflicts[@]}"; do printf '\tconflicts = %s\n' "$c"; done
  for o in "${options[@]}"; do printf '\toptions = %s\n' "$o"; done
  printf '\tsource_x86_64 = %s\n\tsha256sums_x86_64 = %s\n' "${source_x86_64[0]}" "${sha256sums_x86_64[0]}"
  printf '\tsource_aarch64 = %s\n\tsha256sums_aarch64 = %s\n' "${source_aarch64[0]}" "${sha256sums_aarch64[0]}"
  printf '\npkgname = %s\n' "$pkgname"
) > "$out/.SRCINFO"
echo "update.sh: $out/PKGBUILD and .SRCINFO for $version"
