# Research: auto-hide

This page holds the detail behind [ADR-0008](../adr/0008-auto-hide.md),
which decides how the dock hides and comes back: the alternatives it
rejects for detecting the reveal, and how the slide, the reveal strip and
the fullscreen exception follow the dock's edge.

## Detecting the reveal

These are the alternatives to the reveal strip that
[ADR-0008](../adr/0008-auto-hide.md#decision) rejects. A low-level mouse
hook (`WH_MOUSE_LL`) is rejected: it runs in the input path of every
process, Windows removes it silently when a callback is slow, and it would
duplicate the tracking the hit-test thread does. A second poll on the
events thread is rejected for the same duplication.

## Every edge

`Settings.position` puts the dock on `bottom`, `top`, `left` or `right`
([`docs/ipc/sizing-protocol.md`](../ipc/sizing-protocol.md#layout-on-each-edge-the-page),
"Layout on each edge"). The window is flush with that edge of its
monitor's work area and spans its length; the panel is anchored to the
window's edge side. The auto-hide machine
(`src/lib/dock/autohide.ts`) has phases, holds and facts, and nothing about
edges. Only the rectangles and the motion follow the edge:

- **Slide.** The panel slides out towards its edge, past the window's edge
  side, by its own thickness plus its margin to the edge and its shadow:
  down on `bottom` (`translateY`), up on `top`, left on `left`
  (`translateX`), right on `right`. Reduced motion fades on every edge.
  Failure notices rest beyond the panel's side away from the edge and,
  while the panel is out, just inside the window's edge side, and they move
  back with the panel.
- **Strip.** The reveal strip is the 2 CSS px of the window along its edge
  side, exactly as long as the resting panel and in line with it: the
  bottom 2 px on `bottom`, the top 2 px on `top`, the leftmost 2 px on
  `left`, the rightmost on `right`. While the dock is shown it fills the
  margin between the panel and that side, and the shown envelope grows out
  to that side to include it. A drop released in that band counts as on
  the dock only where the strip is (`onDockAt`).
- **Fullscreen exception.** "The work area stops short of the bounds on the
  dock's edge" is, per edge (where `workAreaReachesEdge` is false):
  `bottom` `workArea.y + workArea.height < bounds.y + bounds.height`; `top`
  `workArea.y > bounds.y`; `left` `workArea.x > bounds.x`; `right`
  `workArea.x + workArea.width < bounds.x + bounds.width`. With the stock
  taskbar at the bottom (the default), a dock on `left`, `right` or `top`
  keeps its strip in fullscreen: its edge is the screen's. With the taskbar
  (or another app bar) on the dock's own edge, fullscreen alone leaves no
  strip there, as on `bottom` (see
  [Fullscreen](../adr/0008-auto-hide.md#fullscreen) in ADR-0008).
- Nothing new crosses IPC: the strip, the envelope and the resting panel go
  out through `set_dock_hit_region` and `set_dock_panel_rect`, in the
  window's viewport, and the app layer converts them for the window where
  it placed it.
