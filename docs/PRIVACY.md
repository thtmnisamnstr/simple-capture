# Simple Capture Privacy Policy

Effective date: November 25, 2026

Simple Capture is a screen recorder and editor. This policy covers every copy of the app: the
one on the Mac App Store and the builds published on GitHub (and installed through Homebrew).

## In short

Simple Capture collects no data. It has no account, no ads, no analytics and no tracking, and it
makes no network connections, so nothing you record or do in the app is sent to the developer or
to anyone else. Your recordings stay on your computer.

## What the app collects

Nothing. The developer receives no recordings, files, device details, usage statistics or
personal information from the app, and does not sell or share any data.

## What stays on your computer

Simple Capture saves these files on your computer, and only there:

- **Recordings.** Each recording is a folder in `Movies/Simple Capture` (`Videos/Simple Capture`
  on Windows and Linux) with the recorded tracks, a project file, thumbnails and a log of that
  recording.
- **Exports.** Saved next to the recordings, or in a folder you choose.
- **Settings and logs.** Your settings, export presets, a record of which encoders work on your
  computer, and the app's logs. They are in this folder:
  - macOS (Mac App Store and GitHub alike):
    `~/Library/Containers/com.simplecapture.desktop/Data/Library/Application Support/com.simplecapture.desktop`
  - Windows: `%APPDATA%\com.simplecapture.desktop`
  - Linux: `$XDG_DATA_HOME/com.simplecapture.desktop`, or
    `~/.local/share/com.simplecapture.desktop` when `XDG_DATA_HOME` is not set
- **Diagnostics reports.** Created only when you run Diagnostics. A report lists the app's
  version, your displays, cameras and microphones, the encoders that work and the test results,
  with your home folder's name removed. It holds no recordings. It stays on your computer unless
  you choose to send it to someone.

Logs can contain the names of your devices, displays, windows and apps, and file paths. They are
never sent anywhere by the app.

## Permissions and why they are used

macOS asks for each permission only when you press Allow in Simple Capture, and only for what
you use:

- **Screen Recording**: to list your windows and apps, and to record the screen and system
  audio while you are recording.
- **Camera**: only when you choose a camera to appear in the video.
- **Microphone**: only when you choose a microphone to narrate.
- **Files and folders**: the Movies folder for recordings, and any other folder only after you
  choose it in a macOS dialog.

What the screen, camera and microphone capture is written only to files on your computer and
processed only there: drawing waveforms, finding silences and encoding exports all happen on your
computer. The app never uploads or shares it. You can turn any permission off at any time in
System Settings → Privacy & Security.

## Network

Simple Capture makes no network connections. The FFmpeg helpers bundled with it are built with
network support turned off. The app does not check for updates itself; the Mac App Store or
Homebrew handles updates when you install it that way.

On macOS the app carries Apple's outgoing-network entitlement only because the system web view
that draws its windows needs it inside the App Sandbox, even though it loads them from
files inside the app. The app never uses it to connect anywhere.

## Apple

When you get Simple Capture from the Mac App Store, Apple handles the download under Apple's
own privacy policy. The developer sees only the anonymous, aggregate reports that Apple gives
every developer. If you have chosen to share analytics with app developers in System Settings →
Privacy & Security → Analytics & Improvements, Apple may also share anonymous crash reports and
usage data for Simple Capture with the developer. You control that setting.

## Sharing your recordings

Recordings leave your computer only when you send them yourself, for example by attaching an
export to an email or a chat. The service you send it with handles it under its own privacy policy.

## Children

Simple Capture collects no data from anyone, including children.

## Deleting your data

Move recordings to the Trash from the Library, or delete the `Movies/Simple Capture` folder
(`Videos/Simple Capture` on Windows and Linux). To remove everything, also delete the settings
and logs folder listed above for your system, and the app itself.

## Changes

Any change to this policy is published in this file with a new effective date. Earlier versions
are in the repository's history.

## Contact

Questions about privacy: open an issue at
<https://github.com/thtmnisamnstr/simple-capture/issues>. Issues are public, so don't attach
recordings or personal information.
