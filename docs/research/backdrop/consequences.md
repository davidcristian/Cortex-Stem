# Backdrop research: consequences

If option 6 of the [backdrop research](index.md), a
[backdrop window](backdrop-window.md) under each dock window, ships:

- There is one more top-level window per dock window. It is visible only
  with a native backdrop and a shown panel. It is hidden while the panel is
  slid out, while the dock window is hidden (tray, toggle shortcut), under
  a fullscreen app, and in a contrast theme.
- The material's edge lags the panel's by a few frames when the panel
  changes size. The CSS look never lags, and stays available.
- The material appears at once at the end of a reveal instead of sliding
  in.
- IPC gets two additions: the live panel box while it changes (`null`
  during the slide), and the effective backdrop for the UI. Over a native
  material the UI drops its `backdrop-filter` and lightens its tint.
- The z-order has two windows to keep in step; the topmost re-assert
  restacks the backdrop too.
- Nothing changes on builds before 22621. With *Transparency effects* off
  or Battery Saver on, Windows draws the solid fallback
  ([Acrylic][acrylic]).
- The Playwright assertions that the two choices are disabled
  (`e2e/screenshots.spec.ts`) change when the choices are offered.

[acrylic]: https://learn.microsoft.com/en-us/windows/apps/design/style/acrylic
