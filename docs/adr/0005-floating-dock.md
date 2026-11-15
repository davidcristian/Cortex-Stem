# ADR-0005: A floating, centred dock (not an edge-to-edge bar)

Date: 2026-11-03 · Status: accepted

## Context

The goal is a floating dock: a rounded panel at the bottom
centre, sized to its content, with magnification. An edge-to-edge bar (like
the stock taskbar) is a different product with different layout rules
(AppBar reservation, full-width backgrounds, tray area).

## Decision

- The dock window spans the full length of one edge of its monitor's work
  area (`settings.position`, `bottom` by default) and is as thick as the
  content plus the magnification headroom. The UI reports that size
  (`set_dock_content_size`), Rust places the window, and the UI centres the
  panel in it, so items animating in and out never resize the window.
- No AppBar reservation by default; the dock overlays other windows.
  Reservation (`system.reserveScreenSpace`) is planned as an opt-in setting
  for M4. Hiding the stock taskbar (`system.hideWindowsTaskbar`) is opt-in.
- Visual language: translucent rounded panel, 1 px hairline border, soft
  shadow, icons with a running-indicator dot, separators between groups,
  bounce on launch, a tooltip above the hovered icon.
- Fixed items follow macOS roles: the launcher at the left (Finder's
  spot), the Recycle Bin at the right (Trash's spot).

## Consequences

- The window's transparent regions would take clicks. With
  `behavior.clickThrough` on (the default), the platform makes the window
  click-through outside the panel (the hit region,
  `docs/ipc/hit-region.md`). The headroom is kept minimal.
- Maximised windows sit under the dock unless auto-hide or reservation is
  on.
- Vertical positions (left and right) are a later layout variant.
