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

The webview is not granted the plugins' own commands (`plugin:autostart|*`,
`plugin:global-shortcut|*`).
