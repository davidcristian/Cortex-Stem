# Events (Rust → UI)

These are the events that the Rust app sends to the pages. [Principles](index.md#principles) says
how a page listens and which pages an event reaches.

| Event                | Payload                         | When                                                         |
|----------------------|---------------------------------|--------------------------------------------------------------|
| `dock://state`       | `DockState`                     | the items, the foreground or a dock's facts (`docks`) changed, an identity was resolved, or settings were applied. [The state snapshot](state-snapshot.md) describes the per-dock facts. |
| `dock://settings`    | `Settings`                      | settings were validated and persisted, whatever their source  |
| `dock://theme`       | `ThemeInfo`                     | at start, and when the OS theme or accent colour changes      |
| `dock://monitors`    | `MonitorInfo[]`                 | at start, and when the display topology changes               |
| `dock://item-bounce` | `{ itemKey: string }`           | an app was launched from the dock; the page plays the bounce  |
| `dock://show`        | `null`                          | to one dock window: a tray click or the toggle shortcut requested the dock while auto-hide had slid its panel out. The page brings the panel back (see [Toggle while hidden](toggle-while-hidden.md)). |
| `dock://focus`       | `null`                          | to one dock window: the focus shortcut gave it the keyboard focus. The page focuses the toolbar (see [Keyboard entry](keyboard-entry.md), v1.19). |
| `dock://release-focus` | `null`                        | to one dock window: the focus shortcut was pressed again while that dock still has the keyboard from the last press. The page does what Escape does (see [Keyboard entry](keyboard-entry.md), v1.19). |
| `dock://user-css`    | `UserCss`                       | to every dock window: `reload_user_css` read `dock.css` again (see [Themes](themes.md), v1.20) |
| `dock://settings-closing` | `null`                     | to the settings window (v1.25): a close request (×, Alt+F4) hid the window. Rust destroys it 250 ms later unless it is opened again in that time. The page commits the focused field (its blur) and sends any pending edit at once instead of after its debounce. |

Tauri's own `tauri://drag-enter|over|drop|leave` events (files dragged from the shell over a dock
window) are not part of this contract; see [File drags](file-drags.md).

## Startup sequence (UI side)

A page subscribes to the events first, then calls `get_dock_state`, `get_settings`, `get_theme`
and `get_monitors`. A dock page needs the monitors to find the monitor it is on, and the start-up
`dock://monitors` may have been sent before the page subscribed. The page ignores a snapshot
whose revision is lower than the one it already rendered.
