import { describe, expect, it } from 'vitest';

import {
  hitRegionFor,
  integerRect,
  LAYOUT_UNIT,
  magnifiedRise,
  rectContains,
  sameHitRegion,
  sameRect,
  sameSnap,
  snapOffsets,
} from './geometry';

describe('integerRect', () => {
  it('rounds outwards so the rect still covers the original', () => {
    expect(integerRect({ x: 10.4, y: 20.6, width: 30.2, height: 5.1 })).toEqual({
      x: 10,
      y: 20,
      width: 31, // right edge 40.6 → 41
      height: 6, // bottom edge 25.7 → 26
    });
  });

  it('leaves whole-pixel rects alone', () => {
    const r = { x: 3, y: 4, width: 50, height: 60 };
    expect(integerRect(r)).toEqual(r);
  });

  it('never produces a negative size', () => {
    expect(integerRect({ x: 1, y: 1, width: -3, height: -3 })).toEqual({
      x: 1,
      y: 1,
      width: 0,
      height: 0,
    });
  });

  it('handles negative coordinates', () => {
    expect(integerRect({ x: -0.5, y: -2.5, width: 1, height: 1 })).toEqual({
      x: -1,
      y: -3,
      width: 2,
      height: 2,
    });
  });
});

describe('sameRect / rectContains', () => {
  it('compares every field', () => {
    const a = { x: 1, y: 2, width: 3, height: 4 };
    expect(sameRect(a, { ...a })).toBe(true);
    expect(sameRect(a, { ...a, height: 5 })).toBe(false);
  });

  it('containment allows touching edges', () => {
    const outer = { x: 0, y: 0, width: 100, height: 50 };
    expect(rectContains(outer, outer)).toBe(true);
    expect(rectContains(outer, { x: 10, y: 10, width: 90, height: 40 })).toBe(true);
    expect(rectContains(outer, { x: 10, y: 10, width: 91, height: 40 })).toBe(false);
    expect(rectContains(outer, { x: -1, y: 10, width: 10, height: 10 })).toBe(false);
  });
});

describe('hitRegionFor', () => {
  const panel = { x: 390.5, y: 168, width: 499, height: 64 };

  it('widens by half the growth on each side and raises by the rise, bottom fixed', () => {
    const { panel: p, envelope: e } = hitRegionFor(panel, 117.6, 36.8);
    expect(p).toEqual({ x: 390, y: 168, width: 500, height: 64 });
    // left edge 390.5 - 58.8 = 331.7 → 331; right edge 889.5 + 58.8 = 948.3 → 949
    expect(e).toEqual({ x: 331, y: 131, width: 618, height: 101 });
    expect(e.y + e.height).toBe(p.y + p.height);
  });

  it('always contains the panel', () => {
    for (const [g, r] of [
      [0, 0],
      [0, 8],
      [117.6, 36.8],
      [1, 1],
      [-5, -5],
    ] as const) {
      const region = hitRegionFor(panel, g, r);
      expect(rectContains(region.envelope, region.panel)).toBe(true);
    }
  });

  it('is the panel itself when nothing grows', () => {
    const region = hitRegionFor({ x: 10, y: 20, width: 30, height: 40 }, 0, 0);
    expect(sameHitRegion(region, { panel: region.panel, envelope: region.panel })).toBe(true);
  });
});

describe('magnifiedRise', () => {
  it('is the extra height of a fully magnified icon plus the slack', () => {
    expect(magnifiedRise(48, 1.6, 8)).toBeCloseTo(36.8);
    expect(magnifiedRise(48, 1, 8)).toBe(8);
    expect(magnifiedRise(48, 0.5, 8)).toBe(8);
  });
});

