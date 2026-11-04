# IPC contract changelog

This file lists every version of the [IPC contract](index.md), newest first. A section name links
to the file that covers that section now.

- **v1.5 (autostart and toggle shortcut):** `Settings.behavior.toggleShortcut` added (settings
  payload only, no new command or event), and `Settings.system.autostart` is now applied; both
  are described in [Settings the app shell applies](app-shell-settings.md).
- **v1.4 (click-through setting):** `Settings.behavior.clickThrough` added (settings payload
  only, no new command or event). It replaces the `STEM_HIT_REGION` environment variable, which
  was removed.
- **v1.3 (hit region):** `set_dock_hit_region` added. The [Sizing protocol](sizing-protocol.md)
  now gives the dock window the work area's full width, so item animations never resize it
  (`set_dock_content_size` keeps its signature; `width` is informational).
- **v1.2 (native context menus):** `show_item_menu` and `show_dock_menu` move from planned to
  required. The [Context menus](context-menus.md) section defines the coordinate space and the
  menu contents. `set_dock_hover` stays planned.
- **v1.1 (shell actions and settings):** `shell_action` and `open_settings` move from planned to
  required, and `reset_settings` is added (the mock stubs them).
- **v1:** the initial contract.
