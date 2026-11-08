# The state snapshot

This file describes a part of `DockState`, the snapshot that `get_dock_state` returns and the
[`dock://state`](events.md) event sends: the per-dock facts in `DockState.docks`.

## `DockState.docks`

`DockState.docks` lists the per-dock facts. It has one `DockFacts { monitor, fullscreen,
overlapped }` for each monitor that has a dock window registered with the engine by the app layer
([Dock panel](dock-panel.md)), sorted by monitor id. A dock page finds its own entry by the
monitor it is on ([Dock windows per monitor](windows.md#dock-windows-per-monitor)). A dock
without an entry yet takes both facts as `false`. A page that cannot tell its monitor yet takes
the only entry, if there is exactly one. The two facts are independent, and the UI combines them:

- **`fullscreen`:** `true` when the platform reports that the foreground window covers the whole
  of this dock's monitor (a game, a browser after F11, a video in fullscreen, a slideshow). The
  platform computes it from `SHQueryUserNotificationState` and from the window's frame against
  its monitor's bounds (`rules::fullscreen`). It computes it again when the foreground changes
  and when the foreground window moves, and sends it only when it changes, together with the
  window's monitor (`MonitorFromWindow`). The engine sets the fact on the dock of that monitor,
  or on the primary monitor's dock when the report names no monitor (the shell's state alone, a
  NULL foreground). While the fact is set, the UI hides that dock whatever `autoHide.mode` says.
  It shows the dock again when the fact clears, unless the mode is `always` and the dock was
  hidden anyway. The reveal strip works in the meantime ([Hit region](hit-region.md)). There is
  no `dock://visibility` event (v1.6): the fact is sent in the snapshot, as the foreground is.
- **`overlapped`:** a window of another process covers this dock's resting panel. That window
  is visible, not minimised, not cloaked (hidden by DWM), and has a frame (`RawWindow.rect`) that
  intersects, with a positive area, the panel rect that the dock's page reported through
  `set_dock_panel_rect` ([Dock panel](dock-panel.md)), converted to screen pixels. The engine
  computes the fact (`stem_core::overlap`) for each panel after every input that can change it,
  and sends `dock://state` when a dock's flag changes. The fact is `false` while the dock has no
  known panel: before its page's first report, and while the tray or the toggle shortcut hides
  its window. With `autoHide.mode = whenOverlapped` the UI hides that dock `autoHide.delayMs`
  after the fact is set and shows it again when it clears. Other modes keep the fact and ignore
  it.