describe('snapOffsets', () => {
  const BOTTOM = 8;

  /** Where the panel lands once `margins` replace `current` (centred: half the left margin). */
  function apply(
    left: number,
    top: number,
    current: { marginLeft: number; marginBottom: number },
    margins: { marginLeft: number; marginBottom: number },
  ): { left: number; top: number } {
    return {
      left: left + (margins.marginLeft - current.marginLeft) / 2,
      top: top + (current.marginBottom - margins.marginBottom),
    };
  }

  it('leaves a panel that already sits on whole device pixels alone', () => {
    expect(snapOffsets(360, 168, 1, 0, BOTTOM)).toEqual({ marginLeft: 0, marginBottom: BOTTOM });
    // 360.5 CSS px is 721 device px at 2×: whole.
    expect(snapOffsets(360.5, 168, 2, 0, BOTTOM)).toEqual({ marginLeft: 0, marginBottom: BOTTOM });
    expect(snapOffsets(360, 168, 1.5, 0, BOTTOM)).toEqual({ marginLeft: 0, marginBottom: BOTTOM });
  });

  it('doubles the horizontal correction, because a flex-centred panel moves by half its margin', () => {
    // A 713 px panel centred in a 2560 px window at 100 %.
    const margins = snapOffsets(923.5, 168, 1, 0, BOTTOM);
    expect(margins).toEqual({ marginLeft: 1, marginBottom: BOTTOM });
    expect(apply(923.5, 168, { marginLeft: 0, marginBottom: BOTTOM }, margins).left).toBe(924);
  });

  it('corrects the top edge through the bottom margin, one to one', () => {
    // 169 CSS px is 253.5 device px at 1.5×; the nearest whole is 254, a third of a CSS px down.
    const margins = snapOffsets(360, 169, 1.5, 0, BOTTOM);
    expect(margins.marginLeft).toBe(0);
    expect(margins.marginBottom).toBeCloseTo(BOTTOM - 1 / 3, 12);
    const { top } = apply(360, 169, { marginLeft: 0, marginBottom: BOTTOM }, margins);
    expect(top * 1.5).toBeCloseTo(254, 12);
  });

  it('works at any device pixel ratio', () => {
    for (const dpr of [1, 1.25, 1.5, 1.75, 2, 2.5, 3]) {
      for (const [left, top] of [
        [360.5, 168],
        [923.5, 113.328125],
        [100.25, 50.75],
        [0.1, 0.9],
        [-12.5, 3],
      ] as const) {
        const current = { marginLeft: 0, marginBottom: BOTTOM };
        const margins = snapOffsets(left, top, dpr, 0, BOTTOM);
        const landed = apply(left, top, current, margins);
        expect(landed.left * dpr).toBeCloseTo(Math.round(landed.left * dpr), 9);
        expect(landed.top * dpr).toBeCloseTo(Math.round(landed.top * dpr), 9);
        // Never more than half a device pixel either way.
        expect(Math.abs(landed.left - left) * dpr).toBeLessThanOrEqual(0.5 + 1e-9);
        expect(Math.abs(landed.top - top) * dpr).toBeLessThanOrEqual(0.5 + 1e-9);
      }
    }
  });

  it('is idempotent: measuring the snapped panel asks for the same margins again', () => {
    const current = { marginLeft: 0, marginBottom: BOTTOM };
    const first = snapOffsets(360.5, 169, 1.5, current.marginLeft, current.marginBottom);
    const landed = apply(360.5, 169, current, first);
    const second = snapOffsets(landed.left, landed.top, 1.5, first.marginLeft, first.marginBottom);
    expect(second.marginLeft).toBeCloseTo(first.marginLeft, 12);
    expect(second.marginBottom).toBeCloseTo(first.marginBottom, 12);
  });

  it('builds on the margins the panel already has', () => {
    // Already nudged by 1 px, now half a pixel off again (an item came and went).
    expect(snapOffsets(200.5, 168, 1, 1, BOTTOM)).toEqual({ marginLeft: 2, marginBottom: BOTTOM });
    expect(snapOffsets(200.5, 168, 1, -1, BOTTOM)).toEqual({ marginLeft: 0, marginBottom: BOTTOM });
    expect(snapOffsets(200, 168.5, 1, 0, 7.5)).toEqual({ marginLeft: 0, marginBottom: 7 });
  });

  it('treats a nonsense device pixel ratio as 1', () => {
    expect(snapOffsets(10.5, 20, 0, 0, BOTTOM)).toEqual({ marginLeft: 1, marginBottom: BOTTOM });
    expect(snapOffsets(10.5, 20, NaN, 0, BOTTOM)).toEqual({ marginLeft: 1, marginBottom: BOTTOM });
    expect(snapOffsets(10.5, 20, -2, 0, BOTTOM)).toEqual({ marginLeft: 1, marginBottom: BOTTOM });
  });
});

describe('sameSnap', () => {
  it('ignores corrections within two layout units, which could not move the panel', () => {
    const a = { marginLeft: 1 / 3, marginBottom: 8 - 1 / 3 };
    expect(sameSnap(a, a)).toBe(true);
    expect(sameSnap(a, { ...a, marginLeft: a.marginLeft + LAYOUT_UNIT })).toBe(true);
    // Measured at 150 %: the laid-out 1/3 px margin asked for 1/48 px (1.33 units) more.
    expect(sameSnap(a, { ...a, marginLeft: a.marginLeft + 1 / 48 })).toBe(true);
    expect(sameSnap(a, { ...a, marginLeft: a.marginLeft - 2 * LAYOUT_UNIT })).toBe(true);
    expect(sameSnap(a, { ...a, marginBottom: a.marginBottom - LAYOUT_UNIT })).toBe(true);
  });

  it('sees a real correction', () => {
    const a = { marginLeft: 0, marginBottom: 8 };
    expect(sameSnap(a, { marginLeft: 1, marginBottom: 8 })).toBe(false);
    expect(sameSnap(a, { marginLeft: 0, marginBottom: 7.5 })).toBe(false);
    expect(sameSnap(a, { marginLeft: 3 * LAYOUT_UNIT, marginBottom: 8 })).toBe(false);
    // The smallest real snap at 200 %: a quarter CSS px, doubled.
    expect(sameSnap(a, { marginLeft: 0.5, marginBottom: 8 })).toBe(false);
  });
});
