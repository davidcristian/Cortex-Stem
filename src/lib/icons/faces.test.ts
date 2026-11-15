import { describe, expect, it } from 'vitest';

import type { PowerStatus } from '$lib/ipc/generated/PowerStatus';

import { batteryFace, clockText, msUntilNextMinute, timeFontSize } from './faces';

/** Sunday 15 November 2026, 10:42:30.250 local time. */
const SUNDAY_MORNING = new Date(2026, 10, 15, 10, 42, 30, 250);
const EVENING = new Date(2026, 10, 15, 22, 7, 0, 0);

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
    const text = clockText(SUNDAY_MORNING, 'en-US');
    expect(text.time).toBe('10:42');
    expect(text.dayPeriod).toBe('AM');
    expect(text.periodFirst).toBe(false);
    expect(text.weekday).toBe('Sun');
    expect(text.day).toBe('15');
    expect(plain(text.full)).toBe('Sunday, November 15, 2026 at 10:42 AM');
    expect(clockText(EVENING, 'en-US')).toMatchObject({ time: '10:07', dayPeriod: 'PM' });
  });

  it('keeps a 24-hour time whole, with no day period', () => {
    const text = clockText(EVENING, 'de-DE');
    expect(text.time).toBe('22:07');
    expect(text.dayPeriod).toBe('');
    expect(text.periodFirst).toBe(false);
    // `So` or `So.`, by the ICU version.
    expect(text.weekday).toMatch(/^So\.?$/);
    expect(text.day).toBe('15');
    expect(text.full).toContain('Sonntag');
    expect(text.full).toContain('22:07');
    expect(clockText(SUNDAY_MORNING, 'en-GB').time).toBe('10:42');
  });

  it('notes a day period written before the time', () => {
    const text = clockText(EVENING, 'ko-KR');
    expect(text.dayPeriod).toBe('오후');
    expect(text.periodFirst).toBe(true);
    expect(text.time).toBe('10:07');
  });

  it('follows the default locale when none is given', () => {
    expect(clockText(SUNDAY_MORNING)).toEqual(clockText(SUNDAY_MORNING, undefined));
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
      percentText: '76%',
      glyph: null,
      saver: false,
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
      percentText: '–',
      glyph: null,
      saver: false,
    });
    expect(batteryFace(null)).toEqual({
      level: null,
      percentText: '–',
      glyph: null,
      saver: false,
    });
  });
});
