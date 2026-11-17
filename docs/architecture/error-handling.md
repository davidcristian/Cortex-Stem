# Error handling

This file lists the errors that each layer of Stem returns, and how a crash is reported at
the next start.

- `stem-core`: no I/O, so its only error type is `SettingsError`.
- `stem-platform`: `PlatformError` (`Unsupported(feature)`,
  `WindowNotFound`, `AccessDenied`, `Timeout`, `Os{code,message}`, `Other`).
  Windows code never panics; `windows::core::Error` converts through
  `From`.
- `stem-service`: `ServiceError` wraps `PlatformError`, `SettingsError` and
  `StoreError`, and adds the command-level errors (`UnknownItem`,
  `NotLaunchable`, `NotPinned`, `Pinned`, `UnknownApp`) and `Thread` (a
  service thread that could not start).
- `stem-app`: commands return `Result<T, String>`. The UI logs the error
  and, where it matters, shows a transient notice.

## Crashes

`src-tauri/src/crash.rs` reports crashes through two marker files in
`%LOCALAPPDATA%\dev.davidcristian.stem\` (the logs' parent folder, next to
`taskbar.json`):

- The panic hook first writes the crash marker `crash.json`
  (`{"kind": "panic", "message", "location", "time"}`), then logs the
  message, location and backtrace. The hook has no `AppHandle`, so `setup`
  resolves the folder once into a `OnceLock`; a panic before that writes
  nothing, just as it logs nothing. Release builds have `panic = "abort"`,
  so the hook is the last code that runs.
- The run marker `run.json` (`{"startedAt", "pid", "version"}`) is written
  right after logging is up, and deleted at `RunEvent::Exit`, after the
  taskbar restore.

At the next start the pure `crash::markers::assess` reads both. A crash marker means
the previous run panicked. 2 s later (so the dock is on screen first), a
task on Tauri's blocking pool shows one native, topmost *Yes* / *No* box
(`Platform::ask_yes_no`) with the time, location and message, and offers to
open the log folder (`Platform::launch` on the folder path; *Yes* opens
Explorer). The box is a `MessageBoxW` without an owner window, because the
dock is `WS_EX_NOACTIVATE` and must not own an activating dialog. It blocks
until the user answers, which is why it runs on the pool. The marker is then
deleted whatever the answer, so each crash is reported once. A run marker
alone, from another process id, means a kill, a power loss or a sign-out.
It is logged once at `warn` and not shown. Under the mock platform nothing
is written and nothing is checked. There is no "report an issue" link.
