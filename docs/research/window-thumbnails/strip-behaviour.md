# Research: how the preview strip behaves

This page describes the preview strip of
[ADR-0009](../../adr/0009-window-thumbnails.md#recommendation) in detail:
when it opens and closes, the hit region it reports, the cards it shows
for several, minimised and cloaked windows, and how it works with
auto-hide. A1 to A11 name the assumptions listed in the ADR's
[Assumptions](../../adr/0009-window-thumbnails.md#assumptions).

## Hover timing

The timing is presentation, so it lives in the UI, like the auto-hide
machine (ADR-0008).

- A strip opens when the pointer rests for 400 ms on an app item that has at
  least one window. 400 ms is the system's default hover time
  ([`TRACKMOUSEEVENT`][tme] remarks). Pinned-only apps, fixed items and
  separators never open one.
- Once a strip is open, moving onto another app item with windows switches
  the strip at once, as the tooltip does.
- Leaving both the item and the strip starts a 250 ms grace period. Coming
  back cancels it. So the pointer can cross the gap on a diagonal.
- The strip closes at once on a click on the item or on a card, a
  right-click, a drag lift, a file drag, a native menu, a device pixel
  ratio change, the page being hidden, or the item losing its last window.
- The frame fades in over 120 ms (no fade under reduced motion). The page
  reports the slots when the fade ends, so the image never appears before
  its card. On close, it reports the empty list first.
- The machine is a pure module, `src/lib/dock/preview.ts`, unit-tested like
  `autohide.ts`.

## Hit region

- While a strip is open, the UI reports an envelope that also covers the
  strip, 8 CSS px around it, and the corridor between the strip and the
  panel. The panel stays as it is. The pointer can go from the icon to a
  card and the window stays clickable. Leaving that envelope makes it
  click-through, as with the normal envelope.
- The 8 px margin lets the page see the strip's `pointerleave` before the
  window turns click-through (see the
  [Context](../../adr/0009-window-thumbnails.md#context) of ADR-0009).
- When the strip closes, the UI reports the normal envelope again.

## Several windows per app

- One card per window, in the item's `windows` order, which is the engine's
  order. A card is a title row (the window's icon, its title on one line, a
  close button) above an image area. The foreground window's card has the
  accent outline (`WindowInfo.isActive`).
- Nothing is drawn over the image area, because DWM may draw above the web
  content (A1).
- A click activates that window (`window_action Activate`, which also
  restores a minimised window). The close button and a middle-click close it
  (`window_action Close`: a posted `WM_CLOSE`; the app may ask to save).
  Both commands exist.
- With many windows, the cards shrink to fit the window's width, down to an
  image width of 120 CSS px. Past that, the strip shows the first cards that
  fit and a last "+N" tile. The tile opens the item's native menu
  (`show_item_menu`), which lists every window by title.

## Minimised windows

- DWM gives no live image of a minimised window. Whether a third party gets
  its last frame is not documented. The attribute list mentions a "snapshot
  representation" ([`DWMWINDOWATTRIBUTE`][attrs]), and one third-party note
  says DWM keeps the last frame (A3).
- Unless the probe shows otherwise, the card shows the app's icon (the bitmap
  the item already has), centred in the image area, and nothing is
  registered. A click restores and activates the window.
- If the probe finds a usable last frame, minimised windows get a
  registration at about 60 % opacity (`DWM_TNP_OPACITY`). They then read as
  minimised without anything drawn over the image.
- Capturing a frame before a window is minimised is
  [option 4](options.md#4-captured-bitmaps-rejected-without-a-prototype), rejected.

## Cloaked windows and other desktops

- With `showOnlyCurrentDesktop` on (the default), windows on other desktops
  are not in the item at all.
- With it off, a window on another desktop gets the icon card, like a
  minimised one. A click activates it, and Windows switches to its desktop.
- A suspended Store app is cloaked on the current desktop. It gets the same
  icon card.
- The documentation says a cloaked window "is still composed by DWM"
  ([`DWMWA_CLOAK`][attrs]), so a live image may work (A4). The probe
  checks. The `Elsewhere` rule (in
  [Who decides what](../../adr/0009-window-thumbnails.md#who-decides-what-adr-0006-adr-0007) in
  ADR-0009) is one line to change if it does.

## Auto-hide

- An open strip is a hold (`preview`), like an open menu. The dock does not
  hide while it is open, whatever the mode and whatever the `fullscreen`
  and `overlapped` facts are. It is not a revealing hold.
- No strip opens while the panel is hidden or sliding.
- When the strip closes with the pointer away, the normal delay starts.

[tme]: https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-trackmouseevent
[attrs]: https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwmwindowattribute
