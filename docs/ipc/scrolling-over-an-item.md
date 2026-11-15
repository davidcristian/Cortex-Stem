# Scrolling over an item (v1.24)

This file describes what a notch of the mouse wheel over a dock item does.

- `Settings.behavior.scrollAction: ScrollAction` is `none` (the default) or `cycleWindows`.
- The page sends `scroll_item { itemKey, direction }` for each notch over an item.
  `ScrollDirection` is `next` for the wheel turned towards the user (a positive `deltaY`) and
  `previous` for the wheel turned away.
  - The page adds up `deltaY` per item in pixels (`deltaMode` line × 40, page × the viewport's
    height) and sends one command per 100 px, at most ten per event (the rest are dropped). It
    keeps what is left under 100 px for the next event.
  - The count starts again over another item, over no item, and when the pointer leaves the
    panel. An event that goes more sideways than up or down counts for nothing.
  - The page sends nothing while `scrollAction` is `none` or while an item is being dragged, and
    never for a horizontal wheel. It calls `preventDefault` only when it sends, and only on a
    cancelable event.
- Rust, with `cycleWindows`, on an `app` item that has windows in the calling dock's list
  ([Dock windows per monitor](windows.md#dock-windows-per-monitor)): `next` activates the window
  after the item's foreground window in that list, and `previous` the one before it, wrapping
  around at either end.
  - A notch is *admitted* when the 150 ms limit below lets it through. A dock window's next notch
    steps instead from the window that its last admitted notch tried, while that window is still
    in the list, until the foreground changes. So a window that Windows does not bring forward
    is passed over, and a late foreground report does not activate the same window twice.
  - When neither window is in the list, `next` activates the first window and `previous` the
    last. The activation is `WindowAction::Activate`, which restores a minimised window.
  - Anything else (no windows, a fixed item, a separator, `none`) does nothing and resolves. An
    unknown key (one the snapshot has no item for, including a hidden app's) rejects as
    `primary_click` does, whatever `scrollAction` is.
  - Rust lets at most one activation per dock window through every 150 ms
    (`stem_service::SCROLL_INTERVAL`). A scroll that comes sooner is dropped, so a fast spin of
    the wheel moves one window, not ten.
- `MockBackend` records `scrollItem` and, with `cycleWindows`, makes the chosen demo window the
  foreground window. It does not mirror the 150 ms limit or the memory of the tried window,
  because its activations always succeed at once.
