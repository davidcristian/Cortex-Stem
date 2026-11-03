# Settings

This file describes how Stem stores its settings.

`Settings` (in `stem-core`) is the whole persisted document. It is versioned
(`SETTINGS_VERSION`) and uses `#[serde(default)]` everywhere, so partial or
old files load. The service validates (clamps) it on load and on every
update. It writes atomically (`FileSettingsStore`: temp file, then rename)
to `<app config dir>/settings.json` (`%APPDATA%\dev.davidcristian.stem\`).

`STEM_PLATFORM=mock` uses an in-memory store, so demo runs never touch a
real profile.
