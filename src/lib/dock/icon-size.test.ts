import { describe, expect, it } from 'vitest';

import { ICON_SIZE_LADDER, iconRequestSize, pickIconSize } from './icon-size';

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
