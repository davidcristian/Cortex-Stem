# Commands (UI → Rust)

These are the commands that the pages call. [Principles](index.md#principles) says how their
arguments are named and how a failed command rejects.

| Command                 | Arguments                                   | Returns            | Semantics |
|-------------------------|---------------------------------------------|--------------------|-----------|
| `get_dock_state`        | none                                        | `DockState`        | the current snapshot |
| `get_settings`          | none                                        | `Settings`         | |
| `update_settings`       | `{ settings: Settings }`                    | `Settings`         | validates and persists the settings; emits `dock://settings`, and `dock://state` if the items change |
| `get_theme`             | none                                        | `ThemeInfo`        | |
| `get_monitors`          | none                                        | `MonitorInfo[]`    | |
| `primary_click`         | `{ itemKey: string }`                       | `void`             | the engine chooses the action (launch, activate, minimise, cycle windows or a shell action) among the windows the calling dock window lists (see [Dock windows per monitor](windows.md#dock-windows-per-monitor)) |
| `scroll_item`           | `{ itemKey: string, direction: ScrollDirection }` | `void` | the wheel turned one notch over an item of the calling dock window. Rust chooses what it does (see [Scrolling over an item](scrolling-over-an-item.md), v1.24). |
| `window_action`         | `{ windowId: WindowId, action: WindowAction }` | `void`          | a direct action on one window (the window list in a menu, a middle click, and so on) |
| `launch_app`            | `{ appId: AppId }`                          | `void`             | starts a new instance; emits `dock://item-bounce` |
| `pin_app`               | `{ appId: AppId }`                          | `void`             | pins a running app (the app needs a known launch target) |
| `pin_target`            | `{ target: LaunchTarget }`                  | `AppId`            | pins a dropped or chosen file, shortcut or Store app; returns the canonical id |
| `unpin_app`             | `{ appId: AppId }`                          | `void`             | |
| `hide_app`              | `{ appId: AppId }`                          | `void`             | stops showing a running, unpinned app on every dock (see [Hidden apps](hidden-apps.md), v1.23) |
| `unhide_app`            | `{ appId: AppId }`                          | `void`             | shows a hidden app again (see [Hidden apps](hidden-apps.md)) |
| `reorder_pinned`        | `{ order: AppId[] }`                        | `void`             | the new order of the pinned apps; pinned ids that are not listed keep their relative order at the end |
| `get_icon`              | `{ iconKey: IconKey, size: number }`        | `string \| null`   | a PNG as a `data:` URL; `null` for `shell:*` keys and unknown keys |
| `set_dock_content_size` | `{ width: number, height: number, edge: DockPosition }` (CSS px)| `void` | the UI's content size; Rust sizes the calling window (see [Sizing protocol](sizing-protocol.md); `edge`: see [Report provenance](report-provenance.md)) |
| `set_dock_hit_region`   | `{ panel: Rect, envelope: Rect, edge: DockPosition }` (CSS px, window-local) | `void` | where the calling dock window accepts clicks (see [Hit region](hit-region.md); `edge`: see [Report provenance](report-provenance.md)) |
| `set_dock_panel_rect`   | `{ rect: Rect, edge: DockPosition, viewport: { width: number, height: number } }` (CSS px, window-local) | `void` | where the calling dock window's panel rests, whatever the auto-hide phase. Rust registers it per window, with the window's monitor, for that dock's `docks` entry (see [Dock panel](dock-panel.md); `edge` and `viewport`: see [Report provenance](report-provenance.md)) |
| `set_dock_auto_hidden`  | `{ hidden: boolean }`                       | `void`             | auto-hide slid the calling dock window's panel out (`true`) or brought it back (`false`); dock windows only (see [Toggle while hidden](toggle-while-hidden.md)) |
| `shell_action`          | `{ action: ShellAction }`                   | `void`             | runs a shell action directly (menus, the Recycle Bin's "Empty") |
| `delete_to_recycle_bin` | `{ paths: string[] }`                       | `void`             | moves files and folders to the Recycle Bin, for a file drop on its item (see [File drags](file-drags.md)); dock windows only; resolves when the shell has finished |
| `open_settings`         | none                                        | `void`             | opens the settings window, or focuses it if it is already open |
| `reset_settings`        | none                                        | `Settings`         | restores and persists the defaults; emits `dock://settings` |
| `quit`                  | none                                        | none               | exits the application |
| `get_app_info`          | none                                        | `AppInfo`          | the version, the settings file, the log file and the platform name, for Settings → About (see [About](about.md)) |
| `open_app_folder`       | `{ folder: AppFolder }`                     | `void`             | opens the log folder (`logs`) or the settings file's folder (`settings`) in Explorer (see [About](about.md)); rejects when there is none |
| `show_item_menu`        | `{ itemKey: string, x: number, y: number }` | `void`             | shows the native context menu of one item at `(x, y)` (see [Context menus](context-menus.md)); resolves when the menu has closed |
| `show_dock_menu`        | `{ x: number, y: number }`                  | `void`             | shows the dock's own menu (on the panel background): *Stem settings…*, *Quit Stem* |
| `release_dock_focus`    | none                                        | `void`             | the calling dock window gives the keyboard focus back (on Escape, or in response to `dock://release-focus`). Rust activates again the window that was in front when the focus shortcut was pressed, if it still exists; dock windows only (see [Keyboard entry](keyboard-entry.md), v1.19) |
| `get_user_css`          | none                                        | `UserCss`          | the user's `dock.css` as read at start or by the last `reload_user_css` (see [Themes](themes.md), v1.20) |
| `reload_user_css`       | none                                        | `UserCss`          | reads `dock.css` again; emits `dock://user-css` to every dock window (see [Themes](themes.md)) |
| `open_user_css`         | none                                        | `void`             | creates `dock.css` from the template if it does not exist, then opens it with the default app, or with Notepad when that fails (see [Themes](themes.md)); rejects when neither opens it |

One command is planned and not implemented in Rust: `set_dock_hover { hovering }`. It is listed
so that the mock can stub it: `Backend.setDockHover` is optional, `MockBackend` records it,
`TauriBackend` does not have it, and the dock page calls it only where it exists.
