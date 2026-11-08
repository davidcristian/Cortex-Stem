# Research: window preview options

[ADR-0009](../../adr/0009-window-thumbnails.md) proposes how the dock
shows live pictures of an app's windows when the pointer rests on its
icon. This page compares the four options it considered. A1 to A11 name
the assumptions listed in the ADR's
[Assumptions](../../adr/0009-window-thumbnails.md#assumptions).

## 1. No thumbnails

Keep what exists: the tooltip, click to cycle, the window list in the menu.

- For: no cost and no risk. macOS's dock has no hover previews either.
- Against: with several windows of one app, the user can only pick by title.
  Windows users expect previews from the stock taskbar, and Stem can hide
  that taskbar.

## 2. A separate topmost tool window (the proposal)

A preview window of its own, shown above the hovered item, is the
thumbnails' destination.

- **2a. A native window drawn by `stem-platform`.** Titles, close buttons,
  hover states, light and dark theme and high contrast would all be drawn a
  second time, in GDI. Mock-mode screenshots could not show it, and every
  pixel could only be checked on Windows. Rejected.
- **2b. A Tauri webview window** (`preview`), prepared like a dock window.
  - For: the dock window stays exactly as it is, and it is never layered.
  - Against: a third webview (memory and start-up time).
  - Against: it is shown and hidden on every hover. tao clears its "do not
    focus" marker after creating a window (tao 0.37.2, `window.rs`), so
    every later `show()` uses `SW_SHOW` (`window_state.rs`), which
    activates the window ([`ShowWindow`][show]). `WS_EX_NOACTIVATE` only
    covers clicks and the system's own choices
    ([extended styles][exstyles]). The platform would have to show it with
    `SW_SHOWNOACTIVATE` or `SetWindowPos(SWP_SHOWWINDOW | SWP_NOACTIVATE)`
    ([`SetWindowPos`][swp]).
  - Against: the hover spans two pages. The dock page loses pointer events
    when the pointer enters the preview, so enter and leave must go
    through Rust, with a grace timer for the gap between the windows and
    the races it brings.
  - Against: two topmost windows of Stem's. The topmost re-assert moves the
    dock to the top of the topmost band and could put it above the preview.
  - Against: one shared preview window moves between monitors and gets a
    DPI change each time it crosses to another scale. One per dock
    multiplies the webviews.
  - Against: its transparent corners and shadow take clicks unless the
    hit-test thread tracks it too.

## 3. The dock window is the destination (recommended)

[ADR-0009](../../adr/0009-window-thumbnails.md#recommendation) recommends
this option and describes it.

- For: no new window, webview or thread. No-activation, z-order and
  DPI are already solved for this window.
- For: one page gets every pointer event of the hover, from the icon to
  the cards. Auto-hide holds, tooltips and magnification stay local.
- For: Playwright covers the layout in mock mode.
- Against: the window is taller by the band while previews are on. The band
  is transparent and click-through, but only while click-through is on.
- Against: the window turns layered when it goes click-through. Thumbnails
  must be gone before that happens (A5).
- Against: CSS cannot clip, animate or draw over the DWM image, and whether
  DWM draws it above or below the web content is not documented (A1).
- Variant rejected: grow the window on each open through the drag path.
  That resizes a WebView2 window under the pointer on every hover. This
  rejection assumes that the panel could then show at the wrong height for
  a frame. The dock window already spans the full width so that animations
  never resize it.

## 4. Captured bitmaps (rejected without a prototype)

Copy each window into an image and send it to the page as a PNG.

- `PrintWindow` is a blocking call that returns only after the target app
  has drawn itself (`WM_PRINT`) ([`PrintWindow`][print]). A hung app would block the
  calling thread, while the dock must stay responsive when a target process
  is hung (the Windows engineering rules in
  [`docs/development/windows-rules.md`](../../development/windows-rules.md)).
  The image is not live either.
- Windows.Graphics.Capture draws a coloured border around a captured window.
  Turning it off needs a package capability and the user's consent
  ([`IsBorderRequired`][wgc]). Stem is not a packaged app.
- Both move pixels through Stem's process and the IPC. DWM thumbnails move
  none.

[show]: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-showwindow
[exstyles]: https://learn.microsoft.com/en-us/windows/win32/winmsg/extended-window-styles
[swp]: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowpos
[print]: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-printwindow
[wgc]: https://learn.microsoft.com/en-us/uwp/api/windows.graphics.capture.graphicscapturesession.isborderrequired
