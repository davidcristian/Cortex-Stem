import { describe, expect, it } from 'vitest';

import {
  isVertical,
  LINE_PX,
  MAX_NOTCHES_PER_EVENT,
  NOTCH_PX,
  wheelPixels,
  WheelNotches,
  type WheelInput,
} from './wheel';

const PIXEL = 0;
const LINE = 1;
const PAGE = 2;

/** A wheel event over `key`: `deltaY` in `deltaMode`, nothing sideways, a 240 px viewport. */
function wheel(key: string | null, deltaY: number, deltaMode = PIXEL, deltaX = 0): WheelInput {
  return { key, deltaX, deltaY, deltaMode, pageHeight: 240 };
}

describe('wheelPixels', () => {
  it('takes pixels as they are, lines × 40 and pages × the viewport height', () => {
    expect(NOTCH_PX).toBe(100);
    expect(LINE_PX).toBe(40);
    expect(wheelPixels(100, PIXEL, 240)).toBe(100);
    expect(wheelPixels(-3.5, PIXEL, 240)).toBe(-3.5);
    expect(wheelPixels(3, LINE, 240)).toBe(120);
    expect(wheelPixels(-1, LINE, 240)).toBe(-40);
    expect(wheelPixels(1, PAGE, 240)).toBe(240);
    expect(wheelPixels(-0.5, PAGE, 900)).toBe(-450);
  });

  it('reads an unknown mode as pixels and anything not finite as no travel', () => {
    expect(wheelPixels(100, 7, 240)).toBe(100);
    expect(wheelPixels(Number.NaN, PIXEL, 240)).toBe(0);
    expect(wheelPixels(Number.POSITIVE_INFINITY, LINE, 240)).toBe(0);
    expect(wheelPixels(1, PAGE, 0)).toBe(0);
    expect(wheelPixels(1, PAGE, Number.NaN)).toBe(0);
    expect(wheelPixels(1, PAGE, Number.POSITIVE_INFINITY)).toBe(0);
  });
});

describe('isVertical', () => {
  it('needs travel up or down that is not smaller than the sideways travel', () => {
    expect(isVertical(0, 100)).toBe(true);
    expect(isVertical(0, -1)).toBe(true);
    expect(isVertical(30, 40)).toBe(true);
    expect(isVertical(-40, 40)).toBe(true);
    expect(isVertical(0, 0)).toBe(false);
    expect(isVertical(100, 0)).toBe(false);
    expect(isVertical(41, -40)).toBe(false);
  });
});

describe('WheelNotches (docs/ipc/scrolling-over-an-item.md)', () => {
  it('turns each 100 px of a mouse wheel into one notch: down is next, up is previous', () => {
    const notches = new WheelNotches();
    expect(notches.add(wheel('app:a', 100))).toEqual(['next']);
    expect(notches.add(wheel('app:a', 100))).toEqual(['next']);
    expect(notches.add(wheel('app:a', -100))).toEqual(['previous']);
    // Two notches coalesced into one event are two notches.
    expect(notches.add(wheel('app:a', 200))).toEqual(['next', 'next']);
    expect(notches.add(wheel('app:a', -300))).toEqual(['previous', 'previous', 'previous']);
  });

  it('keeps the rest, so a touchpad’s small steps add up', () => {
    const notches = new WheelNotches();
    const out: string[] = [];
    // Thirty steps of 12.5 px: 375 px, three notches, 75 px left over.
    for (let i = 0; i < 30; i++) {
      out.push(...notches.add(wheel('app:a', 12.5)));
    }
    expect(out).toEqual(['next', 'next', 'next']);
    // The rest counts towards the next notch either way.
    expect(notches.add(wheel('app:a', 25))).toEqual(['next']);
    expect(notches.add(wheel('app:a', -60))).toEqual([]);
    expect(notches.add(wheel('app:a', -40))).toEqual(['previous']);
    // Back and forth below a notch sends nothing.
    for (let i = 0; i < 10; i++) {
      expect(notches.add(wheel('app:a', 90))).toEqual([]);
      expect(notches.add(wheel('app:a', -90))).toEqual([]);
    }
  });

  it('counts lines and pages in pixels', () => {
    const notches = new WheelNotches();
    // Three lines are 120 px: a notch, 20 px kept.
    expect(notches.add(wheel('app:a', 3, LINE))).toEqual(['next']);
    expect(notches.add(wheel('app:a', 2, LINE))).toEqual(['next']);
    // A page of the 240 px viewport is two notches and 40 px.
    expect(notches.add(wheel('app:a', -1, PAGE))).toEqual(['previous', 'previous']);
    expect(notches.add(wheel('app:a', -60))).toEqual(['previous']);
  });

  it('starts again over another item and over none', () => {
    const notches = new WheelNotches();
    expect(notches.add(wheel('app:a', 60))).toEqual([]);
    // 60 + 60 would be a notch over one item; these are two.
    expect(notches.add(wheel('app:b', 60))).toEqual([]);
    expect(notches.add(wheel('app:b', 40))).toEqual(['next']);
    expect(notches.add(wheel('app:b', 60))).toEqual([]);
    expect(notches.add(wheel(null, 60))).toEqual([]);
    expect(notches.add(wheel('app:b', 60))).toEqual([]);
    expect(notches.add(wheel('app:b', 40))).toEqual(['next']);
    // `reset` forgets the rest too.
    expect(notches.add(wheel('app:b', 90))).toEqual([]);
    notches.reset();
    expect(notches.add(wheel('app:b', 90))).toEqual([]);
    expect(notches.add(wheel('app:b', 10))).toEqual(['next']);
  });

  it('ignores a horizontal wheel and keeps the rest across it', () => {
    const notches = new WheelNotches();
    expect(notches.add(wheel('app:a', 0, PIXEL, 100))).toEqual([]);
    expect(notches.add(wheel('app:a', 0, PIXEL, -500))).toEqual([]);
    expect(notches.add(wheel('app:a', 50))).toEqual([]);
    // Mostly sideways: a swipe's vertical jitter does not count.
    expect(notches.add(wheel('app:a', 40, PIXEL, 120))).toEqual([]);
    expect(notches.add(wheel('app:a', 50))).toEqual(['next']);
    // A diagonal that goes as far down as sideways is vertical.
    expect(notches.add(wheel('app:a', 100, PIXEL, -100))).toEqual(['next']);
  });

  it('yields at most ten notches per event and keeps only the rest below one', () => {
    const notches = new WheelNotches();
    expect(MAX_NOTCHES_PER_EVENT).toBe(10);
    expect(notches.add(wheel('app:a', 1e9 + 30))).toHaveLength(10);
    expect(notches.add(wheel('app:a', 60))).toEqual([]);
    expect(notches.add(wheel('app:a', 10))).toEqual(['next']);
    expect(notches.add(wheel('app:a', -5, PAGE))).toEqual(Array(10).fill('previous'));
    expect(notches.add(wheel('app:a', Number.NaN))).toEqual([]);
    expect(notches.add(wheel('app:a', 100))).toEqual(['next']);
  });
});
