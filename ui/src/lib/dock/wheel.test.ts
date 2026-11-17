import { describe, expect, it } from 'vitest';

import {
  isVertical,
  LINE_PX,
  MAX_NOTCHES_PER_EVENT,
  NOTCH_EVENT_PX,
  NOTCH_PX,
  NOTCH_SLACK_PX,
  wheelPixels,
  WheelNotches,
  wheelSends,
  type WheelInput,
} from './wheel';

const PIXEL = 0;
const LINE = 1;
const PAGE = 2;

/** A wheel event over `key`: `deltaY` in `deltaMode`, nothing sideways, no Ctrl, a 240 px viewport. */
function wheel(
  key: string | null,
  deltaY: number,
  deltaMode = PIXEL,
  deltaX = 0,
  ctrlKey = false,
): WheelInput {
  return { key, deltaX, deltaY, deltaMode, ctrlKey, pageHeight: 240 };
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
    // The rest counts towards the next notch the same way.
    expect(notches.add(wheel('app:a', 25))).toEqual(['next']);
    for (let i = 0; i < 3; i++) {
      expect(notches.add(wheel('app:a', -25))).toEqual([]);
    }
    expect(notches.add(wheel('app:a', -25))).toEqual(['previous']);
    // Back and forth below a notch sends nothing.
    for (let i = 0; i < 10; i++) {
      expect(notches.add(wheel('app:a', 29))).toEqual([]);
      expect(notches.add(wheel('app:a', -29))).toEqual([]);
    }
  });

  it('drops the rest when the travel turns the other way', () => {
    const notches = new WheelNotches();
    for (let i = 0; i < 3; i++) {
      expect(notches.add(wheel('app:a', 20))).toEqual([]);
    }
    // The 60 px down are not taken off the way up: 100 px up are a notch.
    for (let i = 0; i < 4; i++) {
      expect(notches.add(wheel('app:a', -20))).toEqual([]);
    }
    expect(notches.add(wheel('app:a', -20))).toEqual(['previous']);
    // Nor are 80 px up taken off the way down.
    for (let i = 0; i < 4; i++) {
      expect(notches.add(wheel('app:a', -20))).toEqual([]);
    }
    for (let i = 0; i < 4; i++) {
      expect(notches.add(wheel('app:a', 20))).toEqual([]);
    }
    expect(notches.add(wheel('app:a', 20))).toEqual(['next']);
  });

  /**
   * One notch of a wheel in Chromium on Windows: lines to scroll at a time
   * × 100/3 px, in single precision (`web_input_event_builders_win.cc`).
   */
  const notchAt = (lines: number): number => Math.fround(lines * Math.fround(100 / 3));

  it('turns every notch of a wheel into one, whatever Windows’ lines to scroll at a time', () => {
    expect(NOTCH_EVENT_PX).toBe(30);
    expect(notchAt(1)).toBeGreaterThan(NOTCH_EVENT_PX);
    expect(notchAt(3)).toBe(NOTCH_PX);
    for (const lines of [1, 2, 3]) {
      const notches = new WheelNotches();
      const down: string[] = [];
      for (let i = 0; i < 6; i++) {
        down.push(...notches.add(wheel('app:a', notchAt(lines))));
      }
      expect(down, `${lines} lines, down`).toEqual(Array(6).fill('next'));
      // Straight back: each notch up is one too, from the first.
      const up: string[] = [];
      for (let i = 0; i < 6; i++) {
        up.push(...notches.add(wheel('app:a', -notchAt(lines))));
      }
      expect(up, `${lines} lines, up`).toEqual(Array(6).fill('previous'));
    }
    // More lines: at least one per notch (the service lets one activation
    // through per 150 ms, so an event's extra ones move nothing).
    const notches = new WheelNotches();
    for (let i = 0; i < 6; i++) {
      expect(notches.add(wheel('app:a', notchAt(4))).length).toBeGreaterThanOrEqual(1);
    }
    expect(notches.add(wheel('app:a', -notchAt(10)))).toEqual(Array(3).fill('previous'));
  });

  it('counts travel just under a whole notch as one, for single-precision deltas', () => {
    expect(NOTCH_SLACK_PX).toBe(0.5);
    const notches = new WheelNotches();
    // Three single-precision thirds of 100 px add up to 99.999996 px.
    const third = Math.fround(100 / 3);
    expect(third + third + third).toBeLessThan(NOTCH_PX);
    const step = third / 2;
    for (let i = 0; i < 5; i++) {
      expect(notches.add(wheel('app:a', step))).toEqual([]);
    }
    expect(notches.add(wheel('app:a', step))).toEqual(['next']);
    // Nothing is left over, not even the 0.000004 px.
    for (let i = 0; i < 3; i++) {
      expect(notches.add(wheel('app:a', 25))).toEqual([]);
    }
    // 99.25 px is short of the slack; 99.5 px is a notch.
    expect(notches.add(wheel('app:a', 24.25))).toEqual([]);
    expect(notches.add(wheel('app:a', 0.25))).toEqual(['next']);
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
    /** Three 25 px steps over `key`, which complete no notch. */
    const threeQuarters = (key: string | null): void => {
      for (let i = 0; i < 3; i++) {
        expect(notches.add(wheel(key, 25)), String(key)).toEqual([]);
      }
    };
    threeQuarters('app:a');
    // 75 + 25 would be a notch over one item; these are two.
    threeQuarters('app:b');
    expect(notches.add(wheel('app:b', 25))).toEqual(['next']);
    threeQuarters('app:b');
    threeQuarters(null);
    threeQuarters('app:b');
    expect(notches.add(wheel('app:b', 25))).toEqual(['next']);
    // `reset` forgets the rest too.
    threeQuarters('app:b');
    notches.reset();
    threeQuarters('app:b');
    expect(notches.add(wheel('app:b', 25))).toEqual(['next']);
  });

  it('ignores a horizontal wheel and keeps the rest across it', () => {
    const notches = new WheelNotches();
    expect(notches.add(wheel('app:a', 0, PIXEL, 100))).toEqual([]);
    expect(notches.add(wheel('app:a', 0, PIXEL, -500))).toEqual([]);
    expect(notches.add(wheel('app:a', 25))).toEqual([]);
    expect(notches.add(wheel('app:a', 25))).toEqual([]);
    // Mostly sideways: a swipe's vertical jitter does not count, not even
    // as a turn the other way.
    expect(notches.add(wheel('app:a', 40, PIXEL, 120))).toEqual([]);
    expect(notches.add(wheel('app:a', -40, PIXEL, 120))).toEqual([]);
    expect(notches.add(wheel('app:a', 25))).toEqual([]);
    expect(notches.add(wheel('app:a', 25))).toEqual(['next']);
    // A diagonal that goes as far down as sideways is vertical.
    expect(notches.add(wheel('app:a', 100, PIXEL, -100))).toEqual(['next']);
  });

  it('ignores a pinch and Ctrl with the wheel (ctrlKey) and keeps the rest across them', () => {
    const notches = new WheelNotches();
    const ctrl = (key: string | null, deltaY: number, deltaMode = PIXEL): WheelInput =>
      wheel(key, deltaY, deltaMode, 0, true);
    // A whole notch and more with Ctrl held, in every mode: nothing.
    expect(notches.add(ctrl('app:a', 100))).toEqual([]);
    expect(notches.add(ctrl('app:a', -300))).toEqual([]);
    expect(notches.add(ctrl('app:a', 3, LINE))).toEqual([]);
    expect(notches.add(ctrl('app:a', 1, PAGE))).toEqual([]);
    // A pinch's small steps, both ways: nothing either, and no rest.
    for (const step of [2.5, -4, 7, -1.5]) {
      expect(notches.add(ctrl('app:a', step))).toEqual([]);
    }
    expect(notches.add(wheel('app:a', 25))).toEqual([]);
    expect(notches.add(wheel('app:a', 25))).toEqual([]);
    // The scroll's rest survives a pinch in its middle, even one the other way.
    expect(notches.add(ctrl('app:a', -40))).toEqual([]);
    expect(notches.add(ctrl('app:a', 60))).toEqual([]);
    expect(notches.add(wheel('app:a', 25))).toEqual([]);
    expect(notches.add(wheel('app:a', 25))).toEqual(['next']);
    // Without Ctrl the same event is a notch.
    expect(notches.add(wheel('app:a', -100))).toEqual(['previous']);
    expect(notches.add(ctrl('app:a', -100))).toEqual([]);
  });

  it('yields at most ten notches per event and keeps only the rest below one', () => {
    const notches = new WheelNotches();
    expect(MAX_NOTCHES_PER_EVENT).toBe(10);
    expect(notches.add(wheel('app:a', 1e9 + 20))).toHaveLength(10);
    for (let i = 0; i < 3; i++) {
      expect(notches.add(wheel('app:a', 25))).toEqual([]);
    }
    expect(notches.add(wheel('app:a', 5))).toEqual(['next']);
    expect(notches.add(wheel('app:a', -5, PAGE))).toEqual(Array(10).fill('previous'));
    expect(notches.add(wheel('app:a', Number.NaN))).toEqual([]);
    expect(notches.add(wheel('app:a', 100))).toEqual(['next']);
  });
});

describe('wheelSends', () => {
  it('sends over the volume item whatever the setting, over other items only while it is not none', () => {
    expect(wheelSends('none', 'volume')).toBe(true);
    expect(wheelSends('cycleWindows', 'volume')).toBe(true);
    for (const kind of ['app', 'launcher', 'battery', 'clock', 'recycleBin'] as const) {
      expect(wheelSends('none', kind)).toBe(false);
      expect(wheelSends('cycleWindows', kind)).toBe(true);
    }
    expect(wheelSends('none', null)).toBe(false);
    expect(wheelSends('cycleWindows', null)).toBe(false);
  });
});
