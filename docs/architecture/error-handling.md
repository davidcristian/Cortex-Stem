# Error handling

This file lists the errors that each layer of Stem returns.

- `stem-core`: no I/O, so its only error type is `SettingsError`.
- `stem-platform`: `PlatformError` (`Unsupported(feature)`,
  `WindowNotFound`, `AccessDenied`, `Timeout`, `Os{code,message}`, `Other`).
  Windows code never panics; `windows::core::Error` converts through
  `From`.
- `stem-service`: `ServiceError` wraps `PlatformError`, `SettingsError` and
  `StoreError`, and adds the command-level errors (`UnknownItem`,
  `NotLaunchable`, `NotPinned`) and `Thread` (a service thread that could
  not start).
- `stem-app`: commands return `Result<T, String>`. The UI logs the error
  and, where it matters, shows a transient notice.
