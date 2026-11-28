# KDE Plasma testing plan

Plan, 2026-11-27 (researched by an agent, checked against the branch). The goal is to test the Linux build on KDE Plasma (Wayland) as well as GNOME,
with no human where possible and with a short hands-on checklist where not. Today's harness
(`tools/linux-dev/headless.sh`, `e2e.sh`) runs a private headless **sway** session whose
`xdg-desktop-portal-wlr` picks the only output by itself (`chooser_type=none`). That tests our
pipeline, not KDE's or GNOME's portal. Sources are listed at the end and cited as [n].

## 0. What is different on KDE

| | GNOME 50 (Ubuntu 26.04) | Plasma 6.6 (Kubuntu 26.04) / 6.8 (Fedora 45) |
|---|---|---|
| ScreenCast dialog | xdg-desktop-portal-gnome (GTK) | xdg-desktop-portal-kde (Qt), with an "Allow restoring on future sessions" checkbox |
| Restore token (persist_mode 2) | Yes | Yes. If the user unticks the box, no token should come back (bug 517454, fixed May 2026 [1]). |
| Pre-authorising without a dialog | No supported way | Only for **RemoteDesktop** (`kde-authorized` table, Plasma 6.3+) [2]. KDE declined a ScreenCast override [3]. |
| GlobalShortcuts portal | GNOME 48+ | Yes, backed by KGlobalAccel. Component name = app ID since kde#492992 [4]. |
| Tray | AppIndicator extension needed | Native SNI host. Tauri still gets no click events on Linux [5], so the menu is the interface everywhere. |
| X11 session | Gone | Plasma 6.7 is the last with X11; 6.8 is Wayland-only [6] |

Every portal on the host depends on the app ID. The app calls `host.portal.Registry.Register`
with `io.github.thtmnisamnstr.SimpleCapture`, and the portal only accepts an ID that matches an
installed `.desktop` file basename [7]. Tauri installs `simple-capture.desktop` instead (see
LINUX_DISTRIBUTION.md §1.1). **Install the matching desktop file first, or every KDE result about
restore tokens and shortcuts is unreliable.** In headless runs, install it into the private
`XDG_DATA_HOME`.

## 1. Automated, headless

### 1.1 A private Plasma-ish session (same pattern as headless.sh)
On Ubuntu 26.04 (arm64 is in the main archive [8]), install these without the full desktop:
```bash
sudo apt install --no-install-recommends kwin-wayland xdg-desktop-portal-kde kglobalacceld \
  qdbus-qt6 pipewire wireplumber dbus-daemon
```
Inside a private bus, runtime dir and config (copy the top of `headless.sh`), run:
```bash
export XDG_CURRENT_DESKTOP=KDE XDG_SESSION_TYPE=wayland KDE_FULL_SESSION=true
printf '[preferred]\ndefault=kde\n' > "$XDG_CONFIG_HOME/xdg-desktop-portal/kde-portals.conf"
kwin_wayland --virtual --width 1280 --height 720 --socket sc-kwin --no-lockscreen \
  > "$BASE/kwin.log" 2>&1 &      # options from kwin's main_wayland.cpp [9]
export WAYLAND_DISPLAY=sc-kwin
/usr/libexec/kglobalacceld &      # path differs by distro; KWin may start it itself
/usr/libexec/xdg-desktop-portal-kde & /usr/libexec/xdg-desktop-portal -r &
```
Then PipeWire, WirePlumber (hardware monitors off) and the null sink, exactly as `headless.sh`
does. Make this `tools/linux-dev/headless-kde.sh` and let `e2e.sh` pick a session with
`SC_E2E_SESSION=sway|kde|gnome`.

Notes:
- `xwayland-run`'s wlheadless already drives `kwin_wayland --virtual` this way [10].
- Without a GPU, KWin renders in software; check that its screencast still hands out memfd
  buffers there.
- A recorder broke under Plasma 6.4 when KWin lowered its maximum buffer count to 4 [11].
  Assert our stream negotiates within that.

### 1.2 Getting past the KDE ScreenCast dialog
There is no switch to skip it. Options, best first:
1. **Seeded permission store (recommended).** The portal keeps restore data in the permission
   store (`$XDG_DATA_HOME/flatpak/db/screencast`), keyed by token. Do one run where the dialog
   is accepted (option 2, or by hand in a nested session) with "restore" ticked. Then save that
   db file plus the app's token file as a fixture. Each test starts from a fresh copy, so the
   single-use token [12] is still unused, and KWin's virtual output name stays the same. The
   portal restores with no dialog, which tests our real restore path on KDE's portal. Re-seed
   when Plasma changes the format. **Verify first**: the virtual output name is stable, and the
   restore data doesn't depend on window UUIDs (record a monitor only).
