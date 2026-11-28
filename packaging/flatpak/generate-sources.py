#!/usr/bin/env python3
"""Regenerates the offline source lists the Flatpak manifest builds from. Run it after changing
Cargo.lock, ui/pnpm-lock.yaml, third_party/versions.toml, the gifski lock or the skia-safe
version, and commit what it writes:

  cargo-sources.json    every crate of the app, gifski and cargo-about (vendored, offline)
  node-sources.json     the UI's pnpm store, under ui/, and pnpm itself
  helper-sources.json   the export helpers' pinned tarballs, into third_party/build/downloads
  skia-sources.json     Skia at the milestone skia-bindings pins, the DEPS externals it builds,
                        and gn at the revision Skia's bin/fetch-gn names

  packaging/flatpak/generate-sources.py [--tools DIR]

Needs flatpak-builder-tools (cloned into DIR at FBT_COMMIT when missing) and a Python with
aiohttp, tomlkit and PyYAML plus its node generator installed (pip install aiohttp tomlkit PyYAML
DIR/node). Network access: crates.io, the npm registry, GitHub.
"""
import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
import tarfile
import tempfile
import urllib.request
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
FBT_URL = "https://github.com/flatpak/flatpak-builder-tools.git"
FBT_COMMIT = "9d86d336a39b9af38b86f114b6ce5820fe260f91"
CARGO_ABOUT = "0.9.3"
# pnpm 12 reads the v11 store layout (flatpak-node-generator README).
PNPM_STORE = "v11"
# Externals in Skia's DEPS that a Linux build never reads (Windows, the V8 and emsdk tooling).
SKIA_SKIP = {"d3d12allocator", "v8", "emsdk", "dawn", "buildtools"}


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def download(url, dest):
    req = urllib.request.Request(url, headers={"User-Agent": "flatpak-sources-generator"})
    with urllib.request.urlopen(req) as r, open(dest, "wb") as f:
        while chunk := r.read(1 << 20):
            f.write(chunk)


def tools(dir_):
    d = Path(dir_)
    if not (d / "cargo" / "flatpak-cargo-generator.py").exists():
        subprocess.run(["git", "clone", "-q", FBT_URL, str(d)], check=True)
    subprocess.run(["git", "-C", str(d), "checkout", "-q", FBT_COMMIT], check=True)
    return d


def write(name, data):
    (HERE / name).write_text(json.dumps(data, indent=2) + "\n")
    print(f"wrote {name}: {len(data)} sources")


def cargo_sources(fbt, tmp):
    gen = fbt / "cargo" / "flatpak-cargo-generator.py"
    url = f"https://static.crates.io/crates/cargo-about/cargo-about-{CARGO_ABOUT}.crate"
    crate = tmp / "cargo-about.crate"
    download(url, crate)
    with tarfile.open(crate) as t:
        t.extractall(tmp, filter="data")
    locks = [
        ROOT / "Cargo.lock",
        next((ROOT / "third_party" / "gifski").glob("gifski-*.Cargo.lock")),
        tmp / f"cargo-about-{CARGO_ABOUT}" / "Cargo.lock",
    ]
    seen, out = set(), []
    for i, lock in enumerate(locks):
        o = tmp / f"cargo-{i}.json"
        subprocess.run([sys.executable, str(gen), str(lock), "-o", str(o)], check=True)
        for s in json.loads(o.read_text()):
            key = (s.get("dest"), s.get("dest-filename"))
            if key not in seen:
                seen.add(key)
                out.append(s)
    out.append({"type": "archive", "archive-type": "tar-gzip", "url": url,
                "sha256": sha256(crate), "dest": "flatpak-tools/cargo-about"})
    write("cargo-sources.json", out)


