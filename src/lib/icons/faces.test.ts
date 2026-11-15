import { describe, expect, it } from 'vitest';

import type { ClockFormat } from '$lib/ipc/generated/ClockFormat';
import type { PowerStatus } from '$lib/ipc/generated/PowerStatus';

import {
  BATTERY_FILL_WIDTH,
  BATTERY_MIN_FILL,
  batteryFace,
  clockText,
  msUntilNextMinute,
  timeFontSize,
} from './faces';

/** Sunday 15 November 2026, 10:42:30.250 local time. */
const SUNDAY_MORNING = new Date(2026, 10, 15, 10, 42, 30, 250);
const EVENING = new Date(2026, 10, 15, 22, 7, 0, 0);
const AFTERNOON = new Date(2026, 10, 15, 14, 42, 0, 0);
const AFTER_MIDNIGHT = new Date(2026, 10, 16, 0, 7, 0, 0);

/** `DockState.clockFormat`: a locale, and by default the locale's own hour cycle. */
function fmt(locale: string | null, hour12: boolean | null = null): ClockFormat {
  return { locale, hour12 };
}

function power(patch: Partial<PowerStatus>): PowerStatus {
  return { hasBattery: true, percent: 76, charging: false, onAc: false, saver: false, ...patch };
}

/**
 * Intl may space the day period (and a percent sign) with a no-break space;
 * NFKC turns both kinds into plain spaces for the comparison.
 */
function plain(s: string): string {
  return s.normalize('NFKC');
}

describe('clockText', () => {
  it('splits a 12-hour time into the time and its day period', () => {
    const text = clockText(SUNDAY_MORNING, fmt('en-US'));
    expect(text.time).toBe('10:42');
    expect(text.dayPeriod).toBe('AM');
    expect(text.periodFirst).toBe(false);
    expect(text.weekday).toBe('Sun');
    expect(text.day).toBe('15');
    expect(plain(text.full)).toBe('Sunday, November 15, 2026 at 10:42 AM');
    expect(clockText(EVENING, fmt('en-US'))).toMatchObject({ time: '10:07', dayPeriod: 'PM' });
  });

  it('keeps a 24-hour time whole, with no day period', () => {
    const text = clockText(EVENING, fmt('de-DE'));
    expect(text.time).toBe('22:07');
    expect(text.dayPeriod).toBe('');
    expect(text.periodFirst).toBe(false);
    // `So` or `So.`, by the ICU version.
    expect(text.weekday).toMatch(/^So\.?$/);
    expect(text.day).toBe('15');
    expect(text.full).toContain('Sonntag');
    expect(text.full).toContain('22:07');
    expect(clockText(SUNDAY_MORNING, fmt('en-GB')).time).toBe('10:42');
  });

  it('notes a day period written before the time', () => {
    const text = clockText(EVENING, fmt('ko-KR'));
    expect(text.dayPeriod).toBe('오후');
    expect(text.periodFirst).toBe(true);
    expect(text.time).toBe('10:07');
  });

  it('follows the page when no format is known', () => {
    expect(clockText(SUNDAY_MORNING)).toEqual(clockText(SUNDAY_MORNING, null));
    expect(clockText(SUNDAY_MORNING, fmt(null))).toEqual(clockText(SUNDAY_MORNING));
    expect(clockText(AFTERNOON, fmt(null))).toEqual(clockText(AFTERNOON));
  });
});

