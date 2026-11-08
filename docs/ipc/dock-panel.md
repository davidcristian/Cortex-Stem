# Dock panel

The dock's `overlapped` fact ([The state snapshot](state-snapshot.md#dockstateoverlapped)) needs
the place where its panel rests, and the [hit region](hit-region.md) cannot give it. While the
dock is hidden, the hit region is the 2 px strip, which no window could overlap, so the dock
would show and hide in a loop. So the UI reports the resting panel separately:

- `set_dock_panel_rect({ rect })`: the panel's bounding rect **at rest** (where it is when
  shown, without the auto-hide slide), as integers in CSS px relative to the calling window's
  viewport, like the hit region's `panel`. The UI sends it whenever that rect changes: at
  startup, when items are added or removed, for the icon size, separators, and the window growing
  for a drag (the rect then follows the panel's place in the grown viewport). It sends it
  whatever the auto-hide phase, never per magnification frame. Only dock windows may call it; a
  negative size rejects.
- Rust converts it once per report into physical pixels of the virtual desktop, the space of
  `RawWindow.rect`. It applies the window's scale factor (rounded outwards like the hit region)
  and adds the window's position. It passes the result to the engine.
  - Rust converts the latest report again whenever it moves the dock window itself (a monitor or
    work-area change, which moves the window without changing the UI's viewport) and when the
    window is shown again.
  - It tells the engine there is no rect while the tray or the toggle shortcut hides the window.

  The engine stores the rect (`DockEngine::dock_panel`) and computes `overlapped` again from it
  and the window frames.
- `MockBackend` records the call (`calls` entry `setDockPanelRect`) and converts it as the app
  layer would for a dock window flush with the bottom of its monitor's work area
  (`panelOnScreen`): the viewport's bottom-left corner is the work area's bottom-left corner. The
  viewport height is `window.innerHeight`, or `MockOptions.viewportHeight`, or the work area's
  height where there is no `window`.

  The mock's demo windows have frames on the mock monitor, none within the bottom 250 px of the
  work area, where the panel rests. `moveWindow(id, rect)` moves one window (a rect-only
  `WindowUpdated`), and `dockPanel()` returns the engine's panel rect.
