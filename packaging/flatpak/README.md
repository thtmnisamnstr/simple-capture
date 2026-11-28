# Flatpak

`io.github.thtmnisamnstr.SimpleCapture.yml` builds the app the way Flathub does: offline, from
pinned sources only, on the GNOME 50 runtime.

| Module | What it builds |
| --- | --- |
| `shared/libayatana-appindicator` | the tray library (copied from [flathub/shared-modules](https://github.com/flathub/shared-modules) at the commit in `shared/SHARED_MODULES_COMMIT`) |
| `nasm` | the assembler the x264 and SVT-AV1 builds need; build-only |
| `helpers` | Rust 1.100.0 (`rust-toolchain.toml`'s pin, which Flathub's rust-stable extension trails), unpacked in the build folder; then `sc-ffmpeg`, `sc-ffprobe` and `sc-gifski` with `third_party/ffmpeg/build-linux.sh` and `third_party/gifski/build.sh`, from the tarballs in `third_party/versions.toml` |
| `simple-capture` | gn, then the UI (pnpm, offline), Skia from source (`SKIA_SOURCE_DIR`), the app, and the Rust notices (cargo-about) |

The install layout matches the .deb: the app and helpers in `/app/bin`, fonts and licenses in
`/app/lib/simple-capture` (Tauri's resource folder for `bin/../lib/<product>`).

## Regenerating the source lists

`cargo-sources.json`, `node-sources.json`, `helper-sources.json`, `rust-sources.json` and
`skia-sources.json` are generated. After changing `Cargo.lock`, `ui/pnpm-lock.yaml`, `third_party/versions.toml`, the
gifski lock or skia-safe:

```sh
python3 -m venv /tmp/fpgen && /tmp/fpgen/bin/pip install aiohttp tomlkit PyYAML
git clone https://github.com/flatpak/flatpak-builder-tools target/flatpak-builder-tools
/tmp/fpgen/bin/pip install target/flatpak-builder-tools/node
PATH=/tmp/fpgen/bin:$PATH /tmp/fpgen/bin/python packaging/flatpak/generate-sources.py
```

CI (`.github/workflows/flatpak.yml`) fails when a crate or helper pin is missing from the lists.

## Building locally

```sh
flatpak install --user flathub org.gnome.Sdk//50 org.gnome.Platform//50 \
  org.freedesktop.Sdk.Extension.node26//25.08 org.freedesktop.Sdk.Extension.llvm21//25.08 org.flatpak.Builder
flatpak run org.flatpak.Builder --user --install --force-clean build-dir \
  packaging/flatpak/io.github.thtmnisamnstr.SimpleCapture.yml
flatpak run io.github.thtmnisamnstr.SimpleCapture
```

A full build takes about an hour on 4 cores (Skia, FFmpeg's encoders and the app).

## Submitting to Flathub

Flathub builds from a manifest in its own repository, so the submission copies this one with
one change: the two `type: dir` sources become the release tag.

```yaml
      - type: git
        url: https://github.com/thtmnisamnstr/simple-capture.git
        tag: v0.1.0
        commit: <the tag's commit>
```

(the `helpers` module keeps `dest: third_party` and its `skip` list does not apply to git
sources). Then follow https://docs.flathub.org/docs/for-app-authors/submission: fork
`flathub/flathub`, branch from `new-pr`, add the manifest, the five JSON lists and `shared/`, and
open the pull request. The metainfo needs screenshots before review (Flathub requires at least
one); they are referenced by URL from the tagged repository.
