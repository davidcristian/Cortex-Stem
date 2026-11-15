# Dock panel

A dock's `overlapped` fact ([The state snapshot](state-snapshot.md#dockstatedocks)) needs the
place where its panel rests, and the [hit region](hit-region.md) cannot give it. While the dock
is hidden, the hit region is the 2 px strip, which no window could overlap, so the dock would
show and hide in a loop. So the UI reports the resting panel separately:

- `set_dock_panel_rect({ rect })`: the panel's bounding rect **at rest** (where it is when
  shown, without the auto-hide slide), as integers in CSS px relative to the calling window's
  viewport, like the hit region's `panel`. The UI sends it whenever that rect changes: at
  startup, when items are added or removed, for the icon size, separators, and the window growing
  for a drag (the rect then follows the panel's place in the grown viewport). It sends it
  whatever the auto-hide phase, never per magnification frame. Only dock windows may call it; a
  negative size rejects.
- Rust converts it once per report into physical pixels of the virtual desktop, the space of
  `RawWindow.rect`. It applies the window's scale factor (rounded outwards like the hit region)
  and adds the window's position. It passes the result to the engine with the calling window's
  label and the monitor the app layer placed that window on
  (`DockService::set_dock_panel(label, monitor, rect)`).
  - Rust converts the latest report again whenever it moves the dock window itself (a monitor or
    work-area change, which moves the window without changing the UI's viewport) and when the
    window is shown again.
  - It tells the engine there is no rect while the tray or the toggle shortcut hides the window,
    and passes neither monitor nor rect once the window is closed (a monitor unplugged, the
    setting back to the primary), which makes the engine drop the label.

  The engine stores the panels by label (`DockEngine::dock_panel(label)`), computes each dock's
  `overlapped` again from its rect and the window frames, and publishes one `docks` entry per
  monitor with a registered window. A window registered without a monitor (the app had no
  monitor list yet) has no entry until it is placed.
- `MockBackend` records the call (`calls` entry `setDockPanelRect`) and converts it as the app
  layer would for a dock window flush with its edge (`Settings.position`, v1.16) of its monitor's
  work area, at that monitor's scale (`panelOnScreen`). On `bottom` the viewport's bottom-left
  corner is the work area's bottom-left corner; on `top` and `left` its top-left corner is the
  work area's top-left corner; on `right` its top-right corner is the work area's top-right
  corner. The viewport is `window.innerWidth` × `window.innerHeight`, or
  `MockOptions.viewportWidth` / `MockOptions.viewportHeight`, or the work area's size where there
  is no `window`. One mock serves one page, so it registers the panel under that page's dock
  window (`MockOptions.monitor`, the page's `?monitor=`; `pageLabel`).

  The mock's demo windows have frames on their monitor, none within the bottom 250 px of the
  work area, where the panel rests. `moveWindow(id, rect)` moves one window (a rect-only
  `WindowUpdated`), and `dockPanel(label?)` returns the engine's panel rect for that dock (this
  page's by default).
