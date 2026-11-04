# ADR-0008: Auto-hide reveals through the hit region, not a new hook

Date: 2026-11-04 · Status: accepted

## Context

With `AutoHideMode::Always` the dock slides away `autoHide.delayMs` after
the pointer has left it, and comes back when the pointer touches the
bottom edge. The question is how the reveal is detected: a `GetCursorPos`
poll on the events thread while the dock is hidden, or a low-level mouse
hook.

The platform already tracks the pointer for every dock window on the
hit-test thread (raw input with `RIDEV_INPUTSINK`, plus a 4 Hz safety
poll). It switches the window between click-through and hit-testable from
two rectangles the UI reports: the `panel` (entering it makes the window
take clicks) and the `envelope` (leaving it makes the window click-through
again). The dock window's bottom edge is the bottom edge of its monitor's
work area.

## Decision

- **No new platform mechanism.** While hidden, the UI reports a thin reveal
  strip as both `panel` and `envelope`: the bottom 2 CSS px of the dock
  window, as wide as the resting panel. The hit-test thread makes the
  window hit-testable when the pointer enters the strip, as it does for the
  panel, and the webview's own `pointerenter` on the strip element starts
  the reveal. A low-level mouse hook and a second poll on the events thread
  are rejected
  ([Detecting the reveal](../research/auto-hide.md#detecting-the-reveal)).
- **The state machine is presentation and lives in the UI**, as a pure,
  unit-tested module beside the magnification maths
  (`src/lib/dock/autohide.ts`), like magnification itself: `shown` → (the
  pointer left the panel) → `pending` → (`delayMs` passed) → `hidden` →
  (the pointer in the strip) → `shown`. It never hides while a hold is on:
  an icon lifted for a drag, a native context menu from the dock open, or
  an item bouncing for a launch the dock started. Whether a hidden dock
  comes back when one of its windows needs attention is left open.
- **Motion:** the panel slides out by its own height plus its margin to
  the edge and its shadow, with `transform: translateY` over 200 ms
  (`ease-in` out, `ease-out` back in). With `prefers-reduced-motion` it
  fades instead. The dock window keeps its size and place, so no
  `SetWindowPos` happens on hide or reveal.
- **Settings:** `autoHide.mode` is `off` (the default), `always` or
  `whenOverlapped`. `autoHide.delayMs` is 250 ms by default and at most
  5000 ms. `whenOverlapped` (hide only while a window overlaps the dock's
  rectangle) needs window geometry from the platform and comes later; until
  then it behaves like `off`, and the settings page says so.
- With click-through off, the whole dock window takes clicks even while
  the panel is hidden; the settings page says that auto-hide works best
  with click-through on. Neither setting changes the other.

## Consequences

- The reveal zone is the work area's bottom edge. That is the screen's
  edge only when the stock taskbar is hidden or auto-hidden; with a visible
  stock taskbar, the strip sits just above it.
- While the dock is hidden, a click within 2 px of the work area's bottom
  edge goes to the dock (the strip is hit-testable while the pointer is
  inside it). macOS has the same reveal zone.
- Touch and pen produce no hover, so they cannot reveal a hidden dock; the
  global shortcut and the tray toggle still show it.
- The reveal itself needs no IPC beyond the hit region and the content size
  the UI reports.