2. **Click it with injected input.** Inject Enter or Tab into the virtual KWin through KWin's EIS
   D-Bus interface, as `kwin-mcp` does in `kwin_wayland --virtual` sessions [13]. Alternatively,
   pre-authorise a small test driver for RemoteDesktop with
   `flatpak permission-set kde-authorized remote-desktop <driver-app-id> yes` [2] and send keys
   through libei. The dialog's own accessibility tree also works with KDE's
   selenium-webdriver-at-spi (Appium) [14]. Use this to create fixtures and to test the dialog
   itself (single screen, window list, the restore checkbox).
3. **Keep sway/wlr** for the pure pipeline tests (fast, no dialog). That's what we have now.

### 1.3 Global shortcuts through the portal, no human
- The first `BindShortcuts` shows KDE's dialog. Accept it once (input injection as in 1.2).
- Keep `$XDG_CONFIG_HOME/kglobalshortcutsrc` as a fixture next to the permission store.
- After that, trigger our shortcut through KGlobalAccel's D-Bus API [15]:
  ```bash
  qdbus6 org.kde.kglobalaccel | grep -i simplecapture        # e.g. /component/io_github_thtmnisamnstr_SimpleCapture
  qdbus6 org.kde.kglobalaccel /component/io_github_thtmnisamnstr_SimpleCapture \
    org.kde.kglobalaccel.Component.shortcutNames             # the IDs we bound
  qdbus6 org.kde.kglobalaccel /component/io_github_thtmnisamnstr_SimpleCapture \
    org.kde.kglobalaccel.Component.invokeShortcut record
  ```
  The portal then emits `Activated`/`Deactivated` to the app. Assert in the driver log that a
  recording started.
- Also inject the real key chord through EIS once, so the keyboard path is tested too.
  `invokeShortcut` skips key handling, and it has been seen to do nothing silently [15].
