# Backdrop research: prototype

The prototype of the [backdrop research](index.md) is
[option 6](backdrop-window.md) for the primary dock, sized at rest from
`set_dock_panel_rect` and driven by `appearance.backdrop` in
`settings.json`, with no UI or IPC change. It is made to settle A2 to A10
([Assumptions](assumptions.md)) through steps 2 to 15 of the
[manual test plan](manual-test-plan.md). It consists of:

- `crates/stem-platform/src/windows/backdrop.rs`: the window of option 6,
  with the styles, attributes, `WM_NCACTIVATE` handling, layered input rule,
  z-order rule and region of "What the window needs" in
  [option 6](backdrop-window.md). The region is `CreateRoundRectRgn` inset
  by 1 px, with corners concentric to the panel's (radius minus 1), or
  `CreateRectRgn` for square corners.
  `DwmExtendFrameIntoClientArea` is called with −1 margins, and the class
  paints the client area black, as the "sheet of glass" effect needs, so
  switching the call off tests A2.
- `Platform::set_dock_backdrop(handle, Option<DockBackdrop>)`, with a
  platform-local type (rect in physical screen px, radius in physical px,
  material, dark). The trait's default returns
  `Unsupported("dock backdrop")`; the mock records every call.
- The dock subclass posts a restack after every z-order change of the dock
  (`WM_WINDOWPOSCHANGED` without `SWP_NOZORDER`) and after the topmost
  re-assert ([context](context.md)). The backdrop thread moves the backdrop
  under the dock only when `GetWindow(dock, GW_HWNDNEXT)` is not already the
  backdrop.
- `src-tauri/src/backdrop.rs`: the primary dock only. It reads
  `appearance.backdrop` from the settings (`acrylic` →
  `DWMSBT_TRANSIENTWINDOW`, `mica` → `DWMSBT_MAINWINDOW`, `css` → none; the
  settings page still shows the two disabled), `appearance.cornerRadius`
  times the window's scale, and dark from `settings.theme` or Windows' app
  theme. The material is sized from every conversion of
  `set_dock_panel_rect` (`push_dock_panel`, `sync_dock_panel`), rounded
  inwards, while the window's viewport keeps the size the report was
  measured in (`panel_fits`). It is hidden while the window is hidden by the
  tray or the toggle shortcut, while auto-hide has the panel slid out, in a
  contrast theme, and after a change of the `position` setting until the UI
  reports the panel on the new edge.
- Log lines at `info`, once per change of the effective look, say what the
  app asked for: `backdrop: css (the built-in look)`, `backdrop: acrylic
  requested under the primary dock's panel (prototype; …)` or `backdrop:
  acrylic refused: a contrast theme is on; …`. The platform's lines give
  the result: `native backdrop applied: Acrylic (DWMSBT_TRANSIENTWINDOW) on
  window … under dock window … (layered click-through, frame extended)`
  once per material, or, once per process, `native backdrop refused:
  Windows build N has no DWMWA_SYSTEMBACKDROP_TYPE (it needs build 22621);
  …` (the build is `CurrentBuildNumber` in the registry). When DWM rejects
  a material, its line is a `warn`. Show, hide, restack and region details
  are `debug`.

Where it differs from the design of [option 6](backdrop-window.md):

- **Threads.** The window runs on a platform thread of its own
  (`stem-platform-backdrop`, with its own `GetMessageW` loop), not on the
  main thread. Nothing requires the dock's thread: `SetWindowPos` with
  another thread's window as `hWndInsertAfter` moves only the backdrop, and
  every signal between the threads (requests, restacks) is a posted thread
  message, so neither thread waits on the other. The first request starts
  the thread without waiting for it (the caller can be a command handler on
  the main thread). What is lost is the synchronous step between a move of
  the dock and the backdrop's move; at rest nothing moves. If step 11 shows
  the backdrop out of place after a z-order change, or steps 7 and 8 show
  more lag than the IPC path explains, the main-thread variant is the next
  thing to try, and the ADR records the choice.
- **Reveal.** The UI reports only the start of the slide in
  (`set_dock_auto_hidden(false)`), and the prototype adds no IPC. So the
  material comes back 200 ms after that report (`REVEAL_DELAY`, the UI's
  `AUTO_HIDE_SLIDE_MS`), unless a newer report comes first. It goes at once
  when the slide out starts.
- **Size.** At rest only: a magnified panel grows past the material until
  it settles, and an item animation shows the old box until the panel is
  measured again. When the window grows for a drag or shrinks back after
  the drop, the stored panel rect belongs to the old viewport (on `bottom`
  the window keeps its bottom edge, so its origin moves by the growth). The
  material is then hidden until the UI reports the panel in the new
  viewport, which it does after every resize. This gives a few frames
  without material instead of a panel-shaped piece hundreds of px above or
  below the panel. A report that does not fit inside the viewport's height
  (one measured before a resize and handled after it) is not used either.
  If the panel's rect comes out unchanged after a resize, the UI sends no
  report, and the material stays hidden until the panel next changes; only
  a width change of a CSS px or two can do that.
  The live box needs the IPC change listed under
  [Remaining work](remaining-work.md).
- **Fullscreen** has no path of its own: the UI slides the panel out in
  front of a fullscreen app, and the backdrop follows the slide.
- **Two switches** for the manual test, read once when the thread starts
  and named in the `applied` line. `STEM_BACKDROP_LAYERED=0` creates an
  ordinary window that returns `MA_NOACTIVATE` for `WM_MOUSEACTIVATE`, the
  fallback for A4. That window is never activated, but it takes any click
  that reaches it, since `HTTRANSPARENT` passes a click on only to windows
  of its own thread. `STEM_BACKDROP_EXTEND_FRAME=0` skips
  `DwmExtendFrameIntoClientArea` (A2). Any other value keeps the default.
- Two messages the design did not list: for `WM_GETMINMAXINFO` the window
  lowers the minimum tracking size a captioned window has, to 1 × 1 px, and
  for `WM_DPICHANGED` it returns 0 without the resize to the suggested rect
  (the app sends the panel again at the new scale).
- The page still draws its CSS tint (72 % by default) over the material:
  lower *Panel opacity* to see the material. The UI change under
  [Remaining work](remaining-work.md) removes it.
