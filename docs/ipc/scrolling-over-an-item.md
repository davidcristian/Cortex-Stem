# Scrolling over an item (v1.24)

This file describes what a notch of the mouse wheel over a dock item does.

- `Settings.behavior.scrollAction: ScrollAction` is `none` (the default) or `cycleWindows`.
- The page sends `scroll_item { itemKey, direction }` for each notch over an item.
  `ScrollDirection` is `next` for the wheel turned towards the user (a positive `deltaY`) and
  `previous` for the wheel turned away.
  - The page adds up `deltaY` per item in pixels (`deltaMode` line × 40, page × the viewport's
    height) and sends one command per 100 px, at most ten per event (the rest are dropped). It
    keeps what is left under 100 px for the next event. Travel within 0.5 px of a whole notch
    counts as that notch.
  - An event of at least 30 px on its own is one notch even when it is shorter than 100 px, and
    leaves nothing over. The reason: Chromium on Windows makes a wheel notch Windows' *lines to
    scroll at a time* × 100/3 px (33⅓ px at one line; taken from Chromium's source), so each
    notch is one command whatever that setting is (v1.26).
  - The count starts again over another item, over no item, when the pointer leaves the panel,
    and for an event in the other direction. An event that goes more sideways than up or down
    counts for nothing. So does an event with `ctrlKey` set (v1.29): Chromium reports a
    touchpad's pinch as wheel events with `ctrlKey` set, and Ctrl with the wheel is a zoom. Both
    leave the count as it was.
  - The page sends nothing while `scrollAction` is `none` or while an item is being dragged, and
    never for a horizontal wheel, a pinch or Ctrl with the wheel. It calls `preventDefault` only
    when it sends, and only on a cancelable event, so an event that sends nothing keeps its
    default handling. Over the volume item it sends whatever `scrollAction` is (v1.30, see
    [The volume item](widget-items.md#the-volume-item-v130)).
- Rust, with `cycleWindows`, on an `app` item that has windows in the calling dock's list
  ([Dock windows per monitor](windows.md#dock-windows-per-monitor)): `next` activates the window
  after the item's foreground window in that list, and `previous` the one before it, wrapping
  around at either end.
  - A notch is *admitted* when the 150 ms limit below lets it through. Until the foreground
    changes, and for at most 2 s after its last admitted notch
    (`stem_core::engine::SCROLL_MEMORY`, v1.31), a dock window's next notch steps instead from the
    window that this notch tried, while that window is still in the list. So a window that
    Windows does not bring forward is passed over, and a late foreground report does not activate
    the same window twice. A notch after a pause steps from the foreground again, and tries such a
    window once more.
  - When neither window is in the list, `next` activates the first window and `previous` the
    last. The activation is `WindowAction::Activate`, which restores a minimised window.
  - The volume item steps the volume whatever the setting, without the limit below (see
    [The volume item](widget-items.md#the-volume-item-v130), v1.30). Anything else (no windows,
    another fixed item, a separator, `none`) does nothing and resolves. An unknown key (one the
    snapshot has no item for, including a hidden app's) rejects as `primary_click` does, whatever
    `scrollAction` is.
  - Rust lets at most one activation per dock window through every 150 ms
    (`stem_service::SCROLL_INTERVAL`). A scroll that comes sooner is dropped, so a fast spin of
    the wheel moves one window, not ten.
- `MockBackend` records `scrollItem` and, with `cycleWindows`, makes the chosen demo window the
  foreground window. It does not mirror the 150 ms limit or the memory of the tried window,
  because its activations always succeed at once.
