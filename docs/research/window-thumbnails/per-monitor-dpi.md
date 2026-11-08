# Research: per-monitor DPI of window previews

This page describes how the window previews of
[ADR-0009](../../adr/0009-window-thumbnails.md) place and scale each
thumbnail on monitors with different scale factors. A1 to A11 name the
assumptions listed in the ADR's
[Assumptions](../../adr/0009-window-thumbnails.md#assumptions).

- The UI reports slots in CSS px of its own viewport. The app layer converts
  them once, with that window's scale factor (`scale_rect`), as it does the
  hit region. Everything below is physical pixels.
- The platform fits the source into the slot. It keeps the aspect ratio,
  centres the image and never scales it above 1:1 (pure
  `rules::thumbnail::fit`).
- It crops the invisible resize borders of Windows 11 frames through
  `rcSource`, using the extended frame bounds against the window rectangle
  ([`DwmGetWindowAttribute`][dga] with `DWMWA_EXTENDED_FRAME_BOUNDS`,
  [`GetWindowRect`][gwr]). Both are already read by `windows/enumerate.rs`
  (A7).
- `rcDestination` is "the area in the destination window" ([props]). For
  the dock window, tao removes the non-client frame, so client and window
  coordinates are the same (A2).
- A source on a monitor with another scale needs nothing: DWM scales it.
- A DPI change of the dock window while a strip is open: the page
  closes the strip. The next hover lays it out at the new scale.
- With one dock per monitor, each dock window has its own strip and
  registrations. Only one strip is open at a time.

[props]: https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ns-dwmapi-dwm_thumbnail_properties
[dga]: https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/nf-dwmapi-dwmgetwindowattribute
[gwr]: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getwindowrect
