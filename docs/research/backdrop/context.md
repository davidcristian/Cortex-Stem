# Backdrop research: context

The goal of the [backdrop research](index.md) is an *Acrylic* and a *Mica*
choice for `appearance.backdrop`, with the [CSS look](css-look.md) as the
fallback where Windows cannot draw them. The workspace lists the
`window-vibrancy` crate. Both values are read from and written to
`settings.json`, but the settings page shows them disabled. Set by hand in
`settings.json`, they turn on the [prototype](prototype.md) under the
primary dock's panel; every other dock draws the built-in CSS look.

The problem is the shape of the dock window. The window spans the full
length of one edge of its monitor's work area and is transparent
everywhere except the rounded panel, which the page centres
(`src-tauri/src/windows/placement.rs`: `plan_dock_rect`, `edge_rect`). The
`position` setting picks the edge, `bottom` by default. This note describes
the dock on `bottom`; on the other edges the same holds along that edge. The
window's height is the panel plus room for magnified icons, tooltips and
the bounce (`HEADROOM` in `src/dock/layout.ts`, and `SIDE_HEADROOM` on
`left` and `right`). Every system backdrop Windows offers is drawn behind
a whole window. On the dock window it would paint a band across the bottom
of the screen, not a panel.

What the code does, as far as it matters here:

- tao (Tauri's window layer) makes a transparent window by calling
  [`DwmEnableBlurBehindWindow`][blurbehind] with an empty region
  (`CreateRectRgn(0, 0, -1, -1)`), so DWM uses the window's alpha values
  (tao 0.37.2, `platform_impl/windows/window.rs`, `on_create`). wry hosts
  WebView2 in a child window with a transparent default background, and
  then "WebView will render hosting app content as the background"
  ([`DefaultBackgroundColor`][webview2-bg]).
- tao keeps `WS_CAPTION | WS_SYSMENU` on undecorated windows and removes the
  frame in its [`WM_NCCALCSIZE`][nccalcsize] handler.
  `Platform::prepare_dock_window` turns DWM rounding off
  ([`DWMWA_WINDOW_CORNER_PREFERENCE`][dwmwa] = `DWMWCP_DONOTROUND`) and keeps
  `WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE` on the window
  (`crates/stem-platform/src/windows/dock_window.rs`). A click never
  activates the dock window, and the system never activates it on its own.
  It is the foreground window only after the focus shortcut
  (`behavior.focusShortcut`) moves the keyboard into it, and while one of
  its native context menus is open.
- Click-through toggles `WS_EX_LAYERED | WS_EX_TRANSPARENT` on the dock
  window at every pass-through transition: each time the pointer enters
  the panel or leaves the envelope ([layered windows][window-features]).
  The envelope is the largest box a magnified or dragged icon can take
  ([Hit region](../../ipc/hit-region.md)).
- The dock window's subclass sets `HWND_TOPMOST` again when an ordinary
  window is above the dock. This note calls that the topmost re-assert.
- The panel changes size often. A slot is `icon × --scale + gap` wide
  (`src/lib/components/DockItem.svelte`), so magnification widens the panel
  through layout: at once when the pointer enters, over a 120 ms CSS
  transition when it leaves (the settle), and near the ends of the row. In
  the middle of the row the width hardly changes (`src/lib/dock/magnify.ts`:
  the growth is the same whatever the pointer offset). Items grow in and
  out, a file drag opens a gap, and a drag grows the window. The UI reports
  the panel only at rest (`set_dock_panel_rect`); nothing reports the live
  box.
- Auto-hide slides the panel off its edge with a CSS translate over 200 ms,
  inside a window that does not move ([ADR-0008](../../adr/0008-auto-hide.md)).

Two documented facts apply to every native option:

- `DWMWA_SYSTEMBACKDROP_TYPE` needs Windows 11 build 22621
  ([`DWM_SYSTEMBACKDROP_TYPE`][sbt]).
- Background acrylic and Mica turn into a solid colour "when an app window
  on desktop deactivates" ([Acrylic][acrylic], [Mica][mica]). The dock is
  almost never the active window. Without a workaround, both materials
  would look flat nearly all the time.

[sbt]: https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwm_systembackdrop_type
[dwmwa]: https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwmwindowattribute
[blurbehind]: https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/nf-dwmapi-dwmenableblurbehindwindow
[window-features]: https://learn.microsoft.com/en-us/windows/win32/winmsg/window-features
[nccalcsize]: https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-nccalcsize
[acrylic]: https://learn.microsoft.com/en-us/windows/apps/design/style/acrylic
[mica]: https://learn.microsoft.com/en-us/windows/apps/design/style/mica
[webview2-bg]: https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2controller2
