# Scrolling over an item (v1.24)

This file describes what a notch of the mouse wheel over a dock item does.

- `Settings.behavior.scrollAction: ScrollAction` is `none` (the default) or `cycleWindows`.
- The page sends `scroll_item { itemKey, direction }` for each notch over an item.
  `ScrollDirection` is `next` for the wheel turned towards the user and
  `previous` for the wheel turned away.
  - The page adds up `deltaY` in pixels (`deltaMode` line × 40, page × the viewport's height)
    and sends one command per 100 px. It keeps what is left under 100 px for the next event.
  - The page sends nothing while `scrollAction` is `none`, and never for a horizontal wheel. It
    calls `preventDefault` only when it sends.
- Rust, with `cycleWindows`, on an `app` item that has windows in the calling dock's list
  ([Dock windows per monitor](windows.md#dock-windows-per-monitor)): `next` activates the window
  after the item's foreground window in that list, and `previous` the one before it, wrapping
  around at either end.
  - When none of them is in front, `next` activates the first window and `previous` the
    last. The activation is `WindowAction::Activate`, which restores a minimised window.
  - Anything else (no windows, a fixed item, a separator, `none`) does nothing and resolves. An
    unknown key rejects as `primary_click` does.
  - Rust lets at most one activation per dock window through every 150 ms. A scroll that comes
    sooner is dropped, so a fast spin of the wheel moves one window, not ten.
- `MockBackend` records `scrollItem` and, with `cycleWindows`, makes the chosen demo window the
  foreground window.
