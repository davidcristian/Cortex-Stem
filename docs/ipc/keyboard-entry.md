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
  1. It stores the window that is in front at that moment: the engine's active window, or the
     settings window when that is the foreground window. (The engine never takes Stem's own
     windows for the foreground, so its active window is then the app behind the settings
     window.)
  2. It picks the dock window on that window's monitor with `monitors = all`. Otherwise, or when
     that monitor has no dock, it picks `dock`.
  3. It shows that dock window if the tray or the toggle shortcut hid it, or sends it
     `dock://show` if auto-hide slid its panel out.
  4. It activates the dock window (`Platform::focus_dock_window`, the foreground activation
     path; the hotkey press gives Stem the right to take the foreground).
  5. It emits `dock://focus` to the dock window.

  A press while a dock window still has the keyboard from an earlier press (it is the foreground
  window, and the stored press was its own) sends it `dock://release-focus` instead, and the page
  does what Escape does. A press while a dock window is in front without that condition (after an
  Escape that had no window to go back to) runs steps 1 to 5 again. When the activation fails,
  Rust logs a warning, clears the stored press and sends nothing. Only the picked window is
  shown: a press never shows the other windows of a set that the toggle hid.
- The page focuses its roving tab stop (the one toolbar item with `tabindex="0"`): the active
  app's item when there is one, else the first item. It calls `focus({ focusVisible: true })`,
  because Chromium chooses whether a script's focus matches `:focus-visible` from the page's last
  input, and the shortcut's keys never reach the page. The page focuses after the DOM update.
  When its panel is still slid out (after a press during the slide), the page brings it back
  itself. The focus ring and the title pill show as they do for Tab, and the `focus` hold of the
  auto-hide state machine keeps the panel up.
- Every Escape that does not cancel a drag, and every `dock://release-focus`, blurs the focused
  item and calls `release_dock_focus`. Rust does nothing unless the stored press was that dock's.
  A call from another dock window keeps the stored press while the press's dock is still the
  foreground window, and clears it when that dock is no longer in front. Rust activates the
  stored window again if it still exists (an app's window through the activation path, the
  settings window with Tauri's `set_focus`), else it leaves the foreground alone. Enter and Space
  click the item, as they do without the shortcut.
- A click supersedes the stored window: Rust clears the window and keeps the press. A click here
  is `primary_click`, `launch_app`, `window_action`, or an item menu entry other than *Pin to
  Stem*, *Unpin from Stem*, *Hide from Stem*, *Stem settings…* and *Quit Stem*. The entries left
  out only change the settings or act on Stem itself (v1.26). The dock keeps the keyboard, so a
  press while it is still in front releases the focus.
- When the dock window loses the activation any other way (a click elsewhere, Alt+Tab: Tauri's
  `Focused(false)` while another window is the foreground window), Rust clears the whole stored
  press.
- `MockBackend.requestFocus()` emits what a press would. While the page's dock has the keyboard
  from an earlier press (until `release_dock_focus`, `pushForeground` or an activation), it emits
  `dock://release-focus`. Otherwise it emits `dock://show` first while the page last reported
  its panel slid out (`set_dock_auto_hidden(true)`), then `dock://focus`. `release_dock_focus`
  is recorded (`calls` entry `releaseDockFocus`).
