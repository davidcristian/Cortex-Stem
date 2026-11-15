# Keyboard entry (v1.19)

The dock window never activates on a click (`WS_EX_NOACTIVATE`), so Tab cannot move the focus
into it. A global shortcut gives it the keyboard focus, and Escape gives the focus back, like
Win+T for the stock taskbar:

- `Settings.behavior.focusShortcut` (default `"Ctrl+Alt+Shift+D"`; `""` means none) has the
  format and rules of `toggleShortcut`
  ([Settings the app shell applies](app-shell-settings.md)). A value that names the same chord
  as `toggleShortcut` (ignoring case and modifier order) registers nothing and logs a warning. A
  change takes effect without a restart.
- On a press, Rust runs these steps:
  1. It stores the window that is in front at that moment: the engine's active window.
  2. It picks the dock window on that window's monitor with `monitors = all`. Otherwise, or when
     that monitor has no dock, it picks `dock`.
  3. It shows that dock window if the tray or the toggle shortcut hid it, or sends it
     `dock://show` if auto-hide slid its panel out.
  4. It activates the dock window (`Platform::focus_dock_window`, the foreground activation
     path; the hotkey press gives Stem the right to take the foreground).
  5. It emits `dock://focus` to the dock window.

  A press while a dock window already has the focus works like Escape.
- The page focuses its roving tab stop (the one toolbar item with `tabindex="0"`): the active
  app's item when there is one, else the first item. The focus ring and the title pill show as
  they do for Tab, and the `focus` hold of the auto-hide state machine keeps the panel up.
- Escape calls `release_dock_focus`. Rust activates the stored window again if it still exists
  (through the activation path), else it leaves the foreground alone. Enter and Space click the
  item, as they do without the shortcut.
- A click that activates or launches something supersedes the stored window: Rust clears the
  window.
- When the dock window loses the activation any other way (a click elsewhere, Alt+Tab), Rust
  clears the stored window too.
- `MockBackend.requestFocus()` emits `dock://focus` as a press would. `release_dock_focus` is
  recorded (`calls` entry `releaseDockFocus`).
