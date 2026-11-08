# Backdrop option 6: a second backdrop window under the dock window

Option 6 of the [backdrop research](index.md) is a small native window that
`stem-platform` creates, one per dock window, sized and shaped to the
panel, and kept directly under the dock window in the z-order. Only this
window has the material. The dock window does not change. The terms
*settle* and *topmost re-assert* are defined in [context.md](context.md),
the assumptions A1 to A10 in [assumptions.md](assumptions.md), and the
other options in [options.md](options.md).

What the window needs (the prototype must prove each point):

- **Styles.** `WS_POPUP | WS_CAPTION`, with a `WM_NCCALCSIZE` handler that
  gives the whole window to the client area, and `WS_EX_TOOLWINDOW |
  WS_EX_NOACTIVATE | WS_EX_TOPMOST` ([extended styles][exstyles]). The
  caption is required. PowerToys found that DWM silently skips
  `DWMWA_SYSTEMBACKDROP_TYPE` on frameless `WS_POPUP` windows, and added
  `WS_CAPTION` with a `WM_NCCALCSIZE` handler ([pull request
  50494][powertoys]). tao does the same on the dock window. Not documented
  (A3).
- **Attributes** ([`DWMWINDOWATTRIBUTE`][dwmwa]).
  `DWMWA_SYSTEMBACKDROP_TYPE`: `DWMSBT_TRANSIENTWINDOW` for *Acrylic*,
  `DWMSBT_MAINWINDOW` for *Mica*. `DWMWA_USE_IMMERSIVE_DARK_MODE` from the
  theme. `DWMWA_BORDER_COLOR = DWMWA_COLOR_NONE`, so DWM draws no border.
  `DWMWA_TRANSITIONS_FORCEDISABLED = TRUE`, so showing and hiding do not
  animate. `DwmExtendFrameIntoClientArea` with −1 margins if A2 shows it is
  needed.
- **Activation.** The window handles [`WM_NCACTIVATE`][ncactivate] as if it
  were active, and gets one `WM_NCACTIVATE(TRUE)` after creation. PowerToys
  reports that this makes DWM draw the live material on a window it would
  otherwise treat as inactive, with no real activation, and that it is safe
  on a `WS_EX_NOACTIVATE` window. Not documented (A6). Without it, Acrylic
  and Mica show only their solid fallback colour.
- **Input.** The window must never take a click. Preferred: `WS_EX_LAYERED
  | WS_EX_TRANSPARENT` with [`SetLayeredWindowAttributes`][slwa] at alpha
  255; with both bits "the mouse events will be passed to other windows
  underneath" ([layered windows][window-features]). Whether DWM still draws
  the material on that window is A4. One third-party report
  ([noctty pull request 239][noctty]) shows the material on a layered
  `LWA_ALPHA` window. Fallback: an ordinary window that returns
  `MA_NOACTIVATE` for [`WM_MOUSEACTIVATE`][mouseactivate]. It could then
  take a click only in the few milliseconds before the hit-test thread
  makes the dock clickable.
- **Z-order.** "An owned window is always above its owner", and ownership
  cannot change after creation ([owned windows][window-features]). So the
  dock window cannot be the backdrop's owner. Two ways remain:
  - *Unowned* (preferred). Create it topmost. Always place it with
    `hWndInsertAfter = dock` ([`SetWindowPos`][setwindowpos]), never with
    `HWND_TOP` or `HWND_TOPMOST`, and show it the same way, not with
    `ShowWindow`. The dock's subclass restacks it after every z-order change
    of the dock (`WM_WINDOWPOSCHANGED` without `SWP_NOZORDER`, which the
    subclass already logs) and after the topmost re-assert. Another window
    can end up between the two for a moment. The backdrop can never cover
    the dock.
  - *The dock owned by the backdrop.* Tauri has `owner_raw(HWND)` on
    Windows. The window manager then keeps the dock above the backdrop, and
    "when a non-topmost window is made topmost, its owned windows are also
    made topmost". But the backdrop must exist before the dock window, for
    every dock window, even with the CSS look (the setting changes at
    runtime). Destroying it destroys the dock window. It changes how the
    dock window is created.
