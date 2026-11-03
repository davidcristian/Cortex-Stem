import { describe, expect, it } from 'vitest';

import {
  hitRegionFor,
  integerRect,
  magnifiedRise,
  rectContains,
  sameHitRegion,
  sameRect,
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
