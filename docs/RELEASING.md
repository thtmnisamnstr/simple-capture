# Releasing

## macOS signing and notarization

Releases are signed with the owner's **Developer ID Application** certificate and notarized, so
macOS opens them without warnings and keeps Screen Recording, Camera and Microphone grants across
updates. Without the secrets below, the package workflow still builds an ad hoc signed DMG.

The DMG and the Mac App Store build are the same sandboxed app with the LGPL helpers and no
gifski (DECISIONS ADR-058); only the signing differs. `scripts/macos-sign.sh` signs both: the
helpers with `app/macos/helper.entitlements.plist` (App Sandbox, inherited), then the app with
the hardened runtime and `app/macos/entitlements.plist` (the DMG) or
`app/macos/entitlements.appstore.plist` (the store: the same sandbox set plus the application
and team identifiers). `scripts/mas-check.sh --entitlements`, run in CI, keeps the two files in
step.

### On this Mac (local builds)

1. The certificate and its private key are in the login keychain, with Apple's **Developer ID -
   G2** intermediate (https://www.apple.com/certificateauthority/). Check:
   `security find-identity -v -p codesigning` lists `Developer ID Application: …`.
2. Notarization credentials, stored once in the keychain (it asks for an app-specific password
   from account.apple.com → Sign-In and Security):
   `xcrun notarytool store-credentials "simple-capture" --apple-id <Apple ID> --team-id <Team ID>`
3. `scripts/local-bundle.sh` (or `just bundle`, which adds `--dmg`) then builds the sandboxed
   app as the DMG ships it: `scripts/ci/notices.sh --macos` (when cargo-about is installed),
   `scripts/stage-helpers.sh` (the LGPL `sc-ffmpeg` and `sc-ffprobe` from
   `third_party/out/macos-arm64`, built by `third_party/ffmpeg/build-macos.sh arm64` or
   `just helpers`; it refuses a GPL build there, such as one made before ADR-058), and `tauri build` with `app/tauri.bundle.conf.json` and
   `app/tauri.bundle.macos.conf.json`. It signs, notarizes and staples the app
   (`scripts/macos-sign.sh`); `--dmg` also builds the installer DMG and notarizes that too.
   Without a Developer ID it signs with the self-signed "Simple Capture Local Signing"
   certificate, or ad hoc. `scripts/mas-check.sh --dmg "<app>"` checks the result (`--adhoc`
   for a local or ad hoc signature): signatures and hardened runtime, each executable's exact
   entitlements, Info.plist, LGPL-only helpers and the license set. The app runs sandboxed, so
   its settings and logs are in `~/Library/Containers/com.simplecapture.desktop/`.

### The DMG

`scripts/make-dmg.sh` packs the app with dmgbuild (pinned, installed into `target/dmgbuild-venv`),
which writes the window layout into the image without scripting Finder. The window's look is
`assets/brand/dmg-background.svg`; after changing it, run `scripts/render-dmg-background.sh`
(needs Chrome) and commit the TIFF it writes. Labels sit on a mid-green that keeps Finder's
white (dark mode) and black (light mode) icon labels readable.

### On GitHub (release builds)

Set these repository secrets yourself; their values never go into the repository or a chat.

| Secret | What |
|---|---|
| `APPLE_CERTIFICATE_P12` | The certificate and private key, exported from Keychain Access (My Certificates → right-click → Export, `.p12` with a password), base64-encoded |
| `APPLE_CERTIFICATE_PASSWORD` | The `.p12` export password |
| `APPLE_API_KEY_P8` | An App Store Connect API key (App Store Connect → Users and Access → Integrations → App Store Connect API → Team Keys → +, role Developer), the downloaded `AuthKey_<id>.p8` file's contents |
| `APPLE_API_KEY_ID` | That key's Key ID |
| `APPLE_API_ISSUER` | The Issuer ID shown above the keys list |

`scripts/set-apple-secrets.sh devid.p12 AuthKey_XXXXXXXXXX.p8` sets all five: it checks the
`.p12` opens and holds the Developer ID, asks for the password and the two IDs without echoing
the password, and sends each value straight to `gh secret set`. Delete the `.p12` afterwards.

The `bundle (macos-26)` job then imports the certificate into a temporary keychain, signs the
app and its helpers, notarizes and staples it, checks it with `scripts/mas-check.sh --dmg`,
builds the DMG, and signs, notarizes and staples that too. Its licenses are checked with
`scripts/ci/notices.sh --check-macos app/gen/licenses`. It signs only for `v*` tags and manual runs; pull-request builds are ad hoc signed, so
pull-request code never runs with the identity. A manual run signs whatever branch it is started
on, so start manual runs from `main` or a tag.

## Windows

The one installer is the NSIS setup `.exe`, a per-user install (no `.msi`). It is not code
signed, so SmartScreen warns until a signed build earns reputation. Signing would go through
Azure Trusted Signing (Artifact Signing) or an OV certificate.

## Homebrew

The cask lives in its own tap, `thtmnisamnstr/homebrew-simple-capture` (`Casks/simple-capture.rb`):
`brew install --cask thtmnisamnstr/simple-capture/simple-capture`. The tap's hourly `bump.yml` reads the
latest published release and updates the cask's version and SHA-256, so a release reaches
Homebrew within about an hour of Publish. To do it now:
`gh workflow run bump.yml -R thtmnisamnstr/homebrew-simple-capture`. The cask's URL uses the DMG's
release name (`simple-capture_<version>_aarch64.dmg`), so keep that name. One-time: the tap
repository (`Casks/simple-capture.rb`, `bump.yml`, `tests.yml`) is created right after v0.1.0
is published, because a draft's files can't be downloaded; the `brew` command works from then
on.

## Mac App Store

The store build (DECISIONS ADR-057, ADR-058) is built from the same tag as the GitHub release by
`.github/workflows/app-store.yml`: the same app as the DMG (sandboxed, LGPL helpers, no gifski)
plus `app/tauri.appstore.conf.json`, signed with Apple Distribution,
`app/macos/entitlements.appstore.plist` and a Mac App Store provisioning profile, and packed as
a `.pkg` signed with Mac Installer Distribution. The same workflow keeps the store listing in step with
`appstore/`. Only the steps below need you; key files, passwords and your contact details never
go into the repository or a chat.

### One-time setup

1. **Agreements.** App Store Connect → Business: accept anything pending. A free app needs only
   the Program License Agreement; the Paid Apps agreement, tax and banking only matter for a
   price.
2. **EU Digital Services Act.** App Store Connect → Business: declare trader or non-trader
   status. Without it the app is not offered in the EU; a trader's address, phone and email
   appear on the EU product page.
3. **API keys.** App Store Connect → Users and Access → Integrations → App Store Connect API →
   Team Keys → +, twice: one with access **App Manager** (for CI) and one with **Admin** (for
   step 4 only). Download both `AuthKey_<id>.p8` files (possible only once) and note the Issuer
   ID above the list. The Developer key (`APPLE_API_KEY_*`) stays for notarization: it can
   upload builds but not submit them.
4. **Signing and secrets.** In the checkout, run
   `scripts/appstore-setup.sh AuthKey_<admin-id>.p8 AuthKey_<appmanager-id>.p8`, Admin key
   first (`--dry-run` shows the requests without sending them). It asks for the Key IDs and the
   Issuer ID and makes the `.p12` password itself. With the Admin key it registers the bundle ID
   `com.simplecapture.desktop` for macOS, creates the Apple Distribution and Mac Installer
   Distribution certificates and the Mac App Store profile, and sends these secrets straight to
   `gh secret set`. It prints only their names, and keeps no key, certificate or `.p12` file:

   | Secret | What |
   |---|---|
   | `ASC_KEY_ID`, `ASC_ISSUER_ID`, `ASC_KEY_P8` | The CI key (the App Manager key): its Key ID, the Issuer ID, the `.p8` contents |
   | `ASC_APP_ID` | The app's Apple ID, looked up by bundle ID once the app record exists |
   | `MAS_APP_CERT_P12`, `MAS_INSTALLER_CERT_P12` | The two identities as `.p12`, base64-encoded |
   | `MAS_CERT_PASSWORD` | Their `.p12` password |
   | `MAS_PROVISION_PROFILE` | The Mac App Store profile, base64-encoded |
   | `ASC_REVIEW_CONTACT_FIRST_NAME`, `_LAST_NAME`, `_PHONE`, `_EMAIL` | Optional, set by you (step 8) |

   The first time there is no app record yet, so the script stops after the secrets and asks
   for one.
5. **The app record** (no API can create it). Apps → + → New App:

   | Field | Value |
   |---|---|
   | Platforms | macOS |
   | Name | simple-capture |
   | Primary Language | English (U.S.) |
   | Bundle ID | com.simplecapture.desktop (registered by step 4) |
   | SKU | simple-capture-mac |
   | User Access | Full Access |

   Then run step 4's command again: it keeps the certificates, finds the app and sets
   `ASC_APP_ID`.
6. **The keys afterwards.** CI now holds the App Manager key. Revoke the Admin key (Team Keys →
   the key → Revoke) and delete its `.p8`. Keep the App Manager `.p8` in a password manager for
   renewals (below), or delete it as well and make a new App Manager key when you renew. Never
   revoke the key in `ASC_KEY_ID`: every app-store run would fail with 401 (NOT_AUTHORIZED).
   Given only one key, the script stores that key for CI; if that was the Admin key, run the
   script again with both keys before revoking it.
7. **App Privacy.** App Privacy → Get Started → "No, we do not collect data from this app" →
   Save → Publish ("Data Not Collected"). The Privacy Policy URL is set by the workflow from
   `appstore/metadata/en-US/privacy_url.txt`:
   `https://github.com/thtmnisamnstr/simple-capture/blob/main/docs/PRIVACY.md`, the page the
   About window links to. It resolves once `docs/PRIVACY.md` is on `main`, so merge before the
   first submission.
8. **App Review contact.** Either store it once as secrets, which every run sends to the version
   it prepares: `gh secret set ASC_REVIEW_CONTACT_FIRST_NAME` (and `_LAST_NAME`, `_PHONE`,
   `_EMAIL`; each asks for its value), or type your name, phone and email on the version page
   under App Review Information, and check them again on later versions. The review notes and
   "sign-in not required" come from `appstore/review.json`.
9. **Store listing.** It lives in `appstore/` (see `appstore/README.md`), and every app-store run
   syncs it from there: name, subtitle, description, keywords, promotional text, the support,
   marketing and privacy policy URLs, the screenshots in `appstore/screenshots/en-US/`
   (2880×1800 PNGs, in file-name order), categories (Photo & Video, then Productivity), age
   rating, copyright and review notes. It also sets the version to release automatically once
   approved. Changes made to these in App Store Connect are overwritten by the next run, so edit
   the files and check them with `python3 -I scripts/asc/asc.py check-metadata` and
   `python3 -I appstore/check.py`. The first run also makes the app free and available in
   every territory; after that, price and availability are left as they are, so change them in
   App Store Connect. `appstore/screenshots/tools/make.sh` renders the screenshots again from
   the UI (see `appstore/README.md`). On the web you do only what has no API: steps 1, 2, 3, 5
   and 7, and step 8 if you don't use the secrets.
10. **First TestFlight check.** After tagging (Cutting a version, step 3),
    `gh workflow run app-store.yml -f tag=v0.1.0 -f submit=false` builds, uploads and prepares
    the version without submitting it. Pass the tag: the release is still a draft, and with no
    tag the workflow takes the latest *published* release. Add yourself to an internal group
    under TestFlight → Internal Testing, install the build with the TestFlight app, and go
    through docs/MANUAL_TEST.md §10h (permissions, display, window and iPhone recordings,
    exports to a chosen folder across a relaunch, focus, the About links). Then publish the
    release, which builds again and submits.

### Each release

1. Publishing the GitHub release (Cutting a version, step 4) starts `app-store.yml` on the tag;
   a prerelease is skipped. It builds and uploads the store build, waits until App Store
   Connect has processed it, creates the App Store version (the same version string) or reuses
   the open one, attaches the build, syncs the listing, sets What's New and submits for review.
   Follow it under Actions → app-store, or with `gh run watch`.
2. **What's New** is the version's section of `CHANGELOG.md` (under its `## <version>` heading,
   such as `## 0.2.0`). `CHANGELOG.md` covers every platform, so the headings, bullets and
   paragraphs about another one are left out: those naming Linux, Android, iOS, gifski, x264 or
   x265, or Windows the operating system ("on Windows", "Windows:", "Windows installer", a
   `### Windows` heading; app windows, as in "Windows from another Space", stay), and headings
   left empty. The run logs how many it left out. The rest becomes plain text: headings become
   plain lines, `- ` bullets become `• `, links keep only their text, and `**` and backticks go.
   The app's first store version (normally 0.1.0) sends none, since the store shows What's New only
   for updates. For every later version, `appstore/check.py` checks that text before anything is
   built (in the plan job, and again in `asc.py preflight`), and the run fails if the section is
   missing or only about other platforms, is over 4000 characters, or contains beta, x264, x265,
   gifski, a price (price, prices, pricing, a `$` amount) or the word free (crash-free,
   free-form and free disk space are fine), or says "no network" without "connections". The
   workflow reads `CHANGELOG.md` at the tag, so check the section before tagging (Cutting a
   version, step 2).
3. App Review replies by email and in App Store Connect; answer there. Once approved, the
   version is released automatically: every run sets "Automatically release this version", so
   a manual-release choice made in App Store Connect is undone.
4. To run it again, use Actions → app-store → Run workflow and fill in **Release tag**, or
   `gh workflow run app-store.yml -f tag=v<version>`. `-f submit=false` uploads and prepares
   without submitting; `-f replace_waiting=true` cancels a submission still waiting for review
   so this build replaces it. Re-running a failed run is safe: each upload asks App Store
   Connect for the next unused build number (`scripts/asc/asc.py next-build`). If a manual run
   already submitted the version, the run that Publish starts doesn't replace that build: it
   stops because the version is in review.

### Renewing the signing certificates

The Apple Distribution and Mac Installer Distribution certificates last a year, and the profile
ends with them. Once they lapse (or are revoked), app-store fails, usually at "Signing keychain
and profile", and asks for `scripts/appstore-setup.sh --renew-certs`. Apps already on the store
are not affected.

1. Make a new temporary Admin key (One-time setup, step 3).
2. `scripts/appstore-setup.sh --renew-certs AuthKey_<new-admin-id>.p8 AuthKey_<appmanager-id>.p8`.
   It makes new certificates and a new profile, replaces the `MAS_*` secrets, and stores the
   second key for CI again. Without the App Manager `.p8`, make a new App Manager key, pass
   that, and revoke the old one only after this run.
3. Revoke the temporary Admin key and delete its `.p8`.

Apple allows only a few active certificates of each type per team. If the script says the
limit is reached, revoke one you no longer use (developer.apple.com → Certificates, Identifiers
& Profiles → Certificates) and run it again; otherwise old certificates can simply expire.

### Building it on this Mac

`scripts/mas-build.sh --adhoc` builds the store build and checks it without certificates
(signed ad hoc with the store entitlements, the `.pkg` unsigned). It runs the same steps as
`scripts/local-bundle.sh` (`scripts/ci/notices.sh --macos`, `scripts/stage-helpers.sh` with the
LGPL `sc-ffmpeg` and `sc-ffprobe` from `third_party/out/macos-arm64`, and `tauri build` with
`app/tauri.bundle.conf.json` and `app/tauri.bundle.macos.conf.json`) plus
`app/tauri.appstore.conf.json`, then `scripts/macos-sign.sh` with
`app/macos/entitlements.appstore.plist`, `productbuild`, and finally `scripts/mas-check.sh`
(signatures, each executable's exact entitlements, the profile, Info.plist, LGPL-only helpers,
the license set and the package). The `.pkg` path is the last line it prints; the output is in
`target/skia/release/bundle/mas`. Signed like CI, it needs `SC_MAS_PROFILE` and both
identities in the keychain; the header of `scripts/mas-build.sh` lists its other variables. It
shares `target/skia` with `scripts/local-bundle.sh`: while it builds, it moves that script's
Developer ID app (`release/bundle/macos/simple-capture.app`) into
`target/skia/release/.mas-build-stash`, and puts it back when it ends, also when it fails. If a
run is killed, it stays in that folder and the next run refuses to start: move it back into
`target/skia/release/bundle/macos`, or delete the folder.

## Cutting a version

1. `main` has everything to release (merge the working branch through a pull request).
2. The version in `Cargo.toml`, `app/tauri.conf.json` and `ui/package.json` matches, and
   `CHANGELOG.md` has its `## <version>` section. From the second store version on, that
   section is also the App Store's What's New (Mac App Store, Each release, step 2), so
   `python3 -I appstore/check.py` must pass (CI runs it on pull requests and on `main`).
3. Tag `main` (`git tag v0.1.0 && git push origin v0.1.0`). The package workflow checks the tag
   against the version, builds every installer and the helpers' source archive, and creates a
   **draft** GitHub release. Asset names are the ones GitHub would give them, with spaces and
   other special characters turned into `.` (the DMG is `simple-capture_<version>_aarch64.dmg`).
   `SHA256SUMS.txt` lists every asset, and each has a build provenance attestation:
   `gh attestation verify <file> --repo thtmnisamnstr/simple-capture`.
4. Check the draft's files, then press Publish.
5. Homebrew follows within about an hour (see Homebrew).
6. The Mac App Store build is built, uploaded and submitted for review (see Mac App Store).
