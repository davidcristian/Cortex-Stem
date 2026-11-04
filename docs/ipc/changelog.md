# IPC contract changelog

This file lists every version of the [IPC contract](index.md), newest first. A section name links
to the file that covers that section now.

- **v1.8 (Recycle Bin drops):** `delete_to_recycle_bin { paths }` added
  (`Backend.deleteToRecycleBin`): a file drop on the Recycle Bin item highlights it instead of
  opening the pin gap, and moves every dropped path to the bin; [File drags](file-drags.md)
  describes it. No event or payload type changed.
- **v1.7 (file drops):** `Backend.onFileDrag` added over Tauri's own `tauri://drag-*` webview
  events, with positions converted to CSS px in `TauriBackend`; new section
  [File drags](file-drags.md). No command, event or payload type of this contract changed.
- **v1.6 (auto-hide):** The planned `dock://visibility` event is dropped: ADR-0008 keeps
  auto-hide in the UI, which reveals the dock through the hit region (a 2 px strip while hidden).
  The [Hit region](hit-region.md) section is updated; no command or payload changed.
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
