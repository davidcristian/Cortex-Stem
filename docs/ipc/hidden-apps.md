# Hidden apps (v1.23)

This file describes how a running app is hidden from every dock and shown again.

- `Settings.hiddenApps: HiddenApp[]`, with `HiddenApp { id: AppId, name: string }`, is empty by
  default. A running app whose id is listed has no item on any dock. Its windows are still
  tracked: `DockState.foreground` and the fullscreen and overlap facts are unchanged, and no item
  is active for them. A pinned app is never hidden: `Settings::validate` drops a listed id that
  is also pinned, empty ids and duplicates.
- `hide_app { appId }`: the service adds `{ id, name }` and saves the settings.
  - `name` is what the app's item shows at that moment: its display name, else its first shown
    window's title (the id's file name when that is blank).

  `hide_app` rejects an app that is pinned or that the engine has no record of. An app that is
  already hidden is not an error, and nothing is saved. `unhide_app { appId }` removes the entry
  and saves; an id that is not listed is not an error. Both reach the pages as any settings
  change does (`dock://settings`, then a new `dock://state`).
- Only these two commands, the pin commands (a pin removes the app from the list) and
  `reset_settings` change `hiddenApps`. `update_settings` ignores the `pinned` and `hiddenApps`
  it is sent and keeps Rust's lists (v1.26). So a settings-page document built before a hide or a
  pin from the dock cannot undo it, whenever it arrives. `Settings::validate` still applies to
  the lists that a settings file contains at start.
- The item menu of a running, unpinned app has *Hide from Stem* after *Pin to Stem* (with no
  confirmation). The settings page lists the hidden apps by their saved names, each with a *Show*
  button, or says that there are none.
- `MockBackend` implements both commands against its own settings. No app is hidden in the demo.
