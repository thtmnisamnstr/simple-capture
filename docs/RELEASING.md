# Releasing

## macOS signing and notarization

Releases are signed with the owner's **Developer ID Application** certificate and notarized, so
macOS opens them without warnings and keeps Screen Recording, Camera and Microphone grants across
updates. Without the secrets below, the package workflow still builds an ad hoc signed DMG.

### On this Mac (local builds)

1. The certificate and its private key are in the login keychain, with Apple's **Developer ID -
   G2** intermediate (https://www.apple.com/certificateauthority/). Check:
   `security find-identity -v -p codesigning` lists `Developer ID Application: …`.
2. Notarization credentials, stored once in the keychain (it asks for an app-specific password
   from account.apple.com → Sign-In and Security):
   `xcrun notarytool store-credentials "simple-capture" --apple-id <Apple ID> --team-id <Team ID>`
3. `scripts/local-bundle.sh` then signs, notarizes and staples the app (`scripts/macos-sign.sh`);
   `scripts/local-bundle.sh --dmg` also builds the installer DMG and notarizes that too.

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

The `bundle (macos-15)` job then imports the certificate into a temporary keychain, signs the
app and its helpers, notarizes and staples it, builds the DMG, and signs, notarizes and staples
that too.

## Windows

Installers are not code signed, so SmartScreen warns until a signed build earns reputation.
Signing would go through Azure Trusted Signing (Artifact Signing) or an OV certificate.

## Cutting a version

1. `main` has everything to release (merge the working branch through a pull request).
2. The version in `Cargo.toml`, `app/tauri.conf.json` and `ui/package.json` matches, and
   `CHANGELOG.md` has its section.
3. Tag `main` (`git tag v0.1.0 && git push origin v0.1.0`). The package workflow checks the tag
   against the version, builds every installer and the helpers' source archive, and creates a
   **draft** GitHub release.
4. Check the draft's files, then press Publish.
