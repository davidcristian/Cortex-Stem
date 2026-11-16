# Hidden apps (v1.23)

This file describes how a running app is hidden from every dock and shown again.

- `Settings.hiddenApps: HiddenApp[]`, with `HiddenApp { id: AppId, name: string, path: string |
  null, iconKey: IconKey | null }`, is empty by default. `path` and `iconKey` were added in
  v1.29; an entry saved without them still loads, with both `null`. A running app whose id is
  listed has no item on any dock. Its windows are still tracked: `DockState.foreground` and the
  fullscreen and overlap facts are unchanged, and no item is active for them. A pinned app is
  never hidden: `Settings::validate` drops a listed id that is also pinned, empty ids and
  duplicates.
- `hide_app { appId }`: the service adds `{ id, name, path, iconKey }` and saves the settings.
  - `name` is what the app's item shows at that moment: its display name, else its first shown
    window's title (the id's file name when that is blank).
  - `path` (v1.29) is the program's path, in its original case: as the platform reported it for
    the app's first shown window, else for its first-seen window (`DockEngine::app_window`), else
    the file its identity launches (an `exe` or a `shortcut` target).
  - `iconKey` is the icon its item shows: the identity's, else that window's. It is never a key
    tied to one window (`hwnd:`), because Windows reuses the handle once the window is gone. The
    program file's own icon (`exe:` or `file:` of `path`) is used instead when `path` is known,
    else `iconKey` is `null`.

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
  button, or says that there are none. Each row draws `iconKey` through `get_icon` (a neutral tile
  when there is none or when it does not resolve) and shows `path` under the name (v1.29). For
  an entry without a path, the row shows the location read from an `exe:` id (in lower case). A
  packaged app's row shows its package family from the id either way.
- `MockBackend` implements both commands against its own settings, `path` and `iconKey`
  included. No app is hidden in the demo.
