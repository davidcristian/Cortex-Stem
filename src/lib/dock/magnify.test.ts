import { describe, expect, it } from 'vitest';

import { computeScales, maxGrowth, scaleAt } from './magnify';

const P = { maxScale: 1.6, rangeIcons: 3 };
const SLOT = 56;

describe('scaleAt', () => {
  it('peaks under the pointer and returns to 1 at the range edge', () => {
    expect(scaleAt(0, SLOT, P)).toBeCloseTo(1.6);
    expect(scaleAt(3 * SLOT, SLOT, P)).toBe(1);
    expect(scaleAt(10 * SLOT, SLOT, P)).toBe(1);
  });

  it('is symmetric and strictly decreasing with distance', () => {
    let previous = scaleAt(0, SLOT, P);
    for (let d = 4; d < 3 * SLOT; d += 4) {
      const s = scaleAt(d, SLOT, P);
      expect(scaleAt(-d, SLOT, P)).toBeCloseTo(s);
      expect(s).toBeLessThan(previous);
      expect(s).toBeGreaterThan(1);
      previous = s;
    }
  });

  it('is flat when magnification is off', () => {
    expect(scaleAt(0, SLOT, { maxScale: 1, rangeIcons: 3 })).toBe(1);
    expect(scaleAt(0, SLOT, { maxScale: 0.5, rangeIcons: 3 })).toBe(1);
    expect(scaleAt(0, SLOT, { maxScale: 2, rangeIcons: 0 })).toBe(1);
  });
});

describe('computeScales', () => {
  it('writes one scale per centre without allocating', () => {
    const centres = new Float64Array([0, SLOT, 2 * SLOT, 3 * SLOT, 4 * SLOT]);
    const out = new Float64Array(5);
    computeScales(2 * SLOT, centres, SLOT, P, out);
    expect(out[2]).toBeCloseTo(1.6);
    expect(out[1]).toBeCloseTo(out[3]!);
    expect(out[0]).toBeCloseTo(out[4]!);
    expect(out[0]).toBeGreaterThan(1);
  });

  it('adds the same total width wherever the pointer sits between slots', () => {
    // The raised cosine is a partition of unity at slot spacing: the sum of
    // (scale - 1) is constant, so the panel grows by the same amount for any
    // pointer offset and `maxGrowth` bounds it.
    const n = 12;
    const centres = Float64Array.from({ length: n }, (_, i) => i * SLOT);
    const out = new Float64Array(n);
    const sums: number[] = [];
    for (let frac = 0; frac < 1; frac += 0.125) {
      computeScales((5 + frac) * SLOT, centres, SLOT, P, out);
      let extra = 0;
      for (const s of out) {
        extra += (s - 1) * SLOT;
      }
      sums.push(extra);
    }
    const expected = (P.maxScale - 1) * SLOT * P.rangeIcons;
    for (const sum of sums) {
      expect(sum).toBeCloseTo(expected, 6);
      expect(sum).toBeLessThanOrEqual(maxGrowth(SLOT, P));
    }
  });
});

describe('maxGrowth', () => {
  it('is zero when magnification is off and scales with range and size', () => {
    expect(maxGrowth(SLOT, { maxScale: 1, rangeIcons: 3 })).toBe(0);
    expect(maxGrowth(SLOT, P)).toBeCloseTo(0.6 * SLOT * 3.5);
    expect(maxGrowth(SLOT, { ...P, rangeIcons: 6 })).toBeGreaterThan(maxGrowth(SLOT, P));
  });
});
