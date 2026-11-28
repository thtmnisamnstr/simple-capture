#!/usr/bin/env bash
# Builds the signed apt and dnf repositories that GitHub Pages serves (linux-repos.yml) from a
# release's .deb and .rpm files.
#   scripts/linux-repos/build.sh PACKAGES_DIR SITE_DIR [BASE_URL]
# Signs with the secret key already in the GnuPG keyring named by $REPO_KEY (a fingerprint or
# email); without $REPO_KEY it signs nothing, for local tests. Needs dpkg-scanpackages
# (dpkg-dev), createrepo_c and, to sign the .rpm files, rpmsign.
set -euo pipefail
PKGS=$(cd "$1" && pwd)
SITE=$2
BASE=${3:-https://thtmnisamnstr.github.io/simple-capture}
NAME=simple-capture
rm -rf "$SITE"
mkdir -p "$SITE"
SITE=$(cd "$SITE" && pwd)

sign() { [ -n "${REPO_KEY:-}" ]; }

# apt: one "stable" suite, component "main", every architecture the release built.
APT="$SITE/apt"
mkdir -p "$APT/pool/main"
mapfile -t debs < <(find "$PKGS" -name '*.deb' | sort)
[ ${#debs[@]} -gt 0 ] || { echo "build.sh: no .deb in $PKGS" >&2; exit 1; }
cp "${debs[@]}" "$APT/pool/main/"
archs=()
for d in "${debs[@]}"; do archs+=("$(dpkg-deb -f "$d" Architecture)"); done
mapfile -t archs < <(printf '%s\n' "${archs[@]}" | sort -u)
for a in "${archs[@]}"; do
  dir="$APT/dists/stable/main/binary-$a"
  mkdir -p "$dir"
  (cd "$APT" && dpkg-scanpackages --multiversion --arch "$a" pool/main /dev/null) > "$dir/Packages"
  gzip -9nk "$dir/Packages"
done
{
  echo "Origin: $NAME"
  echo "Label: $NAME"
  echo "Suite: stable"
  echo "Codename: stable"
  echo "Architectures: ${archs[*]}"
  echo "Components: main"
  echo "Description: simple-capture for Debian and Ubuntu"
  echo "Date: $(LC_ALL=C date -u '+%a, %d %b %Y %H:%M:%S UTC')"
  for algo in MD5Sum:md5sum SHA256:sha256sum; do
    echo "${algo%%:*}:"
    (cd "$APT/dists/stable" && find main -type f | sort | while read -r f; do
      printf ' %s %s %s\n' "$(${algo##*:} "$f" | cut -d' ' -f1)" "$(stat -c %s "$f")" "$f"
    done)
  done
} > "$APT/dists/stable/Release"
if sign; then
  gpg --batch --yes --local-user "$REPO_KEY" --clearsign -o "$APT/dists/stable/InRelease" "$APT/dists/stable/Release"
  gpg --batch --yes --local-user "$REPO_KEY" -abs -o "$APT/dists/stable/Release.gpg" "$APT/dists/stable/Release"
fi

# dnf: one repository per architecture, packages signed so gpgcheck can stay on.
mapfile -t rpms < <(find "$PKGS" -name '*.rpm' | sort)
[ ${#rpms[@]} -gt 0 ] || { echo "build.sh: no .rpm in $PKGS" >&2; exit 1; }
for r in "${rpms[@]}"; do
  arch=$(basename "$r" .rpm); arch=${arch##*.}
  mkdir -p "$SITE/rpm/$arch"
  cp "$r" "$SITE/rpm/$arch/"
done
for dir in "$SITE"/rpm/*/; do
  if sign; then
    rpmsign --define "_gpg_name $REPO_KEY" --define '_gpg_sign_cmd_extra_args --batch' --addsign "$dir"*.rpm
  fi
  createrepo_c --quiet "$dir"
  if sign; then
    gpg --batch --yes --local-user "$REPO_KEY" -abs -o "$dir/repodata/repomd.xml.asc" "$dir/repodata/repomd.xml"
  fi
done

if sign; then
  gpg --armor --export "$REPO_KEY" > "$SITE/$NAME.asc"
  gpg --export "$REPO_KEY" > "$SITE/$NAME.gpg"
fi
cat > "$SITE/$NAME.sources" <<SOURCES
Types: deb
URIs: $BASE/apt
Suites: stable
Components: main
Signed-By: /usr/share/keyrings/$NAME.gpg
SOURCES
cat > "$SITE/$NAME.repo" <<REPO
[$NAME]
name=simple-capture
baseurl=$BASE/rpm/\$basearch
enabled=1
gpgcheck=1
repo_gpgcheck=1
gpgkey=$BASE/$NAME.asc
REPO
version=$(dpkg-deb -f "${debs[0]}" Version)
sed -e "s#@BASE@#$BASE#g" -e "s#@VERSION@#$version#g" "$(dirname "$0")/index.html" > "$SITE/index.html"
touch "$SITE/.nojekyll"
echo "build.sh: $SITE holds simple-capture $version for ${archs[*]} (apt) and $(ls "$SITE/rpm" | tr '\n' ' ')(dnf)$(sign || echo ', unsigned')"
