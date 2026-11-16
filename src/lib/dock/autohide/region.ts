/** The reveal strip and the hit region of a dock that auto-hides (`../autohide.ts`). */

import type { MonitorInfo } from '$lib/ipc/generated/MonitorInfo';
import type { Rect } from '$lib/ipc/generated/Rect';

import type { DockEdge } from '../edge';
import type { HitRegion, Viewport } from '../geometry';

import {
  autoHideActive,
  isAutoHidden,
  onlyFullscreenHides,
  type AutoHidePhase,
  type AutoHideState,
} from './state';

/** Thickness of the reveal strip along the dock window's edge side, CSS px (ADR-0008). */
export const REVEAL_STRIP_PX = 2;

/**
 * The dock window's edge side is the screen's: the window
 * sits on `edge` of `monitor`'s work area (ADR-0008), which reaches the
 * monitor's own edge there unless a visible stock taskbar (or another app
 * bar) holds the space between (docs/ipc/hit-region.md: on `bottom`
 * `workArea.y + workArea.height >= bounds.y + bounds.height`, on `top`
 * `workArea.y <= bounds.y`, on `left` `workArea.x <= bounds.x`, on `right`
 * `workArea.x + workArea.width >= bounds.x + bounds.width`). True with the
 * taskbar auto-hidden or on another edge, and for a monitor not known yet
 * (the strip as it always was).
 */
export function workAreaReachesEdge(
  monitor: MonitorInfo | null | undefined,
  edge: DockEdge,
): boolean {
  if (monitor === null || monitor === undefined) {
    return true;
  }
  const { bounds, workArea } = monitor;
  switch (edge) {
    case 'bottom':
      return workArea.y + workArea.height >= bounds.y + bounds.height;
    case 'top':
      return workArea.y <= bounds.y;
    case 'left':
      return workArea.x <= bounds.x;
    case 'right':
      return workArea.x + workArea.width >= bounds.x + bounds.width;
  }
}

/**
 * The reveal strip is in place in `state`: auto-hide applies, and either
 * the strip is at the screen's edge (`atScreenEdge`, `workAreaReachesEdge`)
 * or something other than fullscreen hides the dock (the mode `always`, or
 * an overlap with `whenOverlapped`), whose strip sits on the work area's
 * edge as it always has (ADR-0008 "Consequences").
 */
export function revealStripOn(state: AutoHideState, atScreenEdge: boolean): boolean {
  return autoHideActive(state) && (atScreenEdge || !onlyFullscreenHides(state));
}

/**
 * (`x`, `y`) is on the dock where no pointer event says so (a drag or a
 * file drag released): over `panel` (the panel as drawn, CSS px of
 * the viewport), or, with the reveal strip in the DOM (`stripOn`,
 * `revealStripOn`), in the band between it and the window's edge side, as
 * long as the panel (below it on `bottom`, left of it on `left`, and so
 * on), out to that side of `viewport`: the strip fills that band while the
 * dock is on screen (`revealStripThickness`), so its `pointerleave` reports
 * the pointer going. Without a strip (fullscreen above a visible taskbar,
 * or auto-hide off) nothing there would, and a pointer counted inside would
 * stay so until it had crossed the panel.
 */
export function onDockAt(
  panel: Rect,
  x: number,
  y: number,
  viewport: Viewport,
  stripOn: boolean,
  edge: DockEdge,
): boolean {
  let left = panel.x;
  let top = panel.y;
  let right = panel.x + panel.width;
  let bottom = panel.y + panel.height;
  if (stripOn) {
    switch (edge) {
      case 'bottom':
        bottom = viewport.height;
        break;
      case 'top':
        top = 0;
        break;
      case 'left':
        left = 0;
        break;
      case 'right':
        right = viewport.width;
        break;
    }
  }
  return x >= left && x < right && y >= top && y < bottom;
}

/**
 * The reveal strip's thickness (across the edge) in `phase`, CSS px:
 * `REVEAL_STRIP_PX` while hidden (the only place the window takes the
 * pointer); on screen, the whole band from the window's edge side to the
 * panel (`gap`, the margin between them), so the pointer coming in from
 * the strip is over the panel the moment it leaves the strip. With a 0 ms
 * countdown (fullscreen, or a delay of 0) a gap between the two
 * would hide the dock on the way in. Never less than the strip.
 */
