import { describe, expect, it } from 'vitest';

import { ICON_SIZE_LADDER, iconRequestSize, iconSources, pickIconSize } from './icon-size';

describe('pickIconSize', () => {
  it('returns the smallest ladder entry that covers the need', () => {
    expect(pickIconSize(1)).toBe(32);
    expect(pickIconSize(32)).toBe(32);
    expect(pickIconSize(33)).toBe(48);
    expect(pickIconSize(48)).toBe(48);
    expect(pickIconSize(77)).toBe(96);
    expect(pickIconSize(96)).toBe(96);
    expect(pickIconSize(97)).toBe(128);
    expect(pickIconSize(154)).toBe(192);
    expect(pickIconSize(256)).toBe(256);
  });

  it('caps at the largest entry', () => {
    expect(pickIconSize(257)).toBe(256);
    expect(pickIconSize(10_000)).toBe(256);
    expect(pickIconSize(Infinity)).toBe(256);
  });

  it('falls back to the smallest entry for nonsense', () => {
    expect(pickIconSize(0)).toBe(32);
    expect(pickIconSize(-5)).toBe(32);
    expect(pickIconSize(NaN)).toBe(32);
  });

  it('only ever returns ladder entries', () => {
    for (let need = 1; need <= 300; need += 1) {
      const size = pickIconSize(need);
      expect(ICON_SIZE_LADDER).toContain(size);
      expect(size >= need || size === 256).toBe(true);
    }
  });
});

describe('iconRequestSize', () => {
  it('covers icon size × magnification × DPR, rounded up', () => {
    // The defaults: 48 px icons, 1.6× magnification.
    expect(iconRequestSize(48, 1.6, 1)).toBe(96); // 76.8 → 77
    expect(iconRequestSize(48, 1.6, 2)).toBe(192); // 153.6 → 154
    expect(iconRequestSize(48, 1.6, 1.5)).toBe(128); // 115.2 → 116
    expect(iconRequestSize(48, 1.6, 1.25)).toBe(96); // 96 exactly
  });

  it('ignores magnification below 1 and DPR below 1', () => {
    expect(iconRequestSize(48, 1, 1)).toBe(48);
    expect(iconRequestSize(48, 0.5, 1)).toBe(48);
    expect(iconRequestSize(48, 1, 0.75)).toBe(48);
    expect(iconRequestSize(64, 1, 1)).toBe(64);
    expect(iconRequestSize(65, 1, 1)).toBe(96);
  });

  it('caps large settings at the top of the ladder', () => {
    expect(iconRequestSize(128, 2.5, 2)).toBe(256);
  });
});

describe('iconSources', () => {
  it('requests the exact device size for rest and the ladder size for magnification', () => {
    // The defaults (48 px, 1.6×) at 100 %, 150 % and 200 %.
    expect(iconSources(48, 1.6, 1)).toEqual({ rest: 48, magnified: 96 });
    expect(iconSources(48, 1.6, 1.5)).toEqual({ rest: 72, magnified: 128 });
    expect(iconSources(48, 1.6, 2)).toEqual({ rest: 96, magnified: 192 });
    // 39 px icons: 39 at rest (not the 64 of the ladder), 64 magnified.
    expect(iconSources(39, 1.6, 1)).toEqual({ rest: 39, magnified: 64 });
    expect(iconSources(39, 1.6, 2)).toEqual({ rest: 78, magnified: 128 });
  });

  it('rounds the rest size to whole pixels', () => {
    expect(iconSources(39, 1.6, 1.5)).toEqual({ rest: 59, magnified: 96 }); // 58.5
    expect(iconSources(41, 1.6, 1.25)).toEqual({ rest: 51, magnified: 96 }); // 51.25
    expect(iconSources(45, 1.6, 1.25)).toEqual({ rest: 56, magnified: 96 }); // 56.25
  });

  it('uses one bitmap when magnification is off or reduced motion turned it off', () => {
    expect(iconSources(48, 1, 1)).toEqual({ rest: 48, magnified: 48 });
    expect(iconSources(48, 1, 1.5)).toEqual({ rest: 72, magnified: 72 }); // not the ladder's 96
    expect(iconSources(48, 1, 2)).toEqual({ rest: 96, magnified: 96 });
    expect(iconSources(39, 1, 1.5)).toEqual({ rest: 59, magnified: 59 });
    expect(iconSources(48, 0.5, 1)).toEqual({ rest: 48, magnified: 48 });
  });

  it('uses one bitmap when the two sizes coincide at the top of the ladder', () => {
    expect(iconSources(128, 2, 2)).toEqual({ rest: 256, magnified: 256 });
  });

  it('never asks for less than a pixel and treats a nonsense DPR as 1', () => {
    expect(iconSources(48, 1.6, 0)).toEqual({ rest: 48, magnified: 96 });
    expect(iconSources(48, 1.6, NaN)).toEqual({ rest: 48, magnified: 96 });
    expect(iconSources(0, 1, 1).rest).toBe(1);
  });
});
