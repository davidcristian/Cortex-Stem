/**
 * Magnification maths: pure functions so the per-frame controller in
 * `magnifier.svelte.ts` does nothing but measure, call these, and write
 * transforms.
 *
 * Everything runs along the dock's main axis (`edge.ts`): the
 * pointer's x and the slots' horizontal centres on a dock on `bottom` or
 * `top`, their y and vertical centres on `left` or `right`. How far the
 * pointer is from the dock across that axis does not change a scale.
 */

import { along, centreAlong, type Box, type DockAxis } from './edge';

export interface MagnifyParams {
  /** Scale of the icon under the pointer (1 disables magnification). */
  maxScale: number;
  /** Neighbours on each side that are affected. */
  rangeIcons: number;
}

/**
 * Scale for an item whose centre is `distancePx` from the pointer along the
 * main axis, with `slotPx` the unmagnified slot length. A raised-cosine
 * bell: `maxScale` under the pointer, back to 1 exactly `rangeIcons` slots
 * away.
 */
export function scaleAt(distancePx: number, slotPx: number, p: MagnifyParams): number {
  const range = p.rangeIcons * slotPx;
  if (p.maxScale <= 1 || range <= 0) {
    return 1;
  }
  const d = Math.abs(distancePx);
  if (d >= range) {
    return 1;
  }
  return 1 + (p.maxScale - 1) * 0.5 * (1 + Math.cos((Math.PI * d) / range));
}

/**
 * Fill `out[i]` with the scale for the item centred at `centres[i]` (along
 * `axis`, `slotCentres`) while the pointer is at (`x`, `y`), viewport CSS
 * px: only its coordinate along `axis` counts. Both arrays have the same
 * length; nothing is allocated.
 */
export function computeScales(
  axis: DockAxis,
  x: number,
  y: number,
  centres: ArrayLike<number>,
  slotPx: number,
  p: MagnifyParams,
  out: Float64Array,
): void {
  const pointer = along(axis, x, y);
  for (let i = 0; i < out.length; i += 1) {
    out[i] = scaleAt(pointer - (centres[i] ?? 0), slotPx, p);
  }
}

/**
 * Fill `out[i]` with the centre of `boxes[i]` along `axis` (where the slot is
 * drawn now: `getBoundingClientRect`). `out` is as long as `boxes`.
 */
export function slotCentres(axis: DockAxis, boxes: ArrayLike<Box>, out: Float64Array): void {
  for (let i = 0; i < out.length; i += 1) {
    const box = boxes[i];
    out[i] = box === undefined ? 0 : centreAlong(axis, box);
  }
}

/**
 * Upper bound on the extra length magnification adds to the panel along the
 * main axis (its width on a horizontal dock, its height on a vertical one).
 * The bell
 * sampled at slot spacing sums to exactly `rangeIcons` whatever the pointer
 * offset, so the exact growth is `(maxScale - 1) * slotPx * rangeIcons`;
 * half a slot of margin covers rounding.
 */
export function maxGrowth(slotPx: number, p: MagnifyParams): number {
  if (p.maxScale <= 1) {
    return 0;
  }
  return (p.maxScale - 1) * slotPx * (p.rangeIcons + 0.5);
}
