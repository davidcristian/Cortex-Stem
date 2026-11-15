/**
 * Rectangle helpers for the sizing and hit-region protocols
 * (docs/ipc/sizing-protocol.md, docs/ipc/hit-region.md). Pure, in CSS px
 * of the dock window's viewport, so `Dock.svelte` can compute what it
 * reports without touching a backend module and the maths can be
 * unit-tested.
 */

import type { Rect } from '$lib/ipc/generated/Rect';

import { across, along, awaySign, axisOf, type DockEdge } from './edge';

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

/** The point (`x`, `y`) lies within `rect`: left and top edges inclusive, right and bottom exclusive. */
export function rectContainsPoint(rect: Rect, x: number, y: number): boolean {
  return x >= rect.x && x < rect.x + rect.width && y >= rect.y && y < rect.y + rect.height;
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
 * px are fine) on `edge`: the panel rounded outwards, and the envelope that
 * is the panel lengthened by `growth` along the main axis (half at each
 * end: the panel is centred, so magnification pushes both ends out) and
 * grown by `rise` away from the edge (how far a magnified icon can stick
 * out beyond the panel, plus slack). The envelope's edge side is the
 * panel's: nothing grows towards the edge.
 */
export function hitRegionFor(panel: Rect, growth: number, rise: number, edge: DockEdge): HitRegion {
  const g = Math.max(0, growth);
  const r = Math.max(0, rise);
  const lengthened: Rect =
    axisOf(edge) === 'horizontal'
      ? { ...panel, x: panel.x - g / 2, width: panel.width + g }
      : { ...panel, y: panel.y - g / 2, height: panel.height + g };
  let envelope: Rect;
  switch (edge) {
    case 'bottom':
      envelope = { ...lengthened, y: panel.y - r, height: panel.height + r };
      break;
    case 'top':
      envelope = { ...lengthened, height: panel.height + r };
      break;
    case 'left':
      envelope = { ...lengthened, width: panel.width + r };
      break;
    case 'right':
      envelope = { ...lengthened, x: panel.x - r, width: panel.width + r };
      break;
  }
  return { panel: integerRect(panel), envelope: integerRect(envelope) };
}

/** A viewport's size, CSS px (`window.innerWidth` × `window.innerHeight`). */
export interface Viewport {
  width: number;
  height: number;
}

/**
 * How far a point that stays put on screen moves in viewport coordinates
 * when the dock window goes from `before` to `after` (CSS px). The window
 * keeps its edge side where it is (docs/ipc/sizing-protocol.md; a drag
 * grows it away from the edge): on `bottom` its top edge moves, so a
 * point's y changes by the change in height; on `right` its left edge
 * moves, so x changes by the change in width; on `top` and `left` the
 * origin stays.
 */
export function edgeShift(
  edge: DockEdge,
  before: Viewport,
  after: Viewport,
): { dx: number; dy: number } {
  switch (edge) {
    case 'bottom':
      return { dx: 0, dy: after.height - before.height };
    case 'right':
      return { dx: after.width - before.width, dy: 0 };
    case 'top':
    case 'left':
      return { dx: 0, dy: 0 };
  }
}

/** How far a magnified icon of `iconSize` px sticks out beyond the panel (away from the edge), with `slack` px to spare. */
export function magnifiedRise(iconSize: number, maxScale: number, slack: number): number {
  return iconSize * Math.max(0, maxScale - 1) + slack;
}

/**
 * The inline margins (CSS px) that place the panel on whole device pixels:
 * `start`, the margin at the start of the main axis (`margin-left`
 * on a horizontal dock, `margin-top` on a vertical one), and `edge`, the
 * margin on the edge side (`margin-bottom` on `bottom`, and so on;
 * `snapMarginProperties`).
 */
export interface SnapMargins {
  start: number;
  edge: number;
}

/** The CSS properties (`CSSStyleDeclaration` names) the two margins of `SnapMargins` go to on `edge`. */
export function snapMarginProperties(edge: DockEdge): {
  start: 'marginLeft' | 'marginTop';
  edge: 'marginBottom' | 'marginTop' | 'marginLeft' | 'marginRight';
} {
  switch (edge) {
    case 'bottom':
      return { start: 'marginLeft', edge: 'marginBottom' };
    case 'top':
      return { start: 'marginLeft', edge: 'marginTop' };
    case 'left':
      return { start: 'marginTop', edge: 'marginLeft' };
    case 'right':
      return { start: 'marginTop', edge: 'marginRight' };
  }
}

/**
 * The margins that move a panel whose top-left corner is at (`left`, `top`)
 * CSS px onto a whole device pixel, so that the bitmaps inside it are drawn
 * 1:1 instead of being resampled by a fraction of a pixel.
 *
 * The panel is centred along the main axis by its flex container, so a
 * start margin of m moves it by m/2; it is anchored to the edge side, so
 * its corner follows the edge margin one to one: towards larger
 * coordinates on `top` and `left` (a larger top or left margin moves it
 * down or right), towards smaller ones on `bottom` and `right` (a smaller
 * bottom margin moves the top edge down). Both are corrections of the
 * margins the panel has *now* (`current`): calling this again with the
 * snapped rect and the margins it returned changes nothing. The fractional
 * part is cancelled towards the nearest device pixel, so the panel never
 * moves by more than half a device pixel.
 */
export function snapOffsets(
  left: number,
  top: number,
  dpr: number,
  current: SnapMargins,
  edge: DockEdge,
): SnapMargins {
  const scale = Number.isFinite(dpr) && dpr > 0 ? dpr : 1;
  const correction = (v: number): number => (Math.round(v * scale) - v * scale) / scale;
  const axis = axisOf(edge);
  const dMain = correction(along(axis, left, top));
  const dCross = correction(across(axis, left, top));
  return { start: current.start + 2 * dMain, edge: current.edge + awaySign(edge) * dCross };
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
    Math.abs(next.start - current.start) <= 2 * LAYOUT_UNIT &&
    Math.abs(next.edge - current.edge) <= 2 * LAYOUT_UNIT
  );
}