describe("clockText in the user's regional format (v1.25)", () => {
  it("takes the region's own hour cycle when Windows names none", () => {
    expect(clockText(AFTERNOON, fmt('en-GB'))).toMatchObject({ time: '14:42', dayPeriod: '' });
    expect(clockText(AFTERNOON, fmt('en-US'))).toMatchObject({ time: '2:42', dayPeriod: 'PM' });
  });

  it('writes a 24-hour time in a 12-hour region (a custom short time with H)', () => {
    const text = clockText(AFTERNOON, fmt('en-US', false));
    expect(text).toMatchObject({ time: '14:42', dayPeriod: '', periodFirst: false });
    expect(text.weekday).toBe('Sun');
    expect(plain(text.full)).toBe('Sunday, November 15, 2026 at 14:42');
    // 0 to 23 (Windows' H), never 24.
    expect(clockText(AFTER_MIDNIGHT, fmt('en-US', false)).time).toMatch(/^0?0:07$/);
  });

  it('writes a 12-hour time in a 24-hour region (a custom short time with h)', () => {
    const text = clockText(AFTERNOON, fmt('en-GB', true));
    expect(text.time).toBe('2:42');
    expect(text.dayPeriod.toLowerCase()).toBe('pm');
    expect(plain(text.full)).toContain('15 November 2026');
    expect(plain(text.full)).toMatch(/2:42 pm$/i);
    // 1 to 12 (Windows' h), never 0, in a locale whose own 12-hour clock
    // starts at 0.
    const japanese = clockText(AFTER_MIDNIGHT, fmt('ja-JP', true));
    expect(japanese.time).toBe('12:07');
    expect(japanese.periodFirst).toBe(true);
    expect(japanese.dayPeriod).not.toBe('');
  });

  it("writes the date in the region's language and order", () => {
    const text = clockText(AFTERNOON, fmt('en-GB', false));
    expect(text).toMatchObject({ time: '14:42', weekday: 'Sun', day: '15' });
    expect(plain(text.full)).toContain('15 November 2026');
    expect(plain(text.full)).toContain('14:42');
    const german = clockText(AFTERNOON, fmt('de-DE', false));
    expect(german.full).toContain('Sonntag');
    expect(german.full).toContain('14:42');
  });

  it("falls back to the page's own locale for a name Intl rejects, keeping the hour cycle", () => {
    for (const bad of ['de-DE_phoneb', 'not a locale', '']) {
      expect(clockText(AFTERNOON, fmt(bad, false))).toEqual(clockText(AFTERNOON, fmt(null, false)));
      expect(clockText(AFTERNOON, fmt(bad))).toEqual(clockText(AFTERNOON));
    }
    expect(clockText(AFTERNOON, fmt('not a locale', false)).time).toMatch(/^14[:.]42$/);
  });
});

describe('msUntilNextMinute', () => {
  it('waits until the next minute starts', () => {
    expect(msUntilNextMinute(SUNDAY_MORNING)).toBe(29_750);
    expect(msUntilNextMinute(new Date(2026, 10, 15, 10, 42, 0, 0))).toBe(60_000);
    expect(msUntilNextMinute(new Date(2026, 10, 15, 10, 42, 59, 999))).toBe(1);
  });

  it('lands on the next minute from anywhere in this one', () => {
    for (const ms of [0, 1, 17_345, 59_000, 59_999]) {
      const now = new Date(new Date(2026, 10, 15, 23, 59, 0, 0).getTime() + ms);
      const next = new Date(now.getTime() + msUntilNextMinute(now));
      expect(next.getSeconds()).toBe(0);
      expect(next.getMilliseconds()).toBe(0);
      expect(next.getMinutes()).toBe(0);
      // Over midnight, into the next day.
      expect(next.getDate()).toBe(16);
    }
  });
});

describe('timeFontSize', () => {
  it('is largest for a short 24-hour time and smaller with a day period', () => {
    const plainTime = timeFontSize({ time: '22:07', dayPeriod: '' });
    const twelve = timeFontSize({ time: '10:42', dayPeriod: 'AM' });
    expect(plainTime).toBeGreaterThan(twelve);
    expect(plainTime).toBeLessThanOrEqual(22);
    expect(twelve).toBeGreaterThanOrEqual(11);
    // A one-digit hour has room for a larger size than a two-digit one.
    expect(timeFontSize({ time: '9:05', dayPeriod: 'AM' })).toBeGreaterThanOrEqual(twelve);
  });

  it('never goes below 11 or above 22', () => {
    expect(timeFontSize({ time: '', dayPeriod: '' })).toBe(22);
    expect(timeFontSize({ time: '10:42:00 long', dayPeriod: 'in the afternoon' })).toBe(11);
  });
});

