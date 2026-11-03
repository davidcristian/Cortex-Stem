# Settings

This file describes how Stem stores its settings.

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
