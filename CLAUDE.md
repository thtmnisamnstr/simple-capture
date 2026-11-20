# Simple Capture: agent rules

## macOS permissions: one dev app, never renamed
The owner approves Screen Recording / Camera / Microphone / system-audio prompts by hand. Every new app name, bundle ID, or re-signed bundle makes macOS ask again, so:

- Any dev, spike, test, or bench binary that needs a macOS privacy permission runs through `tools/macos-dev-host/run.sh <binary> [args...]`. It launches the binary as a child of `~/Applications/Simple Capture Dev.app` (bundle ID `com.simplecapture.dev`), which holds all the grants. Rebuilding or renaming your binary does not matter.
- Never create another `.app` bundle, Info.plist, or bundle identifier to get a permission, and never re-sign or rebuild `Simple Capture Dev.app` (`install.sh --force` resets every grant; only the owner may ask for that).
- Don't call `tccutil`, edit TCC databases, or open System Settings for the owner.
- Child processes inherit the host's grants, so kill tests and helpers spawned by the binary need nothing extra. `SC_DEV_PID_FILE=path` records the child's pid.
- The shipped app (`com.simplecapture.desktop`) will keep its own grants across rebuilds only once it is signed with a stable Apple Development identity (see docs/SPEC.md → Platform notes).

## Don't steal the owner's focus
The owner keeps working while tests run. Every dev, spike, test and fixture binary must leave the frontmost app alone:
- Use `NSApplicationActivationPolicyAccessory` (or `Prohibited` when it shows no window). Never call `activateIgnoringOtherApps`/`activate`, `makeKeyAndOrderFront`, or Tauri's `set_focus` in tests.
- Fixture and overlay windows are non-activating panels (`NSWindowStyleMaskNonactivatingPanel`), shown with `orderFrontRegardless`. They don't become key unless a test needs keyboard input.
- `tools/macos-dev-host/run.sh` already launches with `open -g`; don't add anything that brings the host or its child to the front.
- Announce live runs that record the real screen, camera or mic (start and end banner) and hold `scripts/lib/live.sh`'s lock so only one runs at a time.

## Git
The owner allows any git operations needed to get work done: branches, commits, worktrees, merges, rebases, pushes to origin (a PUBLIC GitHub repo, so never commit secrets or recordings), and PRs.
