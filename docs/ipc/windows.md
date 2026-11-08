# Windows

Each window of the app has a label and loads one page:

| Label              | Page                      | Purpose                         |
|--------------------|---------------------------|---------------------------------|
| `dock`             | `dock.html`               | the dock on the primary monitor. With `Settings.monitors = primary` (the default) it is the only dock window. |
| `dock-<monitorId>` | `dock.html?monitor=<id>`  | with `Settings.monitors = all`, one more dock for each other monitor. `<monitorId>` in the label is the monitor id with every character outside `[A-Za-z0-9_-]` dropped (`\\.\DISPLAY3` → `dock-DISPLAY3`); `<id>` in the URL is the percent-encoded id (see [Dock windows per monitor](#dock-windows-per-monitor) below). |
| `settings`         | `settings.html`           | the settings window. It also reads `get_dock_state`, `dock://state` and `get_icon` for its pinned list, and `get_app_info` and `open_app_folder` for its About section. |

## Dock windows per monitor

`Settings.monitors = all` puts a dock on every monitor. `primary` (the default) keeps the one
`dock` window, and none of the per-monitor behaviour below takes effect.

- **Windows.** The app layer plans the set of windows from the monitor list
  (`windows::plan_docks`): `dock` on the primary monitor (the flagged one, else the first), and
  `dock-<id>` for each other monitor, with every character outside `[A-Za-z0-9_-]` dropped from
  the id (Tauri labels allow `a-zA-Z0-9-/:_`). Two monitors whose ids sanitise to the same label
  get one dock, and a warning names the other monitor.

  The app layer reconciles the windows with the plan at startup, on every `dock://monitors`
  (hot-plug, a resolution or work-area change, a new primary: `dock` moves to it and the others
  follow the plan) and when the setting changes. Missing windows are created, windows whose
  monitor is gone are closed (never `dock`), and the rest are laid out again on their own
  monitor's work area at its scale ([Sizing protocol](sizing-protocol.md)). Each creation and
  each closing logs one info line: `dock window created "<label>" on <monitor>` or `dock window
  closed "<label>" (monitor <id>)`.

  Every command a page sends is routed by its window (its label), as the
  [commands](commands.md) say. `set_dock_hit_region`, `set_dock_content_size`,
  `set_dock_panel_rect` and `set_dock_auto_hidden` name the calling window in this way.
  `toggle_dock` (the tray, the toggle shortcut) decides from the `dock` window's state and
  applies the result to every dock window: all hide, all show, or every page receives
  `dock://show`.
- **The page.** A secondary window's page is `dock.html?monitor=<id>`, with the id
  percent-encoded (`\\.\DISPLAY3` → `monitor=%5C%5C.%5CDISPLAY3`; `URLSearchParams` decodes it).
  The primary's page has no query string and takes the primary monitor of `get_monitors` /
  `dock://monitors` (the same rule as the app layer's). The monitor selects the page's entry in
  `DockState.docks` ([The state snapshot](state-snapshot.md#dockstatedocks)). With
  `Settings.behavior.showOnlyCurrentMonitor`, it also narrows the snapshot's items
  (`src/lib/dock/monitor.ts`, pure and unit-tested):
  - each app item keeps only the windows whose `WindowInfo.monitor` is this monitor;
  - an item left with no windows stays only if it is pinned (fixed items always stay);
  - `isActive` and `needsAttention` follow the remaining windows' `WindowInfo.isActive` and
    `WindowInfo.needsAttention` (v1.14: the engine flags the window that flashed, and the item
    while any of its windows is flagged, so an app flashing on another monitor neither plays the
    attention wiggle on this dock nor reveals it);
  - separators that would then lead, trail or double up are dropped.

  The page decides nothing else: the order, the grouping and what a click does stay in Rust.
  With the setting off, every dock lists everything.
- **Clicks and menus use the dock's own view** (v1.15). `primary_click` and `show_item_menu`
  identify the calling window by its label. With `Settings.monitors = all` and
  `Settings.behavior.showOnlyCurrentMonitor` on, Rust narrows the item's windows to that dock's
  monitor exactly as the page does, in one place (`DockEngine::windows_on_dock`), before it
  chooses the action. The dock's monitor is the one the app layer registered for the label with
  `set_dock_panel_rect`. A click then activates, minimises (`clickActiveMinimizes`) or cycles
  among the windows on this monitor only, and a pinned app with no window on this monitor is
  launched, because the dock shows no window for it. The item's menu lists only those windows,
  and its *Close all windows* closes only them. With `monitors = primary`, with the setting off,
  or for a dock whose monitor Rust has not registered yet (before the page's first
  `set_dock_panel_rect`), every window of the app counts. With `monitors = primary` the one
  dock's page still narrows its list when the setting is on.
- **Mock.** `MockBackend` takes the page's monitor (`?monitor=`, read by `createBackend`;
  `MockOptions.monitor`) and a `twoMonitors` scenario (`?scenario=twoMonitors`): the demo plus
  `\\.\DISPLAY3`, 1920×1080 to the right of the primary at scale 1.5, with a Notepad window (a
  pinned app) and a Paint window (not pinned) on it. `setFullscreen(active, monitor?)` takes the
  monitor of the fullscreen report (`null`: the primary's dock). `moveWindow` works as with one
  monitor. `primaryClick` narrows like Rust for the page's own dock window (`pageLabel`), once
  its `setDockPanelRect` has registered it.
