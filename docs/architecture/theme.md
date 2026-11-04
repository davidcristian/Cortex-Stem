# Theme

This file describes how the dock and the settings window follow the theme.

Theme is CSS, driven by `dock://theme` (`ThemeInfo`) and the `theme`
setting, the same way in both windows:

- `resolveTheme` sets `data-theme` (`light` or `dark`) on `<html>`. The
  setting takes precedence; `system` follows Windows, and
  `prefers-color-scheme` until the first `ThemeInfo` arrives.
- `accentColor` gives `--accent` (dots, the Start tile, switches and
  sliders).
- `effectiveOpacity` gives the panel's `--panel-opacity`: the user's value,
  at least 0.95 with Windows' transparency effects off, and 1 in high
  contrast.

`ThemeInfo.highContrast` also sets `data-high-contrast` on `<html>`. The
dock then drops its blur and shadow and draws a 2 px border. The colour
tokens in `src/app.css` (and the settings page's own) make tooltips,
toasts, cards and controls solid, with lines in the full text colour.

Under a Windows contrast theme WebView2 normally also renders in
forced-colours mode, which repaints backgrounds in the theme's Canvas
colour. The parts drawn as backgrounds that would disappear (running dots,
separators, the tooltip's arrow, the settings switches and sliders) opt out
with `forced-color-adjust: none` inside `@media (forced-colors: active)`,
and use the theme's system colours (`CanvasText`, `Highlight`,
`HighlightText`, `GrayText`).

The settings window's native title bar follows Windows, not the `theme`
setting.
