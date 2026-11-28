# Linux distribution

## Status (2026-11-28, branch linux-distribution)

What exists, and what each channel still needs from the owner. The plan below (§1 onward,
researched 2026-11-27) is the background; where the two differ, this section wins.

| Channel | Built by | State | Owner's step to publish |
|---|---|---|---|
| `.deb`, `.rpm`, AppImage (x86_64, arm64) | `package.yml` bundle job | Real distro dependencies (`app/tauri.linux.conf.json`), the app-ID desktop file and metainfo inside, checked in CI | Push a `v*` tag; publish the draft release |
| apt and dnf repositories | `package.yml` (check) and `linux-repos.yml` (publish) | `scripts/linux-repos/build.sh` makes signed repos; CI installs from them in clean Ubuntu 24.04, Debian 13 and Fedora containers on every build | Once: `scripts/linux-repos/new-key.sh` (sets `LINUX_REPO_GPG_KEY`), and Settings → Pages → Source: GitHub Actions. Then each published release deploys to https://thtmnisamnstr.github.io/simple-capture |
| Snap (strict, core24) | `package.yml` snap job | The `.deb` repackaged on the GNOME content snap; CI checks the snap's contents | Register the name `simple-capture` (`snapcraft register`), export credentials (`snapcraft export-login`) into the `SNAPCRAFT_STORE_CREDENTIALS` secret; tags then publish to edge. Promote to stable by hand after a test on a real desktop |
| Flathub | `flatpak.yml` | Offline manifest in `packaging/flatpak` (GNOME 50, Skia from source, helpers from the same pins), built for x86_64 and aarch64 and linted in CI; metainfo has screenshots | After a tagged release: the submission steps in `packaging/flatpak/README.md` (a PR to `flathub/flathub`) |