describe('batteryFace', () => {
  it('fills to the percent and writes it in the locale', () => {
    expect(batteryFace(power({}), 'en-US')).toEqual({
      level: 0.76,
      fill: BATTERY_FILL_WIDTH * 0.76,
      percentText: '76%',
      glyph: null,
      saver: false,
      low: false,
    });
    expect(plain(batteryFace(power({ percent: 5 }), 'fr-FR').percentText)).toBe('5 %');
    expect(batteryFace(power({ percent: 0 }), 'en-US').level).toBe(0);
    expect(batteryFace(power({ percent: 100 }), 'en-US').level).toBe(1);
  });

  it('draws a bolt while charging and a plug on AC without charging', () => {
    expect(batteryFace(power({ charging: true, onAc: true })).glyph).toBe('bolt');
    expect(batteryFace(power({ charging: true, onAc: false })).glyph).toBe('bolt');
    expect(batteryFace(power({ onAc: true })).glyph).toBe('plug');
    expect(batteryFace(power({})).glyph).toBe(null);
  });

  it('takes the saver fill while battery saver is on', () => {
    expect(batteryFace(power({ saver: true })).saver).toBe(true);
    expect(batteryFace(power({ saver: false })).saver).toBe(false);
  });

  it('has no fill and a dash without a known charge, or before any state', () => {
    expect(batteryFace(power({ percent: null }))).toEqual({
      level: null,
      fill: null,
      percentText: '–',
      glyph: null,
      saver: false,
      low: false,
    });
    expect(batteryFace(null)).toEqual({
      level: null,
      fill: null,
      percentText: '–',
      glyph: null,
      saver: false,
      low: false,
    });
  });

  it('draws a low charge with a visible fill: never narrower than the minimum, none when empty', () => {
    expect(BATTERY_MIN_FILL).toBeGreaterThanOrEqual(4);
    expect(batteryFace(power({ percent: 5 })).fill).toBe(BATTERY_MIN_FILL);
    expect(batteryFace(power({ percent: 1 })).fill).toBe(BATTERY_MIN_FILL);
    expect(batteryFace(power({ percent: 0 })).fill).toBeNull();
    // Above the minimum the fill is the charge's share again.
    expect(batteryFace(power({ percent: 50 })).fill).toBe(BATTERY_FILL_WIDTH / 2);
    expect(batteryFace(power({ percent: 100 })).fill).toBe(BATTERY_FILL_WIDTH);
    for (let percent = 1; percent <= 100; percent += 1) {
      const fill = batteryFace(power({ percent })).fill!;
      expect(fill).toBeGreaterThanOrEqual(BATTERY_MIN_FILL);
      expect(fill).toBeLessThanOrEqual(BATTERY_FILL_WIDTH);
    }
  });

  it('takes the low-charge colour below 20 % while not charging, unless battery saver is on', () => {
    expect(batteryFace(power({ percent: 19 })).low).toBe(true);
    expect(batteryFace(power({ percent: 5 })).low).toBe(true);
    expect(batteryFace(power({ percent: 0 })).low).toBe(true);
    expect(batteryFace(power({ percent: 20 })).low).toBe(false);
    // Charging: the bolt says it is getting better.
    expect(batteryFace(power({ percent: 5, charging: true, onAc: true })).low).toBe(false);
    // On AC power without charging, the battery is not charging either.
    expect(batteryFace(power({ percent: 5, onAc: true })).low).toBe(true);
    // Saver's colour wins.
    expect(batteryFace(power({ percent: 5, saver: true }))).toMatchObject({
      saver: true,
      low: false,
    });
    expect(batteryFace(power({ percent: null })).low).toBe(false);
    expect(batteryFace(null).low).toBe(false);
  });
});
