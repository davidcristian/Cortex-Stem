/**
 * The words and numbers on the clock and battery faces (docs/ipc/widget-items.md
 * "Clock and battery items"). Pure: `ShellIcon.svelte` draws them and
 * `DockItem.svelte` runs the clock's minute timer.
 */

import type { PowerStatus } from '$lib/ipc/generated/PowerStatus';

/** What the clock face shows, in the page's locale. */
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
 * The clock's text for `now`. `locale` is for tests; the page passes
 * nothing, which is the page's own locale.
 */
export function clockText(now: Date, locale?: string): ClockText {
  const parts = new Intl.DateTimeFormat(locale, {
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
    weekday: new Intl.DateTimeFormat(locale, { weekday: 'short' }).format(now),
    day: new Intl.DateTimeFormat(locale, { day: 'numeric' }).format(now),
    full: new Intl.DateTimeFormat(locale, { dateStyle: 'full', timeStyle: 'short' }).format(now),
  };
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
