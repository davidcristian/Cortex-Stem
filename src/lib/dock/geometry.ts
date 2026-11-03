/**
 * Rectangle helpers for the sizing and hit-region protocols
 * (docs/ipc/sizing-protocol.md, docs/ipc/hit-region.md). Pure, in CSS px
 * of the dock window's viewport, so `Dock.svelte` can compute what it
 * reports without touching a backend module and the maths can be
 * unit-tested.
 */

import type { Rect } from '$lib/ipc/generated/Rect';

/** Round a rect outwards to whole CSS pixels (left/top down, right/bottom up). */
export function integerRect(rect: Rect): Rect {
  const x = Math.floor(rect.x);
  const y = Math.floor(rect.y);
  return {
    x,
    y,
    width: Math.max(0, Math.ceil(rect.x + rect.width) - x),
    height: Math.max(0, Math.ceil(rect.y + rect.height) - y),
  };
}

export function sameRect(a: Rect, b: Rect): boolean {
  return a.x === b.x && a.y === b.y && a.width === b.width && a.height === b.height;
}

/** `inner` lies entirely within `outer` (edges may touch). */
export function rectContains(outer: Rect, inner: Rect): boolean {
  return (
    inner.x >= outer.x &&
    inner.y >= outer.y &&
    inner.x + inner.width <= outer.x + outer.width &&
    inner.y + inner.height <= outer.y + outer.height
  );
}

/** What `set_dock_hit_region` takes: the panel at rest and the envelope magnified icons can reach. */
export interface HitRegion {
  panel: Rect;
  envelope: Rect;
}

export function sameHitRegion(a: HitRegion, b: HitRegion): boolean {
  return sameRect(a.panel, b.panel) && sameRect(a.envelope, b.envelope);
}

/**
 * The hit region for a panel whose rest bounds are `panel` (fractional CSS
 * px are fine): the panel rounded outwards, and the envelope that is the
 * panel widened by `growth` (half on each side: the panel is centred, so
 * magnification pushes both ends out) and raised by `rise` (how far a
 * magnified icon can stick out above the panel, plus slack). The envelope's
 * bottom edge is the panel's bottom edge: nothing grows downwards.
 */
export function hitRegionFor(panel: Rect, growth: number, rise: number): HitRegion {
  const g = Math.max(0, growth);
  const r = Math.max(0, rise);
  return {
    panel: integerRect(panel),
    envelope: integerRect({
      x: panel.x - g / 2,
      y: panel.y - r,
      width: panel.width + g,
      height: panel.height + r,
    }),
  };
}

/** How far a magnified icon of `iconSize` px rises above the panel, with `slack` px to spare. */
export function magnifiedRise(iconSize: number, maxScale: number, slack: number): number {
  return iconSize * Math.max(0, maxScale - 1) + slack;
}

/** The inline margins (CSS px) that place the panel on whole device pixels. */
export interface SnapMargins {
  marginLeft: number;
  marginBottom: number;
}

/**
 * The margins that move a panel whose top-left corner is at (`left`, `top`)
 * CSS px onto a whole device pixel, so that the bitmaps inside it are drawn
 * 1:1 instead of being resampled by a fraction of a pixel.
 *
 * The panel is centred by its flex container, so a left margin of m moves
 * it by m/2; it is anchored to the bottom, so its top edge follows the
 * bottom margin one to one (a smaller bottom margin moves the top edge
 * down). Both are corrections of the margins the panel has *now*: calling
 * this again with the snapped rect and the margins it returned changes
 * nothing. The fractional part is cancelled towards the nearest device
 * pixel, so the panel never moves by more than half a device pixel.
 */
export function snapOffsets(
  left: number,
  top: number,
  dpr: number,
  currentMarginLeft: number,
  currentMarginBottom: number,
): SnapMargins {
  const scale = Number.isFinite(dpr) && dpr > 0 ? dpr : 1;
  const dx = (Math.round(left * scale) - left * scale) / scale;
  const dy = (Math.round(top * scale) - top * scale) / scale;
  return { marginLeft: currentMarginLeft + 2 * dx, marginBottom: currentMarginBottom - dy };
}

/**
 * Chromium lays out in 1/64 CSS px (LayoutUnit). At a fractional device
 * pixel ratio the ideal margins are not on that grid: the margin is
 * rounded to it (≤ ½ unit, ¼ unit on the position once the centring halves
 * it) and the centring's division floors (< ½ unit), so the laid-out edge
 * misses the ideal one by < ¾ unit and the next measurement asks for a
 * correction of < 1½ units (horizontal: twice the miss). Applying it would
 * only flip the quantised margin to its neighbour and back, so a change of
 * up to two units is "the same" margins: nothing visible can come of it
 * (two units are 1/32 of a device pixel at 100 %, 3/64 at 150 %).
 */
export const LAYOUT_UNIT = 1 / 64;

/** `true` when applying `next` instead of `current` could not move the panel (see `LAYOUT_UNIT`). */
export function sameSnap(current: SnapMargins, next: SnapMargins): boolean {
  return (
    Math.abs(next.marginLeft - current.marginLeft) <= 2 * LAYOUT_UNIT &&
    Math.abs(next.marginBottom - current.marginBottom) <= 2 * LAYOUT_UNIT
  );
}
