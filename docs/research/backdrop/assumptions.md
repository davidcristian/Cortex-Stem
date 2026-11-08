# Backdrop research: assumptions

The [backdrop research](index.md) relies on the assumptions below.
Microsoft's documentation confirms none of them. The
[manual test plan](manual-test-plan.md) checks each one.

- **A1.** Assumes a CSS `backdrop-filter` in a transparent WebView2 window
  does not blur other windows or the desktop.
- **A2.** Open: whether the system backdrop needs
  `DwmExtendFrameIntoClientArea` with −1 margins to show through a
  transparent client area.
- **A3.** Assumes DWM skips `DWMWA_SYSTEMBACKDROP_TYPE` on a `WS_POPUP`
  window without `WS_CAPTION` (reported by PowerToys, not documented).
- **A4.** Assumes DWM still draws the material on a layered window: on the
  backdrop window, and on the dock window while its pass-through sets
  `WS_EX_LAYERED`.
- **A5.** Assumes mouse hit-testing skips the part of a window outside its
  window region.
- **A6.** Assumes that handling `WM_NCACTIVATE` as active keeps the
  material live on a window that is never active (reported by PowerToys,
  not documented).
- **A7.** Assumes a window region clips the system backdrop, and DWM draws
  no shadow around a window that has a region.
- **A8.** Assumes the material's edge lags the panel's by one to three
  frames.
- **A9.** Open: the radius of `DWMWCP_ROUND` (commonly given as 8 px at
  100 % scale) and whether it scales with the display.
- **A10.** Assumes a topmost tool window that Stem creates stays on every
  virtual desktop, as the dock window does.
