# Hidden apps (v1.23)

This file describes how a running app is hidden from every dock and shown again.

- `Settings.hiddenApps: HiddenApp[]`, with `HiddenApp { id: AppId, name: string }`, is empty by
  default. A running app whose id is listed has no item on any dock. Its windows are still
  tracked: `DockState.foreground` and the fullscreen and overlap facts are unchanged, and no item
  is active for them. A pinned app is never hidden: `Settings::validate` drops a listed id that
  is also pinned, empty ids and duplicates.
- `hide_app { appId }`: the service adds `{ id, name }` and saves the settings.
  - `name` is the app's display name at that moment.

  `hide_app` rejects an app that is pinned or that the engine has no record of.
  `unhide_app { appId }` removes the entry and saves; an id that is not listed is not an error.
  Both reach the pages as any settings change does (`dock://settings`, then a new `dock://state`).
- The item menu of a running, unpinned app has *Hide from Stem* after *Pin to Stem* (with no
  confirmation). The settings page lists the hidden apps by their saved names, each with a *Show*
  button, or says that there are none.
- `MockBackend` implements both commands against its own settings. No app is hidden in the demo.
