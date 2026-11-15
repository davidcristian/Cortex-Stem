# IPC contract changelog

This file lists every version of the [IPC contract](index.md), newest first. A section name links
to the file that covers that section now.

- **v1.24 (scrolling over an item):** `Settings.behavior.scrollAction` (`ScrollAction`: `none`,
  `cycleWindows`) and `scroll_item { itemKey, direction: ScrollDirection }`. New section
  [Scrolling over an item](scrolling-over-an-item.md).
- **v1.23 (hidden apps):** `Settings.hiddenApps` (`HiddenApp { id, name }`), `hide_app` and
  `unhide_app`, and *Hide from Stem* in an unpinned app's menu. New section
  [Hidden apps](hidden-apps.md).
- **v1.22 (clock and battery items):** `Settings.items.clock` and `Settings.items.battery`,
  `DockItemKind` `clock` and `battery` (item keys `fixed:clock`, `fixed:battery`; icon keys
  `shell:clock`, `shell:battery`), `DockState.power: PowerStatus` from the new
  `PlatformEvent::PowerChanged`, and `ShellAction` `openDateTimeSettings` and
  `openPowerSettings`. New section
  [Clock and battery items](widget-items.md#clock-and-battery-items-v122). The
  [Context menus](context-menus.md) heading, missing since v1.18, is restored.
- **v1.21 (quick settings and notification items):** Two optional fixed items.
  `Settings.items.quickSettings` and `Settings.items.notificationCenter` (both default `false`)
  add `DockItemKind` `quickSettings` and `notificationCenter`, with icon keys
  `shell:quickSettings` and `shell:notificationCenter` (drawn by the page, like the other
  `shell:` keys). They are placed in the trailing group before the Recycle Bin, in that order. A
  click runs `ShellAction` `openQuickSettings` (Win+A) or `openNotificationCenter` (Win+N), which
  the platform sends as a key chord (`SendInput`). Their menus follow the Task View item's.
- **v1.20 (themes):** `Settings.appearance.theme` (`ThemePreset`: `standard`, `solid`,
  `minimal`), and the user's `dock.css`: `get_user_css`, `reload_user_css`, `open_user_css`,
  `UserCss` and the `dock://user-css` event. New section [Themes](themes.md).
- **v1.19 (keyboard entry):** `Settings.behavior.focusShortcut` (default `Ctrl+Alt+Shift+D`),
  the `dock://focus` and `dock://release-focus` events and the `release_dock_focus` command. New
  section [Keyboard entry](keyboard-entry.md).
- **v1.18 (report provenance):** `edge` on `set_dock_content_size`, `set_dock_hit_region` and
  `set_dock_panel_rect`, and `viewport` on `set_dock_panel_rect`. Rust drops a report measured
  for another edge and re-anchors a panel rect measured in an older viewport, so the engine's
  panel, the reserved band and the backdrop no longer read an outdated report against a resized
  window. New section [Report provenance](report-provenance.md).
- **v1.17 (reserve screen space):** `Settings.system.reserveScreenSpace` takes effect
  ([Settings the app shell applies](app-shell-settings.md)). With auto-hide off, each visible
  dock window reserves a band of its panel's thickness plus margin at its edge, and the window's
  edge side lies on the band's edge side instead of the work area's edge. Settings payload only;
  no command or event changed its name, arguments or payload.
- **v1.16 (screen edges):** `Settings.position` takes effect: `top`, `left` and `right` join
  `bottom`. The [Sizing protocol](sizing-protocol.md) is rewritten for an edge: the window spans
  its work area's length along the edge, its thickness comes from `set_dock_content_size`'s
  `height` on a horizontal dock and its `width` on a vertical one (the other value is
  informational), the panel is anchored to the window's edge side, and a position change moves
  every dock window to the new edge. New subsection
  [Layout on each edge](sizing-protocol.md#layout-on-each-edge-the-page).
  [Hit region](hit-region.md): the reveal strip lies along the window's edge side, and the
  fullscreen exception compares the work area with the bounds on the dock's own edge.
  [File drags](file-drags.md): the envelope and the gap work along the main axis. Toasts rest on
  the side away from the edge and, while the panel is slid out, just inside the edge side.
  `MockBackend` converts the panel rect for the configured edge and takes
  `MockOptions.viewportWidth`. No command or event changed its name, arguments or payload.
- **v1.15 (clicks per dock):** `primary_click` and `show_item_menu` act on the calling dock's own
  view of the windows. With `Settings.monitors = all` and
  `Settings.behavior.showOnlyCurrentMonitor` on, a click activates, minimises or cycles among the
  windows on that dock's monitor only (a pinned app with none there is launched), and the item's
  menu lists, and *Close all windows* closes, only those windows. With `monitors = primary`, the
  setting off or the dock's monitor not yet known, nothing changes. New bullet "Clicks and menus
  use the dock's own view" in [Dock windows per monitor](windows.md#dock-windows-per-monitor);
  [Context menus](context-menus.md) says which windows. The mock's `primaryClick` narrows the
  same way for its page. No command or event changed its arguments or payload (`primary_click`
  reads the calling window, which Tauri injects).
- **v1.14 (attention per window):** `WindowInfo.needsAttention` added: the window asked for
  attention (taskbar flash) and was not activated since; it is never set on the foreground
  window. The engine fills it from its per-window flag, and `DockItem.needsAttention` stays "any
  window of the item". `dock://state` now also fires when the flag moves between windows of an
  item whose own flag does not change. The per-monitor filter
  ([Dock windows per monitor](windows.md#dock-windows-per-monitor)) takes an item's attention
  from the windows left on the page's monitor, instead of keeping it while any window remains.
  The mock engine mirrors the field. No command or event changed its arguments.
- **v1.13 (one dock per monitor):** `DockState.fullscreen` and `DockState.overlapped` are
  replaced by `DockState.docks: DockFacts[]` (`{ monitor, fullscreen, overlapped }`, one entry per
  monitor with a dock window). `dock://state` fires when an entry's facts change or entries are
  added or removed. `dock-<monitorId>` windows (`dock.html?monitor=<id>`) exist with
  `Settings.monitors = all`. `set_dock_panel_rect` registers the calling window's panel per
  window, with its monitor. The [startup sequence](events.md#startup-sequence-ui-side) fetches
  `get_monitors`. New section [Dock windows per monitor](windows.md#dock-windows-per-monitor),
  and the per-dock reading of [Dock panel](dock-panel.md). `Settings.monitors = all` and
  `Settings.behavior.showOnlyCurrentMonitor` take effect in the settings window. The mock gains
  the `twoMonitors` scenario, `MockOptions.monitor`, `setFullscreen(active, monitor?)` and
  `dockPanel(label?)`. New payload type `DockFacts`. No command changed its arguments.
- **v1.12 (about):** `get_app_info` → `AppInfo` and `open_app_folder { folder: AppFolder }`
  added (`Backend.getAppInfo`, `Backend.openAppFolder`). The settings page's About section shows
  the version, the settings file and the log file, and opens their folders in Explorer; new
  section [About](about.md). New payload types `AppInfo` and `AppFolder` (`logs` | `settings`).
  The mock returns demo facts (`setAppInfo` for tests) and records the folder requests. No other
  command or event changed.
- **v1.11 (toggle while hidden):** `set_dock_auto_hidden { hidden }` added
  (`Backend.setDockAutoHidden`): the dock UI reports when auto-hide slides its panel out and
  back. `dock://show` event added (payload `null`, to one dock window): `toggle_dock` asks the UI
  for the panel instead of hiding a window whose panel is slid out. New section
  [Toggle while hidden](toggle-while-hidden.md). The mock records the call, and `requestShow()`
  emits the event. No other command or event changed.
- **v1.10 (when overlapped):** `set_dock_panel_rect { rect }` added (`Backend.setDockPanelRect`):
  the dock window's resting panel in CSS px, whatever the auto-hide phase; new section
  [Dock panel](dock-panel.md). `DockState.overlapped: boolean` added: the engine's result, `true`
  while a window of another process covers that panel. It is sent in `dock://state`, which now
  also fires when it changes. With `autoHide.mode = whenOverlapped` the UI hides the dock after
  the delay while it is set. The mocks' demo windows now have `rect`s (`MockBackend.moveWindow`,
  `dockPanel`). No other command or event changed.
- **v1.9 (fullscreen):** `DockState.fullscreen: boolean` added: the platform's result, `true`
  while a fullscreen app covers the screen. It is sent in `dock://state`, which now also fires
  when it changes; the UI hides the dock while it is set, whatever `autoHide.mode` says.
  `RawWindow.rect: Rect | null` added (the window's frame in physical screen pixels; `null` when
  unknown, and in the mocks' demo windows). No command or event changed.
- **v1.8 (Recycle Bin drops):** `delete_to_recycle_bin { paths }` added
  (`Backend.deleteToRecycleBin`): a file drop on the Recycle Bin item highlights it instead of
  opening the pin gap, and moves every dropped path to the bin; [File drags](file-drags.md)
  describes it. No event or payload type changed.
- **v1.7 (file drops):** `Backend.onFileDrag` added over Tauri's own `tauri://drag-*` webview
  events, with positions converted to CSS px in `TauriBackend`; new section
  [File drags](file-drags.md). No command, event or payload type of this contract changed.
- **v1.6 (auto-hide):** The planned `dock://visibility` event is dropped: ADR-0008 keeps
  auto-hide in the UI, which reveals the dock through the hit region (a 2 px strip while hidden).
  The [Hit region](hit-region.md) section is updated; no command or payload changed.
- **v1.5 (autostart and toggle shortcut):** `Settings.behavior.toggleShortcut` added (settings
  payload only, no new command or event), and `Settings.system.autostart` is now applied; both
  are described in [Settings the app shell applies](app-shell-settings.md).
- **v1.4 (click-through setting):** `Settings.behavior.clickThrough` added (settings payload
  only, no new command or event). It replaces the `STEM_HIT_REGION` environment variable, which
  was removed.
- **v1.3 (hit region):** `set_dock_hit_region` added. The [Sizing protocol](sizing-protocol.md)
  now gives the dock window the work area's full width, so item animations never resize it
  (`set_dock_content_size` keeps its signature; `width` is informational).
- **v1.2 (native context menus):** `show_item_menu` and `show_dock_menu` move from planned to
  required. The [Context menus](context-menus.md) section defines the coordinate space and the
  menu contents. `set_dock_hover` stays planned.
- **v1.1 (shell actions and settings):** `shell_action` and `open_settings` move from planned to
  required, and `reset_settings` is added (the mock stubs them).
- **v1:** the initial contract.
