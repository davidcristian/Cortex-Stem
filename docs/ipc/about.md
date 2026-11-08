# About

The settings page ends with an *About* section: the version, the locations of the settings file
and the log file, and a button beside each that opens its folder in Explorer. It helps when
testing and when something has gone wrong. The facts come from Rust (the app layer resolved the
paths at startup); the UI shows them and never parses them.

- `get_app_info` → `AppInfo { version, settingsPath, logPath, platform }`.
  - `version` is the app crate's package version (`0.1.0`).
  - `settingsPath` is the settings file the service reads and writes
    (`%APPDATA%\dev.davidcristian.stem\settings.json`), or a note in parentheses when nothing is
    saved in this run: `(demo: not saved)` under the mock platform, `(not saved this session)`
    when the profile could not be used (see `start_service` in `src-tauri/src/lib.rs`).
  - `logPath` is the log file (`%LOCALAPPDATA%\dev.davidcristian.stem\logs\stem.log`) while file
    logging is on in this run, else `""`. `system.fileLogging` is read once at startup, so a
    change shows here after the next start.
  - `platform` is `Platform::name()`: `windows`, or `mock` in a demo run (the page says so under
    the version).

  The facts do not change during a run. The page requests them once as it starts, alongside
  `get_settings`. A failure leaves the rest of the page working, with a notice in the section.
- `open_app_folder { folder }`: `logs` opens the log folder. While no log folder exists yet (file
  logging has never been on), it opens the parent, the app's local data folder, and one info line
  in the log says so. `settings` opens the settings file's folder. Rust picks the folder and
  passes its path to `Platform::launch` as a `LaunchTarget::Uri` (`ShellExecuteExW`, which opens
  a folder in Explorer), on Tauri's blocking pool. The command resolves once the shell has
  accepted the request. It rejects with a reason when there is nothing to open (`settings are not
  saved to a file in this run`, `the log folder is unknown on this host`, a folder that does not
  exist) or when the shell returned an error. The page shows a rejection as the section's own
  notice (`Couldn't open the folder: …`); the save status is not involved. With `logPath` empty,
  the log row says *File logging is off* and its button is disabled.
- `MockBackend.getAppInfo()` returns `version: '0.1.0-mock'`, the two paths that a real
  installation has for the user `me`, and `platform: 'mock'`. `setAppInfo(partial)` changes the
  result for tests (`{ logPath: '' }` is a run with file logging off). `openAppFolder` records
  `{ type: 'openAppFolder', folder }` in `calls` (a browser opens no Explorer).
