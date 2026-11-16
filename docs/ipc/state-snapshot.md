# The state snapshot

This file describes two parts of `DockState`, the snapshot that `get_dock_state` returns and the
[`dock://state`](events.md) event sends: the readings that no item shows, and the per-dock facts
in `DockState.docks`.

## Readings no item shows (v1.31)

`DockState.power` and `DockState.volume` always hold the platform's last reading. A new reading
alone changes the snapshot only while the item that draws it is shown:

- `power`: the battery item, so not while `items.battery` is off, nor on a PC without a battery;
- `volume`: the volume item, so not while `items.volume` is off.

Otherwise the engine stores the reading with no new revision and no `dock://state`. The next
snapshot includes it, whether it is sent for another reason or returned by `get_dock_state`, and
an item that is switched on shows it at once. A reading that adds or removes the battery item
(`hasBattery`) changes the items and is sent. Nothing else on the pages reads the two fields: the
settings page reads only `items`. The app sends every dock window the same snapshot, so there is
no per-dock comparison. Two snapshots of one revision can therefore differ in a reading that no
item shows. The page keeps the later one it receives (an equal `revision` is not stale).

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
