# Plan

The milestones are ordered so that each one confirms what the next one
depends on. Each milestone lists its scope, its status (the parts of the scope
that the code does), and its **exit criterion**:
something a person can observe that ends the milestone. M4 and M6 have no
exit criterion.

## M0: Foundation

**Scope.** The Cargo workspace, the lints, the shared types, the pure engine
with its tests, the platform trait with a mock, the service layer with
integration tests, a Tauri app skeleton that type-checks for Windows from
Linux, and the docs that set the rules for the code.

**Status.** Done.

**Exit criterion.** `scripts/check-all.sh` passes on Linux, and
`cargo clippy --workspace --all-targets --target x86_64-pc-windows-msvc -- -D warnings`
passes for the whole workspace.

## M1: The dock appears

**Scope.** The first run on Windows. The dock appears bottom-centred on the
primary monitor as a floating, rounded, translucent panel with icons that
magnify. It shows the pinned apps from the settings file and the running
windows, with their real icons. Clicks work: launch, activate, minimise, and
cycle through an app's windows. A tray icon offers Quit.

**Status.** Done. A left click on the tray icon also shows or hides the dock,
and its menu also has _Settings…_ and _Restart_.

**Exit criterion.** A person installs Stem on Windows 11 with the NSIS
installer that CI builds, and finds everything above working.

## M2: Daily use

**Scope.** Native context menus, drag to reorder, drag and drop to pin,
auto-hide, a settings window that applies each change at once, start at
sign-in, a single instance, theme sync (accent colour, light and dark mode,
contrast themes), the Recycle Bin item, the Start, Task View and Show Desktop
items, attention flashing, an Acrylic or Mica backdrop option, and a keyboard
shortcut that shows or hides the dock.

**Status.** Not done. Theme sync also follows
Windows' _Transparency effects_.

**Exit criterion.** Stem is the only launcher for a week of daily use, with
the stock taskbar auto-hidden.

## M3: Multiple monitors and refinements

**Scope.** A dock per monitor, handling of DPI changes, monitor hot-plug,
hit-test passthrough for the transparent headroom (clicks there reach the windows behind
the dock), filtering by virtual desktop, fullscreen detection, window
thumbnails on hover (research first), keyboard navigation and accessibility,
a performance pass (event coalescing, a disk icon cache, a measurement of
idle CPU), and handling of elevated windows (apps run as administrator)
without failures.

**Status.**

- Each dock item has `role="button"` and an `aria-label`.
- Bursts of window events are coalesced: title changes per
  window, and the service sends at most one state update per 16 ms.
- Elevated windows appear with their name and icon.

**Exit criterion.** Everything above works on a setup with two monitors at
different DPI.

## M4: The screen edge

An **AppBar** is a window that the shell gives a band of a screen edge, taken
out of the monitor's work area, so that maximised windows stop short of it.

**Scope.** Optional hiding of the stock taskbar, restored when Stem exits and
after a crash; screen space reserved for the dock as an AppBar; the left,
right and top positions.

**Status.** Not done.

**Exit criterion.** None.

## M5: Release

**Scope.** Installer improvements, an updater, a release workflow (tag → build →
GitHub release), crash logging, a README with screenshots and an FAQ, and
notes on code signing.

**Status.** Not done.

**Exit criterion.** v0.1.0 is published on GitHub Releases.

## M6: Optional features

**Scope.** Jump lists and recent documents, research on the notification
area, stacks and folders, widgets (clock, battery, volume), and theming.

**Status.** Not done.

**Exit criterion.** None.

## Explicit non-goals (for now)

- Replacing the notification area (system tray). The stock taskbar stays
  available for the tray, the input indicators and the clock, auto-hidden.
- Supporting Windows 10 (DWM backdrop APIs and some WinEvents differ).
- macOS or Linux builds.

## Risks and how the plan addresses them

| Risk | Mitigation |
|------|------------|
| Most checks run on Linux, where the Windows code cannot run ([ADR-0004](adr/0004-linux-first-verification.md)) | Behaviour lives in `stem-core` and `stem-service`, which are tested on Linux with the mock platform; the Windows code is linted for the Windows target from Linux; CI builds the installer on Windows; the M1 exit criterion is a run on Windows by a person |
| `SetForegroundWindow` restrictions make "activate" unreliable | Activation tries the known fallbacks and logs which one worked; a test matrix on Windows checks them |
| WebView2 takes the focus despite `WS_EX_NOACTIVATE` | A test on Windows checks it; the fallback is to subclass the dock window and handle `WM_MOUSEACTIVATE`, with the code built for the hit-test passthrough |
| The transparent headroom blocks clicks on the windows beneath it | The headroom is only as large as the magnified icons, the tooltips and the bounce need (`HEADROOM` in `src/dock/Dock.svelte`); hit-test passthrough sends clicks on it to the windows beneath |
| Icon extraction is slow or fails for UWP or elevated apps | Icons are extracted on demand, off the UI thread, and cached; each icon source has a documented fallback |
| Win32 or `windows` crate names are guessed wrong | The compiler checks them: `cargo clippy --workspace --all-targets --target x86_64-pc-windows-msvc -- -D warnings` must pass |