export function revealStripThickness(phase: AutoHidePhase, gap: number): number {
  return phase === 'hidden' ? REVEAL_STRIP_PX : Math.max(REVEAL_STRIP_PX, gap);
}

/**
 * The reveal strip (ADR-0008): the `REVEAL_STRIP_PX` CSS px of `viewport`
 * along its edge side (`edge`: the bottom 2 px on `bottom`, the top 2 px on
 * `top`, the leftmost on `left`, the rightmost on `right`), exactly as long
 * as the resting panel `panel` (whole CSS px, as `integerRect` gives it)
 * and in line with it.
 */
export function revealStrip(panel: Rect, viewport: Viewport, edge: DockEdge): Rect {
  switch (edge) {
    case 'bottom': {
      const height = Math.max(0, Math.min(REVEAL_STRIP_PX, viewport.height));
      return { x: panel.x, y: viewport.height - height, width: panel.width, height };
    }
    case 'top': {
      const height = Math.max(0, Math.min(REVEAL_STRIP_PX, viewport.height));
      return { x: panel.x, y: 0, width: panel.width, height };
    }
    case 'left': {
      const width = Math.max(0, Math.min(REVEAL_STRIP_PX, viewport.width));
      return { x: 0, y: panel.y, width, height: panel.height };
    }
    case 'right': {
      const width = Math.max(0, Math.min(REVEAL_STRIP_PX, viewport.width));
      return { x: viewport.width - width, y: panel.y, width, height: panel.height };
    }
  }
}

/** While hidden the strip is both rectangles: the window takes the pointer only there. */
export function hiddenHitRegion(panel: Rect, viewport: Viewport, edge: DockEdge): HitRegion {
  const strip = revealStrip(panel, viewport, edge);
  return { panel: strip, envelope: { ...strip } };
}

/**
 * No clickable area at all: 0 × 0 at the viewport's origin, which contains
 * no point, as both rectangles (docs/ipc/hit-region.md takes any
 * non-negative size; the envelope contains the panel).
 */
export const NO_HIT_REGION: Readonly<HitRegion> = Object.freeze({
  panel: Object.freeze({ x: 0, y: 0, width: 0, height: 0 }),
  envelope: Object.freeze({ x: 0, y: 0, width: 0, height: 0 }),
});

/**
 * What the dock on `edge` reports for `region` (the panel at rest and its
 * envelope) in `state`: `region` itself while auto-hide does not
 * apply; with the reveal strip (`revealStripOn`), the strip while hidden
 * (`hiddenHitRegion`) and the envelope grown out to the window's edge side
 * while shown (`shownHitRegion`); with none (fullscreen above a visible
 * taskbar), `NO_HIT_REGION` while hidden, so the window is
 * click-through everywhere, and `region` while shown.
 */
export function autoHideHitRegion(
  state: AutoHideState,
  region: HitRegion,
  viewport: Viewport,
  atScreenEdge: boolean,
  edge: DockEdge,
): HitRegion {
  if (!autoHideActive(state)) {
    return region;
  }
  const hidden = isAutoHidden(state);
  if (!revealStripOn(state, atScreenEdge)) {
    return hidden
      ? { panel: { ...NO_HIT_REGION.panel }, envelope: { ...NO_HIT_REGION.envelope } }
      : region;
  }
  return hidden
    ? hiddenHitRegion(region.panel, viewport, edge)
    : shownHitRegion(region, viewport, edge);
}

/** The smallest rect containing both. */
function union(a: Rect, b: Rect): Rect {
  const x = Math.min(a.x, b.x);
  const y = Math.min(a.y, b.y);
  return {
    x,
    y,
    width: Math.max(a.x + a.width, b.x + b.width) - x,
    height: Math.max(a.y + a.height, b.y + b.height) - y,
  };
}

/**
 * The region while auto-hide is on and the dock is shown: `region` with the
 * envelope grown to take in the reveal strip between the panel and the
 * window's edge side. A pointer that revealed the dock from the strip, or
 * went from the panel to the screen edge, so stays tracked (the window
 * keeps taking it) instead of the window turning click-through under it;
 * the panel, where the window starts taking the pointer, is unchanged.
 */
export function shownHitRegion(region: HitRegion, viewport: Viewport, edge: DockEdge): HitRegion {
  return {
    panel: region.panel,
    envelope: union(region.envelope, revealStrip(region.panel, viewport, edge)),
  };
}
