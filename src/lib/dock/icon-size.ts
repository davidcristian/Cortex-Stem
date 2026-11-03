/**
 * Which size to ask `get_icon` for. The icon box renders at its true
 * magnified size, so the source must be at least `iconSize × maxScale × DPR`
 * pixels or the magnified icon is upscaled and goes soft. Sizes come from a
 * fixed ladder so one setting change does not spawn a new cache entry per
 * pixel, and so the platform side can keep a handful of well-known sizes
 * crisp (docs/ipc/icon-keys.md).
 */

/** Ascending; the largest entry caps every request. */
export const ICON_SIZE_LADDER: readonly number[] = [32, 48, 64, 96, 128, 192, 256];

/** The smallest ladder entry that is at least `need` pixels (the largest entry if none is). */
export function pickIconSize(need: number): number {
  const top = ICON_SIZE_LADDER[ICON_SIZE_LADDER.length - 1]!;
  if (!(need > 0)) {
    // NaN, zero or negative: nothing sensible was asked for; the smallest size is safe.
    return ICON_SIZE_LADDER[0]!;
  }
  for (const size of ICON_SIZE_LADDER) {
    if (size >= need) {
      return size;
    }
  }
  return top;
}

/**
 * The request size for icons rendered at `iconSize` CSS px, magnified up to
 * `maxScale`, on a display with `dpr` device pixels per CSS px.
 */
export function iconRequestSize(iconSize: number, maxScale: number, dpr: number): number {
  // The product of three floats can land a hair above a whole number
  // (48 × 1.6 × 1.25 = 96.00000000000001); such noise must not bump the rung.
  return pickIconSize(Math.ceil(iconSize * Math.max(1, maxScale) * Math.max(1, dpr) - 1e-6));
}
