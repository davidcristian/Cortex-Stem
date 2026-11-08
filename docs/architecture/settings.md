# Settings

This file describes how Stem stores its settings, and how the app shell applies the ones
that belong to it.

`Settings` (in `stem-core`) is the whole persisted document. It is versioned
(`SETTINGS_VERSION`) and uses `#[serde(default)]` everywhere, so partial or
old files load. The service validates (clamps) it on load and on every
update. It writes atomically (`FileSettingsStore`: temp file, then rename)
to `<app config dir>/settings.json` (`%APPDATA%\dev.davidcristian.stem\`).
A default file is written on first start.

A file the service rejects (invalid JSON or a newer version) is renamed to
`settings.json.broken`; the app then writes fresh defaults and continues
with them. A rejected file that cannot be renamed leaves the session on an
in-memory store.

`system.fileLogging` is read once at startup; a change applies after a
restart. `STEM_PLATFORM=mock` uses an in-memory store, so demo runs never
touch a real profile.

The app shell (`src-tauri`) applies the settings that belong to it on every
`ServiceEvent::Settings` (which arrives on the service worker) and once at
startup. Each one acts only when its own value changed, and none blocks the
worker:

- `behavior.clickThrough` (`windows.rs`): the work is posted to the main
  thread.
- The tray's check mark (`tray.rs`): the same.
- `system.autostart` (`autostart.rs`): `tauri-plugin-autostart`'s Run-key
  entry. It is compared with `is_enabled()` and written only when they
  differ, one sync at a time on Tauri's blocking pool. The mock platform
  leaves it alone.
- `system.hideWindowsTaskbar` (`taskbar.rs`): Windows' own *Automatically
  hide the taskbar* setting, through
  `SHAppBarMessage(ABM_SETSTATE, ABS_AUTOHIDE)`. Stem never hides
  `Shell_TrayWnd`, so the change is reversible and still applies after an
  Explorer restart. Windows persists
  the setting, so the state the user had is kept in a marker file,
  `taskbar.json` under `%LOCALAPPDATA%\dev.davidcristian.stem\`. The marker
  is written before the first change and deleted after the restore. A
  marker found at start means the previous run crashed, and its saved state
  is still the one to restore. The decision is the pure
  `stem_platform::rules::taskbar::plan`. The syncs run like autostart's. The
  restore at `RunEvent::Exit` runs inline because the process is ending,
  and it is bounded: each `SHAppBarMessage` runs on a `stem-taskbar` helper
  thread whose result is awaited for at most 500 ms, so the restore returns
  within about 1 s. A live state that cannot be read (no tray, no reply)
  never writes or deletes the marker. The mock platform keeps the marker
  in memory.
- `behavior.toggleShortcut` (`shortcut.rs`):
  `tauri-plugin-global-shortcut`, which registers with `RegisterHotKey` on a
  hidden main-thread window and makes callers wait for the main thread. So
  a change is posted to the main thread without waiting. There it
  unregisters the old shortcut and registers the new one. A press runs
  `windows::toggle_dock` (what the tray icon's left click runs) in
  `src-tauri/src/windows.rs`.

Both plugins are registered in `setup` rather than in the builder, so a
plugin that fails to start disables only its own feature, and their own
commands are not granted to the webview.