- Version traps: xdg-desktop-portal-kde 6.7.4 skipped `BindShortcuts` for one app (kde#523063)
  [16]; 6.4.4 fixed "return shortcuts when the dialog is accepted" [4]. Log the plasma and portal
  versions in every run.

### 1.4 GNOME, for comparison

**Done on GNOME 50 (2026-11-27):** `tools/linux-dev/approve-portal.py` approves the share and
shortcut dialogs through AT-SPI, and `tools/linux-dev/press-keys.py` presses real key chords
through Mutter's RemoteDesktop D-Bus API. Both ran on the owner's real desktop: the share dialog,
a restore with no dialog, and Alt+Shift+R starting and stopping a recording through the
GlobalShortcuts portal. The headless-GNOME route below is still to do.

- **Session.** `mutter --wayland --headless --virtual-monitor 1280x720` exists for exactly this
  [17]. gnome-remote-desktop's tests run it with `--no-x11` [18]. Portals and GlobalShortcuts
  need the shell, so try `gnome-shell --headless --wayland --virtual-monitor 1280x720` in a
  private bus (verify on GNOME 50; the nested mode is now `--devkit` [19]).
- **Dialog.** xdg-desktop-portal-gnome always shows a dialog [20]. There is no auto-accept
  setting, and a patched test-only portal would test a portal nobody ships. Use the same three
  routes as on KDE:
  - The seeded permission-store fixture (the frontend keeps the restore data, so the approach is
    identical; check that mutter's virtual monitor identity is stable).
  - Input injection through Mutter's `org.gnome.Mutter.RemoteDesktop` D-Bus API. It's private,
    and `waydriver` (Rust, Apache-2.0) does this with headless mutter, AT-SPI and PipeWire [21].
  - AT-SPI via dogtail/gnome-ponytail-daemon [22].
- **Shortcuts.** GNOME has no `invokeShortcut` equivalent, so inject the real key chord through
  Mutter RemoteDesktop.
- **Comparison.** KDE is easier to automate. It has a public D-Bus trigger for shortcuts, a
  public EIS endpoint, and an official pre-authorisation table (for RemoteDesktop only).

### 1.5 GitHub Actions
- Run the KDE and GNOME sessions in containers on `ubuntu-24.04` and `ubuntu-24.04-arm` (free for
  public repos). Ubuntu 24.04's own packages are Plasma 5.27 / GNOME 46, so use `fedora:latest`
  (Plasma 6.8, GNOME 50) or a Kubuntu 26.04 image. GitHub's `ubuntu-26.04` runner labels are
  still in preview.
- Jobs:
  - `e2e (sway)`: today's flows, on every PR.
  - `e2e (kde)` and `e2e (gnome)`: restore-token and shortcut flows from fixtures, nightly and
    on `main`.
- No GPU, no camera and no real audio, so these cover portal, shortcut, encode and export logic
  only.

## 2. Real KDE desktop on the Asahi laptop (aarch64, Ubuntu 26.04, 7 GB RAM)
| Option | Disturbs GNOME? | Fidelity | Cost | Notes |
|---|---|---|---|---|
| **A. Nested Plasma in a window** | No (private bus) | Medium | Minutes | `dbus-run-session startplasma-wayland` from the GNOME session opens Plasma in a window (KWin's Wayland-windowed backend). It uses the host's PipeWire, so the real mic and camera. Global shortcuts reach it only while its window has focus. **Best for day-to-day dialog, tray and shortcut checks.** |
| **B. Plasma as a second login session** | A little | High | ~1 GB, 30 min | `sudo apt install --no-install-recommends plasma-workspace kwin-wayland xdg-desktop-portal-kde plasma-nm` and keep **gdm3** when asked about the display manager. Choose "Plasma (Wayland)" at login. Portal config is per desktop (`kde-portals.conf` vs `gnome-portals.conf`), so GNOME keeps its portal. Risks: KDE apps in GNOME's menus, kwallet prompts, Qt theme env vars. Kubuntu 26.04 ships Plasma 6.6 [8]; `plasma-session-x11` is still in the archive (unsupported) for a cheap X11 pass. |
| **C. Fedora Asahi Remix as a second OS** | No (own partition) | Highest on this hardware | Reboot, ~30 GB | The Asahi installer can add another OS beside the current one. Fedora Asahi Remix 45's flagship desktop is Plasma 6.8 [23], with the native GPU and the real camera and mic. **Best for release sign-off on KDE.** |
| D. QEMU/KVM VM (Kubuntu or Fedora KDE, aarch64) | No | Low–medium | 3 GB of 7 GB RAM | KVM works on Asahi. Use a 4K-page guest kernel (muvm does the same [24]). virtio-gpu with virgl is slow, and a 16K host with 4K guest has known virtio-gpu alignment issues [25]. The built-in camera can't be passed through; a USB webcam can. Don't run it alongside a cargo build. |
| E. Second x86_64 machine with Kubuntu 26.04 | No | High | Hardware | The only way to test VA-API on Intel/AMD and NVENC on KDE. Use it if one exists. |
| F. GitHub Actions | — | Automated only | Free | §1.5 |

Recommendation:
- A for quick checks.
- B or C before each release. Prefer C if disk space allows; it doesn't touch the GNOME install.
- X11 on KDE: one pass in B's X11 session at most. KDE's ScreenCast portal works only on Wayland
  as far as we know (verify), so expect the app to say screen capture is unavailable there.
  Global shortcuts should work through the X11 grab. Drop it once Plasma 6.8 is common.

Announce live runs and hold the `scripts/lib/live.sh` lock, as on the Mac (CLAUDE.md).

## 3. KDE manual checklist (add to docs/MANUAL_TEST.md §13 after LX6)
Plasma 6.6+ on Wayland. Note the Plasma, KWin and xdg-desktop-portal-kde versions
(`plasmashell --version`; `dpkg -l | grep -E 'kwin|portal-kde'`) in the results log.

| # | Step | Expected |
|---|---|---|
| KD1 | Install the .deb (or run the AppImage); log in to Plasma | The app starts; tray icon in the system tray; `~/.local/share/applications` or `/usr/share/applications` has `<app-id>.desktop`; log says `portal: registered` |
| KD2 | Panel → Choose Screen or Window… | KDE's share dialog lists screens and windows, our windows excluded or harmless; "Allow restoring" checkbox present; after choosing, the panel names the pick |
| KD3 | Pick a screen with "Allow restoring" ticked; quit; relaunch; Record | No dialog; it records the same screen (restore token reused). Do it twice: the second relaunch must still restore (we saved the new single-use token) |
| KD4 | KD3 with the box unticked | The dialog appears again on the next launch; nothing crashes |
| KD5 | Record 20 s with mic and system audio playing | Our windows hide while recording; the editor opens; sound in step; Plasma's audio applet shows the app recording |
| KD6 | Camera on (a webcam with YUYV) | Records; an MJPEG-only camera says so; unplugging shows "Camera disconnected" and recording goes on |
| KD7 | Tray: left-click, right-click, middle-click, while idle and while recording | The menu opens (no click actions are expected: Tauri has no Linux tray events); Stop works from the menu while recording |
| KD8 | First launch with shortcuts enabled | KDE's own dialog lists our shortcuts (e.g. start/stop) to confirm; after accepting, System Settings → Shortcuts shows them under simple-capture |
| KD9 | Press the start/stop shortcut from another app's window | Recording starts and stops; works with our windows hidden |
| KD10 | Change the shortcut in System Settings → Shortcuts; press it | The new chord works and the old one does nothing |
| KD11 | Deny the shortcut dialog; relaunch | The app still works; it explains shortcuts are off; no repeated dialog storm |
| KD12 | Record a window, then resize it while recording | Letterboxed, not stretched; the window list in the dialog shows normal windows |
| KD13 | Display scale 125 % or 150 %; record a region | The region matches what was drawn; the panel and bar are sharp |
| KD14 | Self-test (About → Diagnostics) | Lists the encoders, portal versions and session type `wayland`/`KDE` |
| KD15 | Export MP4 and GIF; Show; Move to Trash | Files play in Haruna or VLC; Show opens Dolphin at the file; the project is in Trash |
| KD16 | (B only) X11 session: start, Choose Screen…, a shortcut | A clear "screen capture needs Wayland on Plasma" message (or it works: record which); the shortcut fires |

Run the GNOME checklist LX1–LX6 on the same build and compare. Report the failures with the last
200 lines of `~/.local/share/<app-id>/logs/simple-capture-app.log`.

## 4. Order of work
1. Fix the `.desktop` name so Register matches (prerequisite).
2. `headless-kde.sh` + the dialog-acceptance driver (1.2.2), then build fixtures (1.2.1).
3. Restore-token and shortcut flows on KDE (`invokeShortcut`) and GNOME (key injection).
4. Containers in CI (nightly).
5. Manual pass in A; release pass in C (or B).

## Sources
1. https://bugs.kde.org/show_bug.cgi?id=517454
2. https://develop.kde.org/docs/administration/portal-permissions/ ; https://invent.kde.org/plasma/xdg-desktop-portal-kde/-/commit/2d4a4dfec07e478ab57fb70a1f531ba496382467
3. https://bugs.kde.org/show_bug.cgi?id=506237
4. https://build.opensuse.org/projects/openSUSE:Factory/packages/xdg-desktop-portal-kde6/files/xdg-desktop-portal-kde6.changes?expand=0 ; https://bugs.kde.org/show_bug.cgi?id=483640
5. https://docs.rs/crate/tauri/2.4.0/source/src/tray/mod.rs
6. https://blogs.kde.org/2025/11/26/going-all-in-on-a-wayland-future/ ; https://planet.kde.org/david-edmundson-2026-06-02-ex-11-prepping-for-plasmas-last-x11-supported-release/
7. https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.host.portal.Registry.html
8. https://kubuntu.org/news/kubuntu-26-04-release-notes/ ; https://discourse.ubuntu.com/t/ubuntu-on-arm-summer-26-update/84872
9. https://invent.kde.org/plasma/kwin/-/blob/master/src/main_wayland.cpp
10. https://gitlab.freedesktop.org/ofourdan/xwayland-run/-/merge_requests/6
11. https://discuss.kde.org/t/cant-record-screen-on-plasma-when-using-flatpak-or-snap/46782
12. https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html
13. https://discuss.kde.org/t/kwin-mcp-mcp-server-for-gui-automation-using-kwins-eis-interface-and-virtual-sessions/44678
14. https://develop.kde.org/docs/apps/tests/appium ; https://community.kde.org/Selenium
15. https://discuss.kde.org/t/open-task-switcher-from-cli/28116 ; https://bugs.kde.org/show_bug.cgi?id=432214
16. https://www.1password.community/developers-69/linux-kde-wayland-global-shortcut-ctrl-shift-space-does-not-work-after-1password-restarts-25707
17. https://gitlab.gnome.org/GNOME/mutter/-/merge_requests/1698
18. https://lists.gnome.org/archives/commits-list/2022-May/msg08259.html
19. https://discourse.gnome.org/t/cant-run-dbus-run-session-gnome-shell-devkit-wayland-gnome-shell-50-3/38210
20. https://discourse.gnome.org/t/security-for-mutter-screencast/35009
21. https://docs.rs/crate/waydriver/0.3.3/source/README.md
22. https://fedoraproject.org/wiki/Wayland_features
23. https://fedoramagazine.org/fedora-asahi-remix-45-is-now-available/
24. https://asahilinux.org/docs/sw/broken-software/
25. https://www.mail-archive.com/qemu-devel@nongnu.org/msg982302.html ; https://patchew.org/QEMU/20251203040754.94487-1-j@getutm.app/20251203040754.94487-3-j@getutm.app
