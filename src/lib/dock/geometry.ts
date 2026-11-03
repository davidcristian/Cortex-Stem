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