- **Shape.** [`DWMWCP_ROUND`][corner] gives DWM's own anti-aliased corner,
  but at the system's radius. That radius is not documented (A9) and is not
  `appearance.cornerRadius` (22 px by default). Where the two differ, the
  material shows outside the panel's curve. So the prototype uses a region
  from `CreateRoundRectRgn` at the panel's radius in physical px, inset by
  one pixel so its pixel steps sit under the panel's 1 px hairline, with
  `DWMWCP_DONOTROUND`. Never both: PowerToys traced a corner-shadow leak to
  a window rounded by a region and by the corner preference at once. Whether
  the region clips the material is A7. DWM corners stay an option when the
  user's radius matches the system's.
- **Size and place.** The panel's box in physical screen px, rounded
  inwards (the hit region rounds outwards; the material must never stick
  out). At rest the app already computes it from `set_dock_panel_rect`
  (`panel_on_screen`). While the box changes, the UI would report the live
  box once per animation frame. During hover it can compute the box from
  the scales it writes, with no layout read: the rest width plus
  `iconSize × Σ(scale − 1)`, centred. During the settle and item animations
  it reads the box once at the start of each frame. Each change goes
  UI → IPC → main thread → `SetWindowPos`. The page's frame and the window
  move reach DWM by two paths with no common commit, so the material's edge
  lags the panel's edge by one to three frames (A8). The lag shows when the
  pointer enters (the panel widens at once) and during the settle. In the
  middle of the row nothing is sent. [`DeferWindowPos`][deferwindowpos]
  moves several windows together, but the WebView2 frame is not one of
  them.
- **Slide.** The backdrop hides when the slide out starts and shows when
  the slide in ends (`onpaneltransitionend` already measures there). The
  panel slides with its CSS tint only. Following the slide would take about
  12 moves in 200 ms from Rust with the same easing. Those moves would lag
  the same way, and the window would need clipping at the work area's
  bottom edge, as the panel is clipped by its window. So the backdrop does
  not follow the slide. Under reduced motion it hides and shows the same
  way around the fade.
- **Threads.** The window is created on the main thread, the thread that
  created the dock windows and runs tao's message loop. Its window
  procedure runs there, and a `SetWindowPos` on it is synchronous with
  tao's own moves of the dock. The procedure only handles `WM_NCCALCSIZE`,
  `WM_NCACTIVATE`, `WM_NCHITTEST` and `WM_MOUSEACTIVATE`; nothing blocks.
  The dock subclass works the same way. But the threading rule in
  `AGENTS.md` puts windows that need a message loop on a platform thread,
  so the ADR must record the exception.

Assessment:

- Limited to the panel: yes.
- Size and magnification: yes, with one to three frames of lag at the edges
  while the box changes (A8).
- Slide: yes, by hiding. The material appears at once at the end of a
  reveal.
- Cost: the largest. Roughly 400 to 600 lines in `stem-platform` (window
  class, procedure, attributes, region, placement, restacking), a trait
  method and its mock, about 150 lines in `stem-app` (per-label routing,
  conversion, hiding on toggle and fullscreen), the UI's live box and an IPC
  change. A prototype first: about a day.
- Risk: medium. Four facts the design depends on are undocumented (A3, A4,
  A6, A7). The prototype tests them before anything else is built.

[dwmwa]: https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwmwindowattribute
[corner]: https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwm_window_corner_preference
[setwindowpos]: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowpos
[deferwindowpos]: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-deferwindowpos
[slwa]: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setlayeredwindowattributes
[window-features]: https://learn.microsoft.com/en-us/windows/win32/winmsg/window-features
[exstyles]: https://learn.microsoft.com/en-us/windows/win32/winmsg/extended-window-styles
[ncactivate]: https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-ncactivate
[mouseactivate]: https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-mouseactivate
[powertoys]: https://github.com/microsoft/PowerToys/pull/50494
[noctty]: https://github.com/amanthanvi/noctty/pull/239