def node_sources(tmp):
    o = tmp / "node.json"
    subprocess.run(["flatpak-node-generator", "pnpm", str(ROOT / "ui" / "pnpm-lock.yaml"),
                    "--pnpm-store-version", PNPM_STORE, "-o", str(o)], check=True)
    out = []
    for s in json.loads(o.read_text()):
        s["dest"] = "ui/" + s["dest"] if s.get("dest") else "ui"
        out.append(s)
    workflow = (ROOT / ".github" / "workflows" / "package.yml").read_text()
    version = re.search(r'PNPM_VERSION:\s*"([^"]+)"', workflow).group(1)
    url = f"https://registry.npmjs.org/pnpm/-/pnpm-{version}.tgz"
    tgz = tmp / "pnpm.tgz"
    download(url, tgz)
    out.append({"type": "archive", "url": url, "sha256": sha256(tgz), "dest": "flatpak-tools/pnpm"})
    # pnpm 12 is a native binary in a per-platform package; without it pnpm downloads one.
    for arch, target in (("x86_64", "linux-x64"), ("aarch64", "linux-arm64")):
        url = f"https://registry.npmjs.org/@pnpm/exe.{target}/-/exe.{target}-{version}.tgz"
        exe = tmp / f"pnpm-{target}.tgz"
        download(url, exe)
        out.append({"type": "archive", "url": url, "sha256": sha256(exe), "only-arches": [arch],
                    "dest": f"flatpak-tools/pnpm/node_modules/@pnpm/exe.{target}"})
    write("node-sources.json", out)


def pins():
    sec, out = None, {}
    for line in (ROOT / "third_party" / "versions.toml").read_text().splitlines():
        if m := re.match(r"^\[(.+)\]$", line):
            sec = m.group(1)
            out[sec] = {}
        elif sec and (m := re.match(r'^(\w+)\s*=\s*"(.*)"\s*$', line)):
            out[sec][m.group(1)] = m.group(2)
    return out


def helper_sources():
    out = []
    for p in pins().values():
        for url, file, sha in (("url", "file", "sha256"),
                               ("license_url", "license_file", "license_sha256")):
            if url in p:
                out.append({"type": "file", "url": p[url], "sha256": p[sha],
                            "dest": "third_party/build/downloads", "dest-filename": p[file]})
    write("helper-sources.json", out)


def skia_sources(tmp):
    lock = (ROOT / "Cargo.lock").read_text()
    version = re.search(r'name = "skia-bindings"\nversion = "([^"]+)"', lock).group(1)
    crate = tmp / "skia-bindings.crate"
    download(f"https://static.crates.io/crates/skia-bindings/skia-bindings-{version}.crate", crate)
    with tarfile.open(crate) as t:
        manifest = t.extractfile(f"skia-bindings-{version}/Cargo.toml").read().decode()
    tag = re.search(r'^skia = "([^"]+)"', manifest, re.M).group(1)
    url = f"https://github.com/rust-skia/skia/archive/refs/tags/{tag}.tar.gz"
    arch = tmp / "skia.tar.gz"
    download(url, arch)
    with tarfile.open(arch) as t:
        top = t.getmembers()[0].name.split("/")[0]
        deps = t.extractfile(f"{top}/DEPS").read().decode()
        fetch_gn = t.extractfile(f"{top}/bin/fetch-gn").read().decode()
    out = [{"type": "archive", "url": url, "sha256": sha256(arch), "dest": "skia"}]
    for path, repo, commit in re.findall(
            r'"(third_party/externals/[^"]+)"\s*:\s*"([^"@]+)@([0-9a-f]{40})"', deps):
        if path.rsplit("/", 1)[1] in SKIA_SKIP:
            continue
        out.append({"type": "git", "url": repo, "commit": commit, "dest": f"skia/{path}"})
    gn = re.search(r"rev = '([0-9a-f]{40})'", fetch_gn).group(1)
    # gn's build/gen.py derives its version from the history, so the clone must be deep.
    out.append({"type": "git", "url": "https://gn.googlesource.com/gn", "commit": gn,
                "dest": "flatpak-tools/gn", "disable-shallow-clone": True})
    write("skia-sources.json", out)


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--tools", default=os.environ.get(
        "FBT_DIR", str(ROOT / "target" / "flatpak-builder-tools")))
    args = ap.parse_args()
    fbt = tools(args.tools)
    with tempfile.TemporaryDirectory() as t:
        tmp = Path(t)
        cargo_sources(fbt, tmp)
        node_sources(tmp)
        helper_sources()
        skia_sources(tmp)


if __name__ == "__main__":
    main()
