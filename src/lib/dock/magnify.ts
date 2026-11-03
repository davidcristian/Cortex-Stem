/**
 * Magnification maths: pure functions so the per-frame controller in
 * `Dock.svelte` does nothing but measure, call these, and write transforms.
 */

export interface MagnifyParams {
  /** Scale of the icon under the pointer (1 disables magnification). */
  maxScale: number;
  /** Neighbours on each side that are affected. */
  rangeIcons: number;
}

/**
 * Scale for an item whose centre is `distancePx` from the pointer, with
 * `slotPx` the unmagnified slot width. A raised-cosine bell: `maxScale`
 * under the pointer, back to 1 exactly `rangeIcons` slots away.
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
 * Fill `out[i]` with the scale for the item centred at `centres[i]`.
 * Both arrays have the same length; nothing is allocated.
 */
export function computeScales(
  pointerX: number,
  centres: ArrayLike<number>,
  slotPx: number,
  p: MagnifyParams,
  out: Float64Array,
): void {
  for (let i = 0; i < out.length; i += 1) {
    out[i] = scaleAt(pointerX - (centres[i] ?? 0), slotPx, p);
  }
}

/**
 * Upper bound on the extra width magnification adds to the panel. The bell
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
