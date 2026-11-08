# Backdrop research: options

This page compares the ways to show Acrylic or Mica behind the dock's panel
([backdrop research](index.md)). Option 6, the recommended one, has
[its own page](backdrop-window.md). [Context](context.md) describes the
dock window that every option must work with, and
[assumptions.md](assumptions.md) lists the assumptions A1 to A10.

## 1. `DWMWA_SYSTEMBACKDROP_TYPE` on the dock window

[`DwmSetWindowAttribute`][setattr] with
[`DWMWA_SYSTEMBACKDROP_TYPE`][dwmwa] set to `DWMSBT_TRANSIENTWINDOW`
(Desktop Acrylic), `DWMSBT_MAINWINDOW` (Mica) or `DWMSBT_TABBEDWINDOW` (Mica
Alt). The documentation says each is drawn "behind the entire window
bounds". The client area must let it through. The page is already
transparent, and tao's blur-behind call makes its alpha count. Some apps
also call [`DwmExtendFrameIntoClientArea`][extendframe] with margins of −1,
which the documentation calls the "sheet of glass" effect. Whether the
dock needs that call is A2. The window needs a frame style (see
[option 6](backdrop-window.md)); tao's `WS_CAPTION` provides one.

- Limited to the panel: no. A band as wide as the work area and as tall as
  the window.
- Size and magnification: nothing to follow; the band is always there.
- Slide: the band stays while the panel slides out.
- Cost: a few lines. Risk: low for the API, but the result is unusable. The
  dock window is almost never active, so it would also show the flat
  fallback, and the effect of the layered toggle on the material is unknown
  (A4).

Rejected.

## 2. `SetWindowCompositionAttribute` with `ACCENT_ENABLE_ACRYLICBLURBEHIND`

user32 exports the function, and a caller finds it with `GetProcAddress`.
Its page says "The use of this API is not recommended. Use
DwmSetWindowAttribute instead", and lists Windows 7 as the end of client
support ([`SetWindowCompositionAttribute`][swca]). The `ACCENT_POLICY`
structure and the accent states are not documented anywhere. The
window-vibrancy README reports bad performance while a window that uses it
is resized or dragged, on Windows 10 1903 and later and on Windows 11 build
22000. PowerToys replaced this path with `DWMWA_SYSTEMBACKDROP_TYPE`
([pull request 50494][powertoys], merged 2026-09-09).

- Limited to the panel: no; the same band as option 1. Whether a window
  region clips it is unknown.
- Size and magnification, slide: as option 1.
- Cost: small. Risk: high. There is no contract, and any Windows update can
  change the function.

Rejected.

## 3. `DwmEnableBlurBehindWindow` with a region

The page says: "Beginning with Windows 8, calling this function doesn't
result in the blur effect" ([`DwmEnableBlurBehindWindow`][blurbehind]). On
Windows 11 it only makes the window's alpha values count, which is why tao
calls it. A region (`hRgnBlur`) selects where the alpha counts, not where a
blur appears.

- Limited to the panel: there is no effect to limit.
- Risk: replacing tao's call could break the window's transparency.

Rejected.

## 4. The `window-vibrancy` crate

`window-vibrancy = "0.8"` is in `[workspace.dependencies]`, but no
workspace crate uses it. Tauri 2.12 uses it for `WebviewWindowBuilder::effects` and
`set_effects` (`tauri/src/vibrancy/windows.rs`). Version 0.8.1
(`src/windows.rs`) does this:

- `apply_acrylic`: `DWMWA_SYSTEMBACKDROP_TYPE = DWMSBT_TRANSIENTWINDOW` on
  build 22523 or later, else `SetWindowCompositionAttribute` with
  `ACCENT_ENABLE_ACRYLICBLURBEHIND` on build 17763 or later.
- `apply_mica`: optionally `DWMWA_USE_IMMERSIVE_DARK_MODE`, then
  `DWMSBT_MAINWINDOW` on build 22523 or later, else the undocumented
  attribute 1029 on build 22000 or later.
- `apply_tabbed`: `DWMSBT_TABBEDWINDOW`. `apply_blur`: the accent
  `ACCENT_ENABLE_BLURBEHIND`.
- The `clear_*` functions set `DWMSBT_NONE` or `ACCENT_DISABLED`.

