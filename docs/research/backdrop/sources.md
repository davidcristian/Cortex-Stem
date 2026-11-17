# Backdrop research: sources

This page lists the sources of the [backdrop research](index.md).

Microsoft documentation:

- [`DWM_SYSTEMBACKDROP_TYPE`][sbt], [`DWMWINDOWATTRIBUTE`][dwmwa],
  [`DwmSetWindowAttribute`][setattr],
  [`DwmExtendFrameIntoClientArea`][extendframe],
  [`DwmEnableBlurBehindWindow`][blurbehind],
  [`DWM_WINDOW_CORNER_PREFERENCE`][corner]
- [`SetWindowCompositionAttribute`][swca]
- [`SetWindowRgn`][setwindowrgn], [`CreateRoundRectRgn`][roundrect]
- [`SetWindowPos`][setwindowpos], [`DeferWindowPos`][deferwindowpos],
  [`SetLayeredWindowAttributes`][slwa]
- [Window features: layered and owned windows, z-order][window-features],
  [extended window styles][exstyles]
- [`WM_NCACTIVATE`][ncactivate], [`WM_NCCALCSIZE`][nccalcsize],
  [`WM_MOUSEACTIVATE`][mouseactivate]
- [Apply rounded corners in desktop apps][rounded]
- [Acrylic material][acrylic], [Mica material][mica],
  [Use Mica in Win32 apps][mica-win32]
- [`Compositor.CreateHostBackdropBrush`][hostbrush]
- [WebView2 `DefaultBackgroundColor`][webview2-bg]

Code read (this repository and the locked dependencies):

- `crates/stem-platform/src/windows/dock_window.rs` and `dock_window/`
  (styles, corner preference, pass-through toggle, topmost re-assert)
- `src-tauri/src/windows/placement.rs` (the window along its edge:
  `plan_dock_rect`, `edge_rect`), `src-tauri/src/windows/panel.rs`
  (`panel_on_screen`) and `src-tauri/src/commands/dock_window.rs`
  (`set_dock_panel_rect`)
- `ui/src/dock/Dock.svelte`, `ui/src/dock/DockStage.svelte`,
  `ui/src/dock/magnifier.svelte.ts`, `ui/src/dock/measure.svelte.ts`,
  `ui/src/lib/components/DockItem.svelte`, `ui/src/lib/dock/magnify.ts` (panel
  styles, magnification, measurement, slide)
- tao 0.37.2 `src/platform_impl/windows/window.rs` and `event_loop.rs`
  (transparency, `WM_NCCALCSIZE`)
- tauri 2.12.5 `src/vibrancy/windows.rs`; window-vibrancy 0.8.1
  `src/windows.rs` and `README.md`

Reports from other projects (evidence, not documentation):

- [PowerToys pull request 50494][powertoys]: DWM skips the backdrop type on
  frameless `WS_POPUP` windows; `WM_NCACTIVATE(TRUE)` keeps the material
  live; a region plus the corner preference leaked a shadow.
- [noctty pull request 239][noctty]: the material showed on a layered
  `LWA_ALPHA` window.

[sbt]: https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwm_systembackdrop_type
[dwmwa]: https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwmwindowattribute
[setattr]: https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/nf-dwmapi-dwmsetwindowattribute
[extendframe]: https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/nf-dwmapi-dwmextendframeintoclientarea
[blurbehind]: https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/nf-dwmapi-dwmenableblurbehindwindow
[corner]: https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwm_window_corner_preference
[swca]: https://learn.microsoft.com/en-us/windows/win32/dwm/setwindowcompositionattribute
[setwindowrgn]: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowrgn
[roundrect]: https://learn.microsoft.com/en-us/windows/win32/api/wingdi/nf-wingdi-createroundrectrgn
[setwindowpos]: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowpos
[deferwindowpos]: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-deferwindowpos
[slwa]: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setlayeredwindowattributes
[window-features]: https://learn.microsoft.com/en-us/windows/win32/winmsg/window-features
[exstyles]: https://learn.microsoft.com/en-us/windows/win32/winmsg/extended-window-styles
[ncactivate]: https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-ncactivate
[nccalcsize]: https://learn.microsoft.com/en-us/windows/win32/winmsg/wm-nccalcsize
[mouseactivate]: https://learn.microsoft.com/en-us/windows/win32/inputdev/wm-mouseactivate
[rounded]: https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/ui/apply-rounded-corners
[acrylic]: https://learn.microsoft.com/en-us/windows/apps/design/style/acrylic
[mica]: https://learn.microsoft.com/en-us/windows/apps/design/style/mica
[mica-win32]: https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/ui/apply-mica-win32
[hostbrush]: https://learn.microsoft.com/en-us/uwp/api/windows.ui.composition.compositor.createhostbackdropbrush
[webview2-bg]: https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2controller2
[powertoys]: https://github.com/microsoft/PowerToys/pull/50494
[noctty]: https://github.com/amanthanvi/noctty/pull/239