Decisions taken (they were §7's questions):
- **App ID:** `io.github.thtmnisamnstr.SimpleCapture` on every Linux package (Flathub verifies
  it through GitHub). GTK's program name is the app ID, so windows match the desktop file. The
  Tauri identifier stays `com.simplecapture.desktop`, so the data folder doesn't move and macOS
  is untouched.
- **Oldest distro:** Ubuntu 24.04 / Debian 13 / current Fedora (packages are built on
  ubuntu-24.04). Older systems use the Flatpak.
- **Repo hosting:** GitHub Pages, deployed from an Actions artifact (no binaries in git history),
  holding only the newest release (about 150 MB per release, well under the 1 GB limit).
- **Not pursued:** Copr and openSUSE OBS (they refuse x264/x265), official Debian/Fedora packages
  (they would require unbundling every crate and the FFmpeg build), and a Launchpad PPA (only if
  someone asks for it).

Not verified yet: the Snap and Flatpak on a real desktop. The PipeWire mic and system-audio
path, the tray and the camera inside the sandboxes have only been built, not run. Run each once
on GNOME and KDE before promoting.

## 1. Things that affect every channel

**1.1 The app ID can't go on Flathub as it is.** Flathub's linter rule
`appid-ends-with-lowercase-desktop` is in the "never granted" class [2], and the requirements page
says an ID must not end in `.desktop`, `.app` or `.linux` [1]. That rules out
`com.simplecapture.desktop`. On the host, a second constraint applies: an unsandboxed app that
calls `org.freedesktop.host.portal.Registry.Register` (as `crates/capture-linux/src/imp/portal.rs`
now does) must pass an ID that matches the basename of an installed `.desktop` file [3]. Restore
tokens and GlobalShortcuts are stored under that ID. Tauri names the desktop file after
`productName` (`simple-capture.desktop`) and the icons after the binary [4]. So **the current
`.deb` has no `com.simplecapture.desktop.desktop`, and the Register call has nothing to match.**
The fix doesn't depend on any channel: install `<app-id>.desktop` through `bundle.linux.*.files`
(or rename it in CI), and check the result with `dpkg-deb -c`. Options for the ID itself:
- (a) Keep `com.simplecapture.desktop` on the host packages and use a separate Flatpak ID. This
  works because Flatpak apps get their ID from the sandbox and keep their data in
  `~/.var/app/<id>/`. The cost is that one app ends up with two IDs in GNOME Software and in
  AppStream.
- (b) Move every Linux package to one new ID now, while Linux is still experimental and has
  almost no users. Candidates are `io.github.thtmnisamnstr.SimpleCapture`, which Flathub verifies
  through GitHub ownership [5], or `com.simplecapture.SimpleCapture`, which needs control of
  simplecapture.com and a token at `/.well-known/org.flathub.VerifiedApps.txt` [5]. Override the
  identifier in `app/tauri.linux.conf.json`. macOS and Windows keep `com.simplecapture.desktop`,
  so the owner's macOS grants are untouched. The Linux data folder moves, so migrate it once.

  **Recommendation: (b).** Decide before any channel ships, because Flathub IDs, snap names and
  repo package names are hard to change later.

  **Status (2026-11-27, branch linux-testing):** the portal side of (b) is done. The app registers
  `io.github.thtmnisamnstr.SimpleCapture` with the portal (`APP_ID` in
  `crates/capture-linux/src/imp/portal.rs`), and on the owner's GNOME machine registration,
  restore tokens and GlobalShortcuts all work once a desktop file of that name is installed
  (`~/.local/share/applications/io.github.thtmnisamnstr.SimpleCapture.desktop` for dev runs).
  Still open: ship that desktop file in the packages, and decide whether the Tauri identifier
  (and with it the Linux data folder) moves too.

**1.2 Updates come from the channel.** The app makes no network connections, so it never checks
for updates. That is a selling point on Flathub, whose listing shows "No network access" when
`--share=network` is absent. It also means every channel has to deliver updates itself:
- apt and dnf: `apt upgrade` / `dnf upgrade`
- Flatpak and Snap: automatic background updates
- AppImage: external tools only (AppImageUpdate, Gear Lever), reading update information embedded
  in the file. The app itself never fetches anything.

**1.3 Codec patents decide where the helpers may be hosted.** FFmpeg with x264 and x265, and
gifski, are fine to host on Flathub (Flathub's OBS manifest builds x264 from source [6]), on
Launchpad (Ubuntu ships x264 itself), in our own repos and on GitHub Releases. **Copr, the
openSUSE Build Service and Fedora itself refuse them.** OBS's blacklist names x264, x265 and "full"
FFmpeg [7]. Copr allows only what Fedora could ship [8], and Fedora forbids patent-encumbered
codecs; they live in RPM Fusion [9]. That leaves Fedora/RHEL users with our own RPM repo or
Flathub.

**1.4 Offline builds.** Flathub, Launchpad and OBS all build with no network [1][10][11], and Copr
does too unless a project turns networking on. Flathub additionally bans prebuilt binaries in the
submission [1]. Make one "vendor kit" script that every source build reuses:
- **Rust**: `cargo vendor` for the workspace, or `flatpak-cargo-generator.py Cargo.lock` →
  `cargo-sources.json` [12]. Do the same separately for gifski's
  `third_party/gifski/gifski-1.34.0.Cargo.lock`.
- **UI**: `flatpak-node-generator pnpm ui/pnpm-lock.yaml` → `node-sources.json` (the generator
  supports pnpm [13]), or a pnpm offline store tarball for deb/rpm sources. Flathub has to build
  `ui/dist` itself; a shipped dist counts as a prebuilt.
- **Skia**: build from source with `FORCE_SKIA_BUILD=1`. Set `SKIA_SOURCE_DIR` to a Skia tree
  that already holds its `git-sync-deps` dependencies, because skia-bindings runs that script and
  needs network otherwise [14]. Generate one source entry per DEPS line (freetype, harfbuzz, icu,
  libpng, zlib, expat, …). On 16 KB-page kernels keep `SKIA_GN_COMMAND=/usr/bin/gn`
  (docs/TESTING_ON_WINDOWS_LINUX.md §2).
- **FFmpeg and its libraries**: already pinned with SHA-256 in `third_party/versions.toml`.
  Generate Flatpak modules or source entries from that file.
- **Toolchain**: `rust-toolchain.toml` pins 1.100, and rustup would try to download it. In
  sandboxed builds, set `RUSTUP_TOOLCHAIN=stable` (Flatpak `rust-stable` extension, which must be
  ≥ 1.100) or delete the file before the build.

**1.5 Tauri gotchas on Linux**

| Topic | What to know |
|---|---|
| WebKitGTK | Tauri 2 needs webkit2gtk-**4.1** [15]: Ubuntu 22.04+, Debian 12+ and every current Fedora have it. **RHEL 9 does not, and won't** (Red Hat closed the request WONTFIX; RHEL 9 has only 4.0). Red Hat also said RHEL 10 would not ship WebKitGTK at all [16]. Flatpak is the RHEL route; check EPEL 10 before promising an RPM there. |
| Tray library | Tauri 2 uses libayatana-appindicator and loads it at runtime, so the `.deb` doesn't depend on it automatically. Package names: `libayatana-appindicator3-1` (Debian/Ubuntu), `libayatana-appindicator-gtk3` (Fedora 41–44, EPEL 9/10) [17]. Don't depend on Fedora's older `libappindicator-gtk3`. Flathub: build it from Flathub's shared-modules. Tray clicks: Tauri sends no click events on Linux on any desktop [18]. The menu is the interface. |
| glibc baseline | An AppImage runs only where glibc is at least as new as the build machine's. Tauri says to build on the oldest system you support [19]. ubuntu-24.04 means glibc 2.39, which excludes Debian 12 (2.36) and Ubuntu 22.04 (2.35). |
| `.deb` depends | `app/tauri.bundle.conf.json` lists only webkit, gtk3, pipewire and xdg-desktop-portal. Also needed: `libayatana-appindicator3-1`; `gstreamer1.0-plugins-base`, `-good` and `-bad` (`vah264enc`, `openh264enc`, `videoconvert`); and a portal backend (`xdg-desktop-portal-gnome \| xdg-desktop-portal-kde \| xdg-desktop-portal-wlr`). RPM needs its own list in Fedora names (`bundle.linux.rpm.depends`). |
| GStreamer in the AppImage | The in-process GStreamer screen track needs the host's GStreamer and plugins unless `bundleMediaFramework` is on. Tauri supports that fully only on Ubuntu build machines, and it adds size and the "ugly" plugin licensing question [19]. Decide, then test the AppImage on Fedora. |
| arm64 | GitHub's `ubuntu-24.04-arm` runners are free and GA for public repos [20]. linuxdeploy can't cross-compile, so arm64 AppImages must be built on arm64 [19]. |

## 2. Channels

### 2.1 Flathub (first; widest reach)
- **Hosting.** A PR to `flathub/flathub` with `<id>.yml` plus the generated `cargo-sources.json`,
  `node-sources.json` and `skia-sources.json`. After acceptance, Flathub creates
  `flathub/<id>`; updates are PRs to that repo (a bot can open them). Only new apps go through
  review [1].
- **Build.** Runtime `org.gnome.Platform`/`Sdk` (Tauri's guide uses 51 [12]). It already contains
  webkit2gtk-4.1, GStreamer and the PipeWire libraries. Add the `rust-stable` and `node` SDK
  extensions (node major = the pinned one, or the closest published).
  - Modules: libayatana-appindicator (shared-modules), then x264, x265, SVT-AV1, libvpx, opus,
    ogg, vorbis and `sc-ffmpeg`/`sc-ffprobe` built by `third_party/ffmpeg/build-linux.sh` logic
    (meson/autotools modules, `--disable-network`), then gifski (cargo, offline), then the app:
    `pnpm install --offline`, `tauri build --no-bundle`, and install the binary, helpers, fonts,
    licenses, desktop file, icons and metainfo by hand.
  - Flathub builds x86_64 and aarch64 by default [1].
  - Test locally:
    `flatpak run org.flatpak.Builder --force-clean --user --repo repo build <id>.yml` and
    `flatpak run --command=flatpak-builder-lint org.flatpak.Builder manifest <id>.yml` [12].
- **Sandbox (finish-args).** Flathub requires a portal wherever one exists [1]. Here is what each
  part of the app needs:
  - Screen capture: the ScreenCast portal (the app already uses it).
  - Global shortcuts: the GlobalShortcuts portal.
  - Saving to another folder: the FileChooser portal.
  - "Show in folder": OpenURI.
  - Recordings in ~/Videos: `--filesystem=xdg-videos`.
  - Static permissions: `--socket=wayland --socket=fallback-x11 --share=ipc`.
  - Mic and system audio through cpal's PipeWire host: `--filesystem=xdg-run/pipewire-0`, as OBS
    does [6]. Verify that WirePlumber lets a sandboxed client see and link the sink monitor.
  - Tray: `--talk-name=org.kde.StatusNotifierWatcher` plus `--filesystem=xdg-run/tray-icon:create`,
    or point the tray's temp dir at the app cache [12].
  - Cameras and VA-API/NVENC: `--device=all`, as OBS does [6]. Raw V4L2 needs it, and it includes
    `dri`. Listing both gets the `finish-args-redundant-device-all` warning [2].
  - **Risk:** a reviewer may ask for the Camera portal (PipeWire cameras) instead of
    `--device=all`. Have an answer ready (MJPEG/YUYV control, libcamera coverage), or plan a
    PipeWire camera path.
  - No `--share=network`.
- **Signing.** Flathub signs; nothing for us to do.
- **Helpers' source.** Every source is a public URL in the manifest, and Flathub publishes a
  Sources extension. That extension comes from x86_64 pipelines only, and Flathub keeps only the
  last ~3 builds [21]. Keep attaching our source tarball to each GitHub release, and keep the
  About → Licenses "Source code" button.
- **Effort.** 6–10 days. The Skia offline source list and the FFmpeg modules are most of it.
  Expect 1–4 weeks of review.

### 2.2 Snap Store
- **Build.** `snap/snapcraft.yaml` with `base: core24`, `confinement: strict`, and the `gnome`
  extension (WebKitGTK 4.1 and GStreamer from the GNOME content snap). Tauri's guide uses core22
  and dumps a `.deb` built inside the part [22]; use the same pattern with core24. Build both
  arches with `snapcore/action-build` on `ubuntu-24.04` and `ubuntu-24.04-arm`, or with Launchpad
  remote-build. Publish with `snapcraft upload --release=edge`, then promote to stable.
- **Confinement.** Classic isn't needed, and it would likely be refused: classic is granted only
  for listed categories after a manual review [23]. Strict works with portals:
  - `desktop` (auto-connects) carries the ScreenCast and GlobalShortcuts portals.
  - `screencast-legacy` is the old GNOME-Shell D-Bus path; don't use it.
  - Plugs: `camera`, `audio-record` and `pipewire`. None of the three auto-connects [24]. Ask for
    auto-connect in the forum's store-requests category; until then users run
    `snap connect simple-capture:camera`.
  - Also needed: `audio-playback`, `wayland`, `x11`, `opengl` and `home` (auto-connects on
    classic distros [24]).
  - Verify in a real strict build: the tray (SNI over D-Bus), and cpal's PipeWire client through
    the `pipewire` plug. Forum reports show PipeWire-in-strict-snap breakage [25], so test that
    first.
  - A May 2026 KDE thread: a recorder failed in Snap/Flatpak on Plasma 6.4 until its GStreamer
    buffer count was lowered, because KWin cut its maximum to 4 [26]. Check our PipeWire buffer
    request.
- **Signing and updates.** The store signs; snapd refreshes automatically.
- **Helpers' source.** The store doesn't host source. Rely on the GitHub release tarball and the
  About → Licenses button (the same reasoning as the Mac App Store build, docs/LICENSES.md §3.4).
- **Effort.** 3–5 days, plus 1–2 weeks for interface requests.

### 2.3 apt (Debian, Ubuntu)
| Option | Fit |
|---|---|
| **Own signed repo** (recommended) | Reuses the CI `.deb`s. Generate metadata with `reprepro` or `apt-ftparchive`, and sign `InRelease` with a repo key held in a GitHub secret. Host on GitHub Pages only while it fits: 1 GB per site, 100 MB per file, ~100 GB/month soft bandwidth [27]. Otherwise use Cloudflare R2 or **Cloudsmith's free OSS tier** (50 GB storage / 200 GB transfer, public repo with an OSI license, credit required; it signs for you [28]). 1–2 days. |
| Launchpad PPA | Builds amd64 and arm64 from a *source* package and publishes the source too (automatic GPL compliance), and x264 is allowed. But builds have no network [10] and the archive has no Node 26/pnpm, so the source package has to carry vendored crates, the pnpm store, Skia and its deps, and probably a Node binary. That means large uploads and a separate upload per Ubuntu series. 4–6 days and continuing upkeep. Only for Ubuntu users who want "a PPA". |
| packagecloud | Proprietary freemium; nothing found about an OSS plan. Skip. |
| OBS | Builds deb and rpm for many distros on aarch64 too [11], but **x264/x265 are blacklisted** [7]. Only works if the helpers are left out. |
| Official Debian/Ubuntu | Not realistic. Debian builds offline from packaged crates: every Rust crate, the npm tree and Skia would need packaging, and the bundled FFmpeg would have to go in favour of the system one. Months of work for a fork of the app. |

Users install it with (deb822):
```bash
sudo install -d /etc/apt/keyrings
curl -fsSL https://<repo>/key.asc | sudo tee /etc/apt/keyrings/simple-capture.asc >/dev/null
printf 'Types: deb\nURIs: https://<repo>/apt\nSuites: stable\nComponents: main\nArchitectures: amd64 arm64\nSigned-By: /etc/apt/keyrings/simple-capture.asc\n' \
  | sudo tee /etc/apt/sources.list.d/simple-capture.sources
sudo apt update && sudo apt install simple-capture
```
Ship one `.deb` per arch built on Ubuntu 24.04 (covers Ubuntu 24.04+ and Debian 13+). Debian 12
and Ubuntu 22.04 would need an ubuntu-22.04 build and an older GStreamer/PipeWire floor; decide
whether they matter (§6).

**Helpers' source.** Put `simple-capture-helper-sources.tar.gz` next to the packages in the repo
(or publish `deb-src`), so the source sits where the binaries are.

### 2.4 dnf/yum (Fedora, RHEL)
- **Own signed RPM repo** (recommended, alongside the APT one).
  - Add `rpm` to the Linux bundles. Tauri's bundler builds RPMs on Ubuntu and signs them with
    `TAURI_SIGNING_RPM_KEY` [29].
  - Run `createrepo_c`, then sign `repodata/repomd.xml` (`.asc`).
  - Users add a `.repo` file with `gpgcheck=1`, `repo_gpgcheck=1` and the key URL.
  - A binary built on Ubuntu 24.04 (glibc 2.39) runs on Fedora 41+.
  - RHEL 9 is out (no webkit2gtk4.1). RHEL 10 depends on EPEL's WebKitGTK (§1.5), so point RHEL
    users to Flathub.
  - 1 day on top of 2.3.
- **Copr.** It would host x86_64 and aarch64 and publish SRPMs, but **not with x264/x265 inside**
  [8]. Possible only as a variant without the GPL helpers that depends on RPM Fusion's `ffmpeg`
  at runtime. That changes behaviour (no `sc-` helpers, different FFmpeg build). Not recommended.
- **Official Fedora / RPM Fusion.** Fedora's Rust guidelines allow a vendored tarball only as a
  "last resort", and reviewers push back on it [30]. x264/x265 are excluded [9]. RPM Fusion
  accepts the codecs but follows similar packaging rules. Not realistic.

### 2.5 AppImage (keep)
- **Build.** Build the x86_64 AppImage on `ubuntu-22.04` (glibc 2.35) if the GStreamer/PipeWire
  floor allows; otherwise state "Ubuntu 24.04-class or newer". Add arm64 on `ubuntu-24.04-arm`
  (22.04-arm also exists [20]).
- **Updates.** Embed update information
  (`gh-releases-zsync|thtmnisamnstr|simple-capture|latest|*x86_64.AppImage.zsync`) and upload the
  `.zsync` file, so AppImageUpdate and Gear Lever can update it. The app itself still never
  fetches anything.
- **Signing.** Optionally `appimagetool --sign`. The SHA256SUMS file and build attestations
  already exist.
- **Effort.** 1–2 days.

## 3. Helpers' corresponding source, per channel
GPL-2.0+ FFmpeg (with x264/x265) and AGPL-3.0+ gifski run as child processes and are never linked
into the app (docs/LICENSES.md). AGPL §13 (network users) doesn't apply because nothing is served
over a network. In every channel, keep the license texts in the package and the About →
Licenses "Source code" button.

| Channel | Where the source is |
|---|---|
| GitHub release, AppImage | The same release page has the tarball |
| Own APT/RPM repo | The tarball (or deb-src/SRPM) in the same repo, kept 3 years |
| Flathub | Manifest URLs, the Sources extension, and our tarball (the extension is x86_64-only, last ~3 builds [21]) |
| Snap Store | Our tarball, linked from the app (the equivalent of a written offer) |
| PPA / Copr / OBS | The platform publishes the source package |

## 4. Desktop file and AppStream metainfo (shared by all channels)
Add `app/linux/<id>.desktop` and `app/linux/<id>.metainfo.xml` to the repo, install them to
`share/applications` and `share/metainfo`, and validate with `appstreamcli validate --strict` and
`desktop-file-validate` in CI. Flathub treats warnings as errors [31].
- **Desktop file.** `Icon=<id>`, `StartupWMClass` matching the GTK app ID, categories
  `AudioVideo;Video;Recorder;`. Install icons as `<id>.png` (256 px or larger, or an SVG [1]).
  Tauri's own icon naming (after the binary [4]) has to be overridden.
- **Metainfo.** It needs:
  - `id` = app ID, `metadata_license` CC0-1.0, `project_license` MIT (the helpers' licenses are
    listed in the description and the app)
  - `developer id=` + `name`, `launchable type="desktop-id"`
  - screenshots: PNG, a `type="default"` one, captions, URLs pinned to a tag (reuse the
    `appstore/screenshots` pipeline at Linux sizes)
  - `releases` with dates, generated from CHANGELOG.md
  - `content_rating type="oars-1.1"` (no content flags)
  - `url` homepage, bugtracker and vcs-browser, and `branding` colours [31]

## 5. Changes to package.yml
1. **helpers**: add `linux-arm64` on `runs-on: ubuntu-24.04-arm`, running
   `build-linux.sh linux` natively. Cache it the same way.
2. **bundle**: add `{os: ubuntu-24.04-arm, plat: linux-arm64, bundles: deb,rpm,appimage}`, and
   change x86_64 to `deb,rpm,appimage`, or move the AppImage to an ubuntu-22.04 job. The Skia
   cache key must include the arch.
3. **New `linux-meta` step.** Install the `<id>.desktop`, metainfo and icons into the bundles, run
   `appstreamcli`/`desktop-file-validate`, and fail if `dpkg-deb -c` / `rpm -qlp` lacks
   `usr/share/applications/<id>.desktop`. Extend the "helpers off the system's names" check to RPM.
4. **Smoke install.** A container job installs the `.deb` on `debian:13` and `ubuntu:24.04` and
   the `.rpm` on `fedora:latest`, then runs `simple-capture --self-test` under `xvfb-run`.
5. **On a published release** (`release: published`, not the draft):
   - `publish-repos`: sign and push APT/RPM metadata, using `REPO_GPG_KEY` and Cloudsmith or R2
     secrets.
   - `snap`: build both arches and upload to the edge channel with `SNAPCRAFT_STORE_CREDENTIALS`.
   - `flathub`: open a PR on `flathub/<id>` bumping the tag and the generated source JSONs.
   - AppImage `.zsync` uploads.
6. Keep pull-request runs free of secrets, as the macOS signing job is today.

## 6. Order and effort
| Step | What | Effort |
|---|---|---|
| 0 | ID decision; desktop file + metainfo + icons; `.deb` depends; arm64 helpers and bundles in CI | 2–3 days |
| 1 | **Flathub**: offline manifest (vendor kit, Skia deps, FFmpeg/gifski modules), lint, submit | 6–10 days + review |
| 1b (parallel, cheap) | Own signed APT + RPM repo from the CI packages | 2 days |
| 2 | AppImage: arm64, update info, glibc decision | 1–2 days |
| 3 | Snap Store (strict, core24), interface auto-connect requests | 3–5 days + requests |
| later | Launchpad PPA, only if asked for | 4–6 days |
| no | Copr/OBS (codec bans), official Debian/Fedora (unbundling) | — |

Flathub goes first because it reaches every distro, RHEL included, and on both arches. Its
sandbox fits an app that already uses portals, it lets us host x264/x265 and gifski, and it
handles updates and signing. The self-hosted repos are cheap because they reuse what CI already
builds, so they can run alongside it. Snap comes next because Ubuntu's own store is the Snap
Store and Ubuntu has no Flatpak by default.

## 7. Decisions for the owner
1. Linux app ID: keep `com.simplecapture.desktop` on host packages plus a separate Flatpak ID, or
   one new Linux ID. If new, `io.github.thtmnisamnstr.SimpleCapture` or a domain ID (do you
   control simplecapture.com?).
2. Oldest supported distro: Ubuntu 24.04/Debian 13 only, or also 22.04/Debian 12 (an older build
   machine and a lower GStreamer floor)?
3. Repo hosting: Cloudsmith OSS (needs an attribution credit), Cloudflare R2, or GitHub Pages
   (size limits). This includes a new repo-signing GPG key you generate and keep offline.
4. Flathub camera access: defend `--device=all`, or add a Camera-portal path?
5. Snap Store account, the snap name `simple-capture`, and filing the interface requests.
6. Bundle GStreamer into the AppImage (bigger, "ugly" plugin licensing), or require the host's?

## Sources
1. https://docs.flathub.org/docs/for-app-authors/requirements
2. https://docs.flathub.org/docs/for-app-authors/linter
3. https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.host.portal.Registry.html
4. https://github.com/tauri-apps/tauri/blob/dev/crates/tauri-bundler/src/bundle/linux/freedesktop/mod.rs
5. https://docs.flathub.org/docs/for-app-authors/verification
6. https://github.com/flathub/com.obsproject.Studio/blob/master/com.obsproject.Studio.json
7. https://en.opensuse.org/Application_Black_List
8. https://lists.fedoraproject.org/pipermail/legal/2013-November/002281.html (Copr: same licensing bar as Fedora)
9. https://docs.fedoraproject.org/en-US/quick-docs/fedora-and-software-patents/ ; https://www.fedoraproject.org/wiki/ForbiddenItems
10. https://bugs.launchpad.net/launchpad/+bug/1812282 (no network in Launchpad builds)
11. https://openbuildservice.org/help/manuals/obs-user-guide/art.obs.bg ; https://github.com/openSUSE/obs-service-cargo_vendor
12. https://v2.tauri.app/distribute/flatpak/
13. https://github.com/flatpak/flatpak-builder-tools/blob/master/node/README.md
14. https://github.com/rust-skia/rust-skia/blob/master/skia-bindings/README.md
15. https://v2.tauri.app/blog/tauri-2-0-0-alpha-3/ (webkit2gtk-4.1)
16. https://bugzilla.redhat.com/show_bug.cgi?id=2232124
17. https://packages.fedoraproject.org/pkgs/libayatana-appindicator/libayatana-appindicator-gtk3/
18. https://docs.rs/crate/tauri/2.4.0/source/src/tray/mod.rs (TrayIconEvent: "Linux: Unsupported")
19. https://v2.tauri.app/distribute/appimage/
20. https://github.blog/changelog/2025-08-07-arm64-hosted-runners-for-public-repositories-are-now-generally-available/
21. https://docs.flathub.org/docs/for-users/finding-sources
22. https://v2.tauri.app/distribute/snapcraft/
23. https://snapcraft.io/docs/reference/administration/reviewing-classic-confinement-snaps/
24. https://snapcraft.io/docs/reference/interfaces/
25. https://forum.snapcraft.io/t/pipewire-doesnt-work-in-snaps/40235
26. https://discuss.kde.org/t/cant-record-screen-on-plasma-when-using-flatpak-or-snap/46782
27. https://docs.github.com/en/pages/getting-started-with-github-pages/github-pages-limits
28. https://docs.cloudsmith.com/open-source-hosting-policy ; https://cloudsmith.com/pricing
29. https://v2.tauri.app/distribute/rpm/
30. https://docs.fedoraproject.org/en-US/packaging-guidelines/Rust/
31. https://docs.flathub.org/docs/for-app-authors/metainfo-guidelines
