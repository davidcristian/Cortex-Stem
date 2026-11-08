# Events (Rust → UI)

These are the events that the Rust app sends to the pages. [Principles](index.md#principles) says
how a page listens.

| Event                | Payload                         | When                                                         |
|----------------------|---------------------------------|--------------------------------------------------------------|
| `dock://state`       | `DockState`                     | the items, the foreground, `fullscreen` or `overlapped` changed, an identity was resolved, or settings were applied. [The state snapshot](state-snapshot.md) describes `fullscreen` and `overlapped`. |
| `dock://settings`    | `Settings`                      | settings were validated and persisted, whatever their source  |
| `dock://theme`       | `ThemeInfo`                     | at start, and when the OS theme or accent colour changes      |
| `dock://monitors`    | `MonitorInfo[]`                 | at start, and when the display topology changes               |
| `dock://item-bounce` | `{ itemKey: string }`           | an app was launched from the dock; the page plays the bounce  |
| `dock://show`        | `null`                          | to one dock window: a tray click or the toggle shortcut requested the dock while auto-hide had slid its panel out. The page brings the panel back (see [Toggle while hidden](toggle-while-hidden.md)). |

Tauri's own `tauri://drag-enter|over|drop|leave` events (files dragged from the shell over a dock
window) are not part of this contract; see [File drags](file-drags.md).

## Startup sequence (UI side)

A page subscribes to the events first, then calls `get_dock_state`, `get_settings` and
`get_theme`. The page ignores a snapshot whose revision is lower than the one it already rendered.
