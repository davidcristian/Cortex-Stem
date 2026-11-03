# ADR-0005: A floating, centred dock (not an edge-to-edge bar)

Date: 2026-11-03 · Status: accepted

## Context

The goal is a floating dock: a rounded panel at the bottom
centre, sized to its content, with magnification. An edge-to-edge bar (like
the stock taskbar) is a different product with different layout rules
(AppBar reservation, full-width backgrounds, tray area).

## Decision

- The dock window is sized to its content plus the magnification headroom
  and centred on the bottom edge of its monitor's work area. The UI reports
  that size (`set_dock_content_size`), and Rust places the window.
- No AppBar reservation by default; the dock overlays other windows.
  Reservation (`system.reserveScreenSpace`) and hiding the stock taskbar
  are planned as opt-in settings for M4.
- Visual language: translucent rounded panel, 1 px hairline border, soft
  shadow, icons with a running-indicator dot, separators between groups,
  bounce on launch, a tooltip above the hovered icon.
- Fixed items follow macOS roles: the launcher at the left (Finder's
  spot), the Recycle Bin at the right (Trash's spot).

## Consequences

- The window's transparent regions take clicks until hit-test passthrough
  exists. The headroom is kept minimal.
- Maximised windows sit under the dock unless auto-hide or reservation is
  on.
- Vertical positions (left and right) are a later layout variant.