It never calls `DwmExtendFrameIntoClientArea`, `SetWindowRgn` or the corner
preference, and it always acts on a whole window. Its threshold, 22523, is
an Insider build below the documented 22621. Below it the crate uses option
2 or an undocumented attribute.

- Limited to the panel: no; it is options 1 and 2 behind one call.
- Cost: the smallest. Risk: that of options 1 and 2.

Rejected as the mechanism. [Option 6](backdrop-window.md) needs only its
one documented call, which `stem-platform` can make directly: it already
enables the `Win32_Graphics_Dwm` feature.

## 5. A window region on the dock window (`SetWindowRgn`)

[`SetWindowRgn`][setwindowrgn]: "The system does not display any portion of
a window that lies outside of the window region". A region would cut a
whole-window backdrop down to the panel. It would also cut everything else
the page draws outside the panel: magnified icons, which rise above it;
tooltips; the bounce; toasts; the icon lifted for a drag. Mouse
hit-testing also stops at the region (A5: widely reported, but the
`SetWindowRgn` page does not say it), so the window region would also limit
where the dock takes clicks, which is the job of the hit region.

- Limited to the panel: yes, but only by also clipping what must overflow
  the panel. A region the size of the envelope keeps the content and brings
  the band back over the envelope.
- Size and magnification: a new region per frame. Each call sends
  `WM_WINDOWPOSCHANGING` and `WM_WINDOWPOSCHANGED` and redraws. The region
  comes through IPC, so it lags the page by frames.
- Slide: the region would have to follow the slide, per frame.
- Corners: a region is a set of whole pixels
  ([`CreateRoundRectRgn`][roundrect]), so its curve has pixel steps, and a
  window with a region "cannot ever be rounded" by DWM
  ([rounded corners][rounded]).
- Cost: medium. Risk: high. It changes the dock window, which is stable,
  and how a region interacts with the layered toggle is unknown.

Rejected.

## Not taken further

- The Windows App SDK's `MicaController` and `DesktopAcrylicController`
  need the Windows App SDK runtime and a composition target
  ([Mica in Win32 apps][mica-win32]): a new runtime dependency.
- A Windows.UI.Composition visual in the backdrop window, filled with a host
  backdrop brush (`DWMWA_USE_HOSTBACKDROPBRUSH`,
  [`Compositor.CreateHostBackdropBrush`][hostbrush]) under a rounded clip,
  would give an anti-aliased corner at any radius. It is a variant of
  [option 6](backdrop-window.md) for the case where the region's corners
  look bad. It needs more code (WinRT composition) and has the same lag.
  Not evaluated.
- Mica Alt (`DWMSBT_TABBEDWINDOW`) is a stronger wallpaper tint for tabbed
  title bars. A dock has no use for it, and `Backdrop` has no value for it.

## Summary

| Option | Panel only | Size, magnification | Auto-hide slide | Cost | Risk |
|---|---|---|---|---|---|
| 1 backdrop type on the dock | no, a band | n/a | the band stays | tiny | low API, unusable result |
| 2 accent policy | no, a band | n/a | the band stays | small | high, undocumented |
| 3 blur-behind region | no effect | n/a | n/a | none | breaks transparency |
| 4 window-vibrancy | no, it is 1 and 2 | n/a | the band stays | tiny | as 1 and 2 |
| 5 region on the dock | only by clipping icons | per-frame region, lags | per-frame region | medium | high |
| 6 backdrop window | yes | yes, 1 to 3 frames late (A8) | hidden during the slide | large | medium, 4 undocumented facts |
| CSS, today | yes | exact | exact | none | none; no desktop blur (A1) |

[dwmwa]: https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwmwindowattribute
[setattr]: https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/nf-dwmapi-dwmsetwindowattribute
[extendframe]: https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/nf-dwmapi-dwmextendframeintoclientarea
[blurbehind]: https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/nf-dwmapi-dwmenableblurbehindwindow
[swca]: https://learn.microsoft.com/en-us/windows/win32/dwm/setwindowcompositionattribute
[setwindowrgn]: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowrgn
[roundrect]: https://learn.microsoft.com/en-us/windows/win32/api/wingdi/nf-wingdi-createroundrectrgn
[rounded]: https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/ui/apply-rounded-corners
[mica-win32]: https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/ui/apply-mica-win32
[hostbrush]: https://learn.microsoft.com/en-us/uwp/api/windows.ui.composition.compositor.createhostbackdropbrush
[powertoys]: https://github.com/microsoft/PowerToys/pull/50494
