/**
 * The words and numbers on the clock and battery faces (docs/ipc/widget-items.md
 * "Clock and battery items"). Pure: `ShellIcon.svelte` draws them and
 * `DockItem.svelte` runs the clock's minute timer.
 */

import type { ClockFormat } from '$lib/ipc/generated/ClockFormat';
import type { PowerStatus } from '$lib/ipc/generated/PowerStatus';

/** What the clock face shows, in the user's regional format. */
export interface ClockText {
  /** The time without its day period: `10:42` (`hour: 'numeric', minute: '2-digit'`). */
  time: string;
  /** The day period of a 12-hour locale (`AM`, `PM`, `오후`), or `''`. */
  dayPeriod: string;
  /** Whether the locale writes the day period before the time (`오후 10:42`). */
  periodFirst: boolean;
  /** The short weekday: `Sun`. */
  weekday: string;
  /** The day of the month: `15`. */
  day: string;
  /** The full date and time, the title pill and the accessible name. */
  full: string;
}

/**
 * The clock's text for `now` in `format` (`DockState.clockFormat`,
 * docs/ipc/widget-items.md "The clock's regional format"): its `locale`, or
 * the page's own while that is `null`, and its hour cycle, or the locale's own
 * while `hour12` is `null`. `null` (the default) is the page's own locale and
 * hour cycle.
 */
export function clockText(now: Date, format: ClockFormat | null = null): ClockText {
  const parts = clockFormatter(format, {
    hour: 'numeric',
    minute: '2-digit',
  }).formatToParts(now);
  const periodAt = parts.findIndex((p) => p.type === 'dayPeriod');
  const hourAt = parts.findIndex((p) => p.type === 'hour');
  return {
    time: parts
      .filter((p) => p.type !== 'dayPeriod')
      .map((p) => p.value)
      .join('')
      .trim(),
    dayPeriod: periodAt === -1 ? '' : parts[periodAt]!.value,
    periodFirst: periodAt !== -1 && periodAt < hourAt,
    weekday: clockFormatter(format, { weekday: 'short' }).format(now),
    day: clockFormatter(format, { day: 'numeric' }).format(now),
    full: clockFormatter(format, { dateStyle: 'full', timeStyle: 'short' }).format(now),
  };
}

/**
 * An `Intl.DateTimeFormat` with `options` in `format`'s locale and hour
 * cycle. Windows' `h` is an hour from 1 to 12 and `H` one from 0 to 23,
 * which are Intl's hour cycles `h12` and `h23`; `hour12: true` would be
 * 0 to 11 in some locales (ja-JP). A locale name Intl rejects (a
 * `RangeError`) falls back to the page's own, keeping the hour cycle.
 */
function clockFormatter(
  format: ClockFormat | null,
  options: Intl.DateTimeFormatOptions,
): Intl.DateTimeFormat {
  const withCycle: Intl.DateTimeFormatOptions =
    format === null || format.hour12 === null
      ? options
      : { ...options, hourCycle: format.hour12 ? 'h12' : 'h23' };
  if (format !== null && format.locale !== null) {
    try {
      return new Intl.DateTimeFormat(format.locale, withCycle);
    } catch (error) {
      if (!(error instanceof RangeError)) {
        throw error;
      }
    }
  }
  return new Intl.DateTimeFormat(undefined, withCycle);
}

/**
 * How long from `now` until the next minute starts, in ms: what the clock's
 * one `setTimeout` waits. Never 0, so a timer that fires a little early
 * waits the last millisecond instead of spinning.
 */
export function msUntilNextMinute(now: Date): number {
  const into = now.getSeconds() * 1000 + now.getMilliseconds();
  return Math.max(1, 60_000 - into);
}

/** The width the time's text may take on the 64-unit face. */
const TIME_WIDTH = 54;
/** The day period is drawn at this fraction of the time's size. */
export const DAY_PERIOD_SCALE = 0.5;

/**
 * The font size (face units) that fits `time` and `dayPeriod` side by side
 * in [`TIME_WIDTH`], from typical glyph widths (digits are tabular): 22 for
 * `10:42`, smaller for `10:42 AM`, never below 11.
 */
export function timeFontSize(text: Pick<ClockText, 'time' | 'dayPeriod'>): number {
  const em = (s: string): number =>
    [...s].reduce((sum, c) => sum + (c === ':' || c === '.' ? 0.3 : /\d/.test(c) ? 0.58 : 0.66), 0);
  const width =
    em(text.time) + (text.dayPeriod === '' ? 0 : 0.15 + em(text.dayPeriod) * DAY_PERIOD_SCALE);
  return Math.max(11, Math.min(22, Math.floor(TIME_WIDTH / Math.max(width, 0.1))));
}

/** What the battery face shows. */
export interface BatteryFace {
  /** The fill, 0 to 1; `null` when the charge is not known (no fill). */
  level: number | null;
  /** The charge as text in the page's locale (`76%`), or `–` when not known. */
  percentText: string;
  /** A bolt while charging, a plug on AC power without charging, else none. */
  glyph: 'bolt' | 'plug' | null;
  /** Battery saver is on: the fill takes the saver colour. */
  saver: boolean;
}

/**
 * The battery face for `power` (`DockState.power`; `null` before the page
 * has a state). `locale` is for tests.
 */
export function batteryFace(power: PowerStatus | null, locale?: string): BatteryFace {
  const percent = power?.percent ?? null;
  const level = percent === null ? null : Math.min(100, Math.max(0, percent)) / 100;
  return {
    level,
    percentText:
      level === null
        ? '–'
        : new Intl.NumberFormat(locale, { style: 'percent', maximumFractionDigits: 0 }).format(
            level,
          ),
    glyph: power?.charging ? 'bolt' : power?.onAc ? 'plug' : null,
    saver: power?.saver ?? false,
  };
}
