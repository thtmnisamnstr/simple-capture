# Mac App Store listing

The App Store Connect metadata for the Mac App Store edition of Simple Capture (bundle ID
`com.simplecapture.desktop`, platform `MAC_OS`). The app-store workflow sends these files
through the App Store Connect API (`scripts/asc/asc.py prepare`) on every run, so an edit made
in App Store Connect is overwritten by the next run. Edit them here, so the listing stays under
version control.

The listing describes the Mac App Store build, which differs from the GitHub build: its FFmpeg
is LGPL-only (no x264 or x265, so H.264 and HEVC are encoded by VideoToolbox), and it has no
gifski (GIFs use FFmpeg's palette mode). Don't promise those encoders, other platforms, prices
or "beta" anywhere in the listing. The app makes no network connections; say it that way, not
"no network access": the store build carries the `network.client` entitlement because its
WKWebView windows need it in the sandbox (`review.json` explains this to App Review).

Check the listing after every edit:

```bash
python3 -I appstore/check.py                      # rules, limits, Cargo.toml version's What's New
python3 -I appstore/check.py --version 0.2.0      # the same with the What's New of 0.2.0
python3 -I scripts/asc/asc.py check-metadata      # what the upload script accepts
```

## Files and where they go

| File | API resource and attribute | Limit |
|---|---|---|
| `metadata/<locale>/name.txt` | `appInfoLocalizations.name` | 30 |
| `metadata/<locale>/subtitle.txt` | `appInfoLocalizations.subtitle` | 30 |
| `metadata/<locale>/privacy_url.txt` | `appInfoLocalizations.privacyPolicyUrl` | URL |
| `metadata/<locale>/description.txt` | `appStoreVersionLocalizations.description` | 4000 |
| `metadata/<locale>/keywords.txt` | `appStoreVersionLocalizations.keywords` | 100 |
| `metadata/<locale>/promotional_text.txt` | `appStoreVersionLocalizations.promotionalText` | 170 |
| `metadata/<locale>/support_url.txt` | `appStoreVersionLocalizations.supportUrl` | URL |
| `metadata/<locale>/marketing_url.txt` | `appStoreVersionLocalizations.marketingUrl` | URL |
| `CHANGELOG.md`, the version's `## <version>` section | `appStoreVersionLocalizations.whatsNew` | 4000 |
| `app_info.json` `primaryCategory`, `secondaryCategory` | `appInfos` relationships `primaryCategory`, `secondaryCategory` (`appCategories` ids) | |
| `app_info.json` `copyright` | `appStoreVersions.copyright` | |
| `app_info.json` `contentRightsDeclaration` | `apps.contentRightsDeclaration` | |
| `age_rating.json` | `ageRatingDeclarations` attributes, sent as they are | |
| `review.json` | `appStoreReviewDetails` attributes (`demoAccountRequired`, `notes`) | notes 4000 |
| `screenshots/<locale>/*.png` | `appScreenshotSets` with `screenshotDisplayType` `APP_DESKTOP` | 10 |

## What's New

There is no release-notes file here. What's New is the version's section of the repository's
`CHANGELOG.md` (the `## 0.2.0` heading, or `## 0.2.0 (unreleased)`). `CHANGELOG.md` covers every
edition, so `scripts/asc/asc.py` leaves out the headings, bullets and paragraphs about another
one (those naming Linux, Android, iOS, gifski, x264 or x265, or Windows the operating system, as
in "on Windows", "Windows:" or a `### Windows` heading; "Windows from another Space" stays) and
logs how many. The rest becomes plain text: headings become plain lines, `- ` bullets become
`• `, links become their text, and `**` and backticks are dropped.

- The app's first store version (normally 0.1.0; any version while none has gone live) sends
  no What's New. `check.py` skips What's New for the oldest version in `CHANGELOG.md`.
- For every later version the section must exist and keep something for the Mac App Store, or
  the release fails.
- `check.py` checks that text (in CI, in the release workflow before the build, and again in
  `asc.py preflight`): at most 4000 characters, and no beta, x264, x265, gifski, price, prices,
  pricing, `$` amounts, the word free (crash-free, free-form and free disk space are fine) or
  "no network" without "connections".

## Screenshots

PNG only, sRGB with no transparency, at 2880 × 1800, 2560 × 1600, 1440 × 900 or 1280 × 800
pixels. There can be 1 to 10. They are uploaded in filename order, so name them with a two-digit
prefix. See `screenshots/en-US/README.md`.

`screenshots/tools/make.sh` renders them again from the real UI running on its mock backend,
with the store-edition flag (`edition=mas`), so the Export sheet shows what the store build
offers. It needs Google Chrome and the UI's dependencies (`pnpm --dir ui install`).

## Notes for the scripts

- Leading and trailing whitespace is stripped from every text file; the files end with a
  newline.
- The per-locale file names follow fastlane `deliver`'s, but `app_info.json`,
  `age_rating.json` and `review.json` are read only by `scripts/asc/asc.py`.
- `PHOTO_AND_VIDEO` and `PRODUCTIVITY` are `appCategories` ids whose platforms include
  `MAC_OS`; there is no separate video category id.
- `age_rating.json` follows the age rating questionnaire Apple introduced in 2025 (age bands
  4+, 9+, 13+, 16+ and 18+). It leaves out the deprecated `ageRatingOverride` and the fields
  that only apply after a "yes" (`socialMediaAgeRestricted`, `gracRatingClassificationNumber`).
  Every answer is "none" or "no", which rates the app 4+.

## Set by the workflow

- Price (free) and availability (all territories): `scripts/asc/asc.py prepare`, on the first
  run only. After that they are left as they are, so change them in App Store Connect.
- App Review contact (name, phone, email): the `ASC_REVIEW_CONTACT_FIRST_NAME`,
  `ASC_REVIEW_CONTACT_LAST_NAME`, `ASC_REVIEW_CONTACT_PHONE` and `ASC_REVIEW_CONTACT_EMAIL`
  repository secrets, or typed in App Store Connect → the version → App Review Information.
  Never put them in this folder; `check.py` and `asc.py` reject contact details here.

## Set by the owner in App Store Connect

These have no API:

- App Privacy: "Data Not Collected". `docs/PRIVACY.md` is the policy it links to.
- Agreements and EU trader status.
