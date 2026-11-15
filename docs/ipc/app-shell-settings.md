# Settings the app shell applies

Most settings are rendered by the UI or used by the engine. The Tauri layer applies a few itself,
at start and on every settings change (when `dock://settings` is emitted). The UI only edits them
through `update_settings` and receives nothing more than the saved value; the outcome is in the
log, not on IPC:

- `Settings.behavior.clickThrough`: see [Hit region](hit-region.md).
- `Settings.system.autostart` (default `false`): the OS "start at sign-in" entry (the `Stem`
  value under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`) follows it. Rust compares the
  setting with the entry and writes only when they differ, off the main thread. Demo runs
  (`STEM_PLATFORM=mock`) never touch the entry.
- `Settings.behavior.toggleShortcut` (default `"Ctrl+Alt+D"`): a global shortcut that shows or
  hides the dock, exactly like a left click on the tray icon. The format is modifiers (`Ctrl`,
  `Alt`, `Shift`, `Super`) first, then one key, joined by `+`, case-insensitive; `""` means none.
  Unless the key is `F1` to `F24`, it must include `Ctrl`, `Alt` or `Super`, because a global
  shortcut on a plain or shifted key would take that key away from every app. A value that does
  not parse, breaks that rule or is registered by another app is kept in the settings as written
  but registers nothing (with a warning in the log). A change takes effect without a restart.
  The settings window applies the field on Enter or blur, not per keystroke.
- `Settings.system.reserveScreenSpace` (default `false`, ADR-0010): while it is on **and**
  `Settings.autoHide.mode` is `off`, every visible dock window registers as an AppBar (a desktop
  toolbar registered with the shell, `SHAppBarMessage`) on its edge (`Settings.position`) of its
  monitor. The shell then takes a band there out of the monitor's work area, and maximised
  windows stop short of the dock.
  - The band is the panel's resting thickness plus its margin to the edge, from the window's
    latest `set_dock_panel_rect`: the distance from the panel's far side to the viewport's edge
    side, read against the window's viewport when Rust handles the report. A rect that does not
    lie inside that viewport across the edge (one measured before a resize that the page had not
    seen yet) leaves the band as it was. The band does not include the magnification
    headroom. The shell moves the band past the stock taskbar and other bars on that edge.
  - The window keeps its thickness and puts its edge side on the band's edge side instead of on
    the work area's edge ([Sizing protocol](sizing-protocol.md), step 1; with the taskbar visible
    on that edge the two are the same place). So it reaches past the band into the work area,
    transparent and click-through there.
  - A dock that auto-hides reserves nothing, and a window hidden by the tray or the toggle
    shortcut gives its band back until it is shown. Either setting, the position and the monitors
    change the bands without a restart, and quitting gives them back.
  - `MonitorInfo.workArea` (`dock://monitors`) shrinks by the band, so the fullscreen exception of
    the [Hit region](hit-region.md) applies to such a dock: there is no reveal strip while a
    fullscreen window is up.
  - The settings window greys the switch out while auto-hide is on. Demo runs
    (`STEM_PLATFORM=mock`) record the registrations and reserve nothing.
- `Settings.behavior.focusShortcut` (default `"Ctrl+Alt+Shift+D"`): a global shortcut that gives
  the keyboard focus to a dock window ([Keyboard entry](keyboard-entry.md)). It has the same
  format, rules and live re-registration as `toggleShortcut`; the same chord as `toggleShortcut`
  registers nothing.

The webview is not granted the plugins' own commands (`plugin:autostart|*`,
`plugin:global-shortcut|*`).
