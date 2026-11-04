import { describe, expect, it } from 'vitest';

import {
  DRAG_START,
  exceedsMoveThreshold,
  gapShift,
  insertionIndex,
  isInsideWithSlack,
  moveItem,
} from './reorder';

// Five pinned slots, 56 px apart, centred at 28, 84, 140, 196, 252.
const SLOT = 56;
const CENTRES = Float64Array.from({ length: 5 }, (_, i) => SLOT * i + SLOT / 2);

describe('exceedsMoveThreshold', () => {
  it('compares the straight-line distance with the threshold', () => {
    expect(exceedsMoveThreshold(0, 0, DRAG_START.movePx)).toBe(false);
    expect(exceedsMoveThreshold(5, 0, 6)).toBe(false);
    expect(exceedsMoveThreshold(6, 0, 6)).toBe(true);
    expect(exceedsMoveThreshold(-4, 5, 6)).toBe(true);
    expect(exceedsMoveThreshold(3, 3, 6)).toBe(false);
  });

  it('starts after 150 ms or 6 px by default', () => {
    expect(DRAG_START).toEqual({ holdMs: 150, movePx: 6 });
  });
});

describe('insertionIndex', () => {
  it('keeps the index while the centre stays within its own slot', () => {
    expect(insertionIndex(CENTRES[2]!, CENTRES, 2)).toBe(2);
    expect(insertionIndex(CENTRES[2]! - 20, CENTRES, 2)).toBe(2);
    expect(insertionIndex(CENTRES[2]! + 20, CENTRES, 2)).toBe(2);
  });

  it('moves one position per neighbour centre crossed, in both directions', () => {
    expect(insertionIndex(CENTRES[3]! + 1, CENTRES, 2)).toBe(3);
    expect(insertionIndex(CENTRES[4]! + 1, CENTRES, 2)).toBe(4);
    expect(insertionIndex(CENTRES[1]! - 1, CENTRES, 2)).toBe(1);
    expect(insertionIndex(CENTRES[0]! - 1, CENTRES, 2)).toBe(0);
  });

  it('clamps to the ends however far the pointer goes', () => {
    expect(insertionIndex(-1000, CENTRES, 2)).toBe(0);
    expect(insertionIndex(1000, CENTRES, 2)).toBe(4);
    expect(insertionIndex(1000, CENTRES, 4)).toBe(4);
    expect(insertionIndex(-1000, CENTRES, 0)).toBe(0);
  });

  it('is 0 for a single item', () => {
    expect(insertionIndex(999, Float64Array.of(28), 0)).toBe(0);
  });

  it('ranges over 0..=n when nothing is lifted (from = -1)', () => {
    expect(insertionIndex(-1000, CENTRES, -1)).toBe(0);
    expect(insertionIndex(CENTRES[2]! + 1, CENTRES, -1)).toBe(3);
    expect(insertionIndex(1000, CENTRES, -1)).toBe(5);
  });
});

describe('gapShift', () => {
  const shifts = (from: number, to: number): number[] =>
    CENTRES.length === 5 ? [0, 1, 2, 3, 4].map((i) => gapShift(i, from, to, SLOT)) : [];

  it('is all zero while the gap is at the lifted item', () => {
    expect(shifts(2, 2)).toEqual([0, 0, 0, 0, 0]);
  });

  it('shifts the items between the origin and a later gap one slot left', () => {
    expect(shifts(0, 2)).toEqual([0, -SLOT, -SLOT, 0, 0]);
    expect(shifts(1, 4)).toEqual([0, 0, -SLOT, -SLOT, -SLOT]);
  });

  it('shifts the items between an earlier gap and the origin one slot right', () => {
    expect(shifts(4, 2)).toEqual([0, 0, SLOT, SLOT, 0]);
    expect(shifts(3, 0)).toEqual([SLOT, SLOT, SLOT, 0, 0]);
  });

  it('never moves the lifted item', () => {
    for (let from = 0; from < 5; from += 1) {
      for (let to = 0; to < 5; to += 1) {
        expect(gapShift(from, from, to, SLOT)).toBe(0);
      }
    }
  });

  it('matches moveItem: the gap is where the item lands', () => {
    // After the move, every shifted neighbour sits where its shift pointed.
    const keys = ['a', 'b', 'c', 'd', 'e'];
    for (let from = 0; from < 5; from += 1) {
      for (let to = 0; to < 5; to += 1) {
        const moved = moveItem(keys, from, to);
        keys.forEach((key, i) => {
          if (i === from) {
            expect(moved[to]).toBe(key);
            return;
          }
          const landed = i + gapShift(i, from, to, SLOT) / SLOT;
          expect(moved[landed]).toBe(key);
        });
      }
    }
  });
});

describe('moveItem', () => {
  it('moves forwards and backwards without touching the input', () => {
    const list = ['a', 'b', 'c', 'd'];
    expect(moveItem(list, 0, 2)).toEqual(['b', 'c', 'a', 'd']);
    expect(moveItem(list, 3, 1)).toEqual(['a', 'd', 'b', 'c']);
    expect(list).toEqual(['a', 'b', 'c', 'd']);
  });

  it('returns a copy for a no-op or an out-of-range index', () => {
    const list = ['a', 'b'];
    expect(moveItem(list, 1, 1)).toEqual(list);
    expect(moveItem(list, 1, 1)).not.toBe(list);
    expect(moveItem(list, 5, 0)).toEqual(list);
    expect(moveItem(list, 0, -1)).toEqual(list);
  });
});

describe('isInsideWithSlack', () => {
  const bounds = { left: 100, right: 300, top: 50, bottom: 100 };

  it('accepts points inside the grown box and rejects the rest', () => {
    expect(isInsideWithSlack(200, 75, bounds, 28, 48)).toBe(true);
    expect(isInsideWithSlack(100 - 28, 50 - 48, bounds, 28, 48)).toBe(true);
    expect(isInsideWithSlack(300 + 28, 100 + 48, bounds, 28, 48)).toBe(true);
    expect(isInsideWithSlack(100 - 29, 75, bounds, 28, 48)).toBe(false);
    expect(isInsideWithSlack(200, 100 + 49, bounds, 28, 48)).toBe(false);
    expect(isInsideWithSlack(200, 0, bounds, 28, 48)).toBe(false);
  });
});
