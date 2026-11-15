/**
 * Idle guard for the dock page.
 *
 * The dock runs permanently and must idle at ~0 % CPU (docs/development/frontend-rules.md,
 * docs/architecture/performance.md): once it has settled after a start, a hover, a
 * launch bounce, a toast, a drag or an auto-hide slide, the page must do
 * nothing at all until the next input or backend event. Each scenario opens
 * the dock in its own browser context (its own renderer process, so the
 * thread-wide `TaskDuration` is the page's own), drives it into a state,
 * waits for an explicit settled condition and then watches it for
 * `WINDOW_MS`:
 *
 * - CDP `Performance.getMetrics` deltas: `LayoutCount` and
 *   `RecalcStyleCount` must not move, `ScriptDuration` and `TaskDuration`
 *   stay under small budgets (the probe's own calls fit in them, a loop
 *   does not);
 * - an init script wraps `requestAnimationFrame`, `setTimeout`,
 *   `setInterval` and `requestIdleCallback` before any page code runs: no
 *   frame callback may run, no timer under a minute may be set or fire, no
 *   interval under a minute may be live;
 * - `document.getAnimations()`: nothing may be running (an infinite CSS
 *   animation is busy work, even when the compositor plays it).
 *
 * Settled means: fonts and images loaded, and for `QUIET_FOR_MS` straight no
 * running animation, no pending frame callback and no pending timer or live
 * interval under a minute. A page that never gets there is measured anyway
 * after `SETTLE_TIMEOUT_MS`, and the report says what was still pending.
 *
 * Scenarios run concurrently (one context each), so the whole file takes a
 * few windows' time, not one per scenario. Two controls show that the
 * probe sees busy work: a synthetic frame loop with an interval, and the
 * attention wiggle, which runs until the window is activated by
 * design.
 *
 * Vite's dev client (`/@vite/client`, a 30 s WebSocket ping interval) is
 * served only by the dev server these tests run against, not with the
 * bundle the app loads (`dist/`): its timers are counted apart and never
 * fail a scenario.
 */

import { expect, test, type Browser, type CDPSession, type Page } from '@playwright/test';

import type { Settings } from '../src/lib/ipc/generated/Settings';
import type { ThemePreset } from '../src/lib/ipc/generated/ThemePreset';

/** How long a settled page is watched. */
const WINDOW_MS = 3_000;
/** A timer that repeats less often than this is not busy work. */
const MINUTE_MS = 60_000;
/** How long the settled condition must hold before the window opens. */
const QUIET_FOR_MS = 250;
/** The most a scenario may take to settle; past it the page is measured as it is. */
const SETTLE_TIMEOUT_MS = 10_000;
/**
 * JavaScript time allowed in a 3 s window. A settled page measures 0 (the
 * probe's evaluations come through DevTools and are not counted); the
 * control's frame loop measures about 6 ms per second.
 */
const SCRIPT_BUDGET_MS = 3;
/**
 * Main-thread task time allowed in a 3 s window. A settled page measures
 * 1–2 ms (the probe's evaluations and the CDP calls themselves); the
 * control's frame loop about 20 ms per second. The margin is for loaded
 * hosts.
 */
const TASK_BUDGET_MS = 25;

const APP_SLOT = '.slot[data-kind="app"]';
const PINNED = '.slot[data-pinned="true"]';
const PANEL = '.dock-panel';
/** A window on a side edge, as the edge tests of screenshots.spec.ts size it. */
const SIDE_VIEWPORT = { width: 360, height: 800 };
/** Above a bottom dock, in the transparent part of the 1280 × 240 window. */
const AWAY = { x: 640, y: 10 };
/** The demo's Explorer window (src/lib/ipc/mock.ts). */
const EXPLORER_WINDOW = 1;
const EXPLORER = `${APP_SLOT}[data-key="app:exe:c:/windows/explorer.exe"]`;

/** What the pages expose in mock mode (`src/lib/ipc/demo.ts`; structural, for `page.evaluate`). */
interface DemoWindow {
  __dockMock?: {
    calls: Array<{ type: string } & Record<string, unknown>>;
    failNextCommand: (message: string) => void;
    getSettings: () => Promise<Settings>;
    updateSettings: (settings: Settings) => Promise<Settings>;
    /** `HSHELL_FLASH` as the platform reports it (`WindowAttention`). */
    pushAttention: (id: number, flashing: boolean) => void;
    pushForeground: (id: number | null) => void;
  };
}

/** One window of the probe's log, summarised (`IdleProbe.since`). */
interface ProbeDelta {
  /** Frame callbacks of page code that ran, counted by the code that asked for them. */
  frames: Record<string, number>;
  /** Timers under a minute that page code set: "timeout 160 ms from <source>". */
  timers: string[];
  /** Timer callbacks of page code that ran. */
  timerCallbacks: string[];
  /** Idle callbacks of page code that ran. */
  idleCallbacks: number;
}

/** What `installIdleProbe` adds to the page. */
interface IdleProbe {
  /** A mark in the log, for `since`. */
  mark(): number;
  since(mark: number): ProbeDelta;
  /** What keeps the page from being settled right now; empty once it is. */
  pending(): string[];
  /**
   * `[]` once `pending()` has been empty for `quietMs` straight, else the
   * last non-empty `pending()` after `timeoutMs`. Polls with the native
   * timers, which the probe does not count.
   */
  settled(quietMs: number, timeoutMs: number): Promise<string[]>;
  /** Running animations, described. */
  running(): string[];
  /** Live intervals of page code under a minute, described. */
  intervals(): string[];
  /** Timers Vite's dev client set so far (not counted against the page). */
  devTimers(): number;
}

interface ProbeWindow {
  __idleProbe: IdleProbe;
}

/**
 * Wrap the page's frame, timer and idle-callback APIs before anything else
 * runs (Svelte's scheduler and the app capture them at module load).
 */
async function installIdleProbe(page: Page): Promise<void> {
  await page.addInitScript((minuteMs: number) => {
    type ProbeEvent =
      | { type: 'frame'; source: string }
      | { type: 'timer'; kind: 'timeout' | 'interval'; delay: number; source: string }
      | { type: 'fired'; kind: 'timeout' | 'interval'; delay: number; source: string }
      | { type: 'idle' };
    interface Timer {
      kind: 'timeout' | 'interval';
      delay: number;
      source: string;
      dev: boolean;
    }
    type Callback = (...args: unknown[]) => void;

    const nativeRaf = window.requestAnimationFrame.bind(window);
    const nativeCancelRaf = window.cancelAnimationFrame.bind(window);
    const nativeSetTimeout = window.setTimeout.bind(window);
    const nativeSetInterval = window.setInterval.bind(window);
    const nativeClearTimeout = window.clearTimeout.bind(window);
    const nativeClearInterval = window.clearInterval.bind(window);
    const nativeIdle = window.requestIdleCallback?.bind(window);

    const log: ProbeEvent[] = [];
    const pendingFrames = new Map<number, string>();
    const pendingTimeouts = new Map<number, Timer>();
    const liveIntervals = new Map<number, Timer>();
    let devTimers = 0;

    /** The code that called the wrapper (two frames, origin and query strings dropped). */
    function caller(): { source: string; dev: boolean } {
      const stack = new Error().stack ?? '';
      // "Error", this function, the wrapper, then the caller.
      const source = stack
        .split('\n')
        .slice(3, 5)
        .map((line) =>
          line
            .trim()
            .replace(/^at /, '')
            .replaceAll(window.location.origin, '')
            .replace(/\?[^:)]*/g, ''),
        )
        .join(' < ');
      return { source: source || 'unknown', dev: stack.includes('/@vite/client') };
    }

    const describe = (t: Timer): string => `${t.kind} ${t.delay} ms from ${t.source}`;

    window.requestAnimationFrame = (callback: FrameRequestCallback): number => {
      const { source } = caller();
      const id = nativeRaf((time) => {
        pendingFrames.delete(id);
        log.push({ type: 'frame', source });
        callback(time);
      });
      pendingFrames.set(id, source);
      return id;
    };
    window.cancelAnimationFrame = (id: number): void => {
      pendingFrames.delete(id);
      nativeCancelRaf(id);
    };

    function wrapTimer(kind: 'timeout' | 'interval'): typeof window.setTimeout {
      const native = kind === 'timeout' ? nativeSetTimeout : nativeSetInterval;
      return ((handler: TimerHandler, timeout?: number, ...args: unknown[]): number => {
        const { source, dev } = caller();
        const timer: Timer = { kind, delay: Math.max(0, Number(timeout) || 0), source, dev };
        if (dev) {
          devTimers += 1;
          return native(handler, timeout, ...args);
        }
        log.push({ type: 'timer', kind, delay: timer.delay, source });
        if (typeof handler !== 'function') {
          // A string handler: counted, not followed (the page has none).
          return native(handler, timeout, ...args);
        }
        const id = native(() => {
          if (kind === 'timeout') {
            pendingTimeouts.delete(id);
          }
          log.push({ type: 'fired', kind, delay: timer.delay, source });
          (handler as Callback)(...args);
        }, timeout);
        (kind === 'timeout' ? pendingTimeouts : liveIntervals).set(id, timer);
        return id;
      }) as typeof window.setTimeout;
    }
    window.setTimeout = wrapTimer('timeout');
    window.setInterval = wrapTimer('interval') as typeof window.setInterval;
    // Either clears either kind, as in the browser.
    const clear = (id: number | undefined): void => {
      if (id !== undefined) {
        pendingTimeouts.delete(id);
        liveIntervals.delete(id);
      }
    };
    // (The casts: Node's typings, loaded for this file, widen the ids.)
    window.clearTimeout = ((id?: number): void => {
      clear(id);
      nativeClearTimeout(id);
    }) as typeof window.clearTimeout;
    window.clearInterval = ((id?: number): void => {
      clear(id);
      nativeClearInterval(id);
    }) as typeof window.clearInterval;
    if (nativeIdle !== undefined) {
      window.requestIdleCallback = (callback, options): number =>
        nativeIdle((deadline) => {
          log.push({ type: 'idle' });
          callback(deadline);
        }, options);
    }

    function running(): string[] {
      return document
        .getAnimations()
        .filter((a) => a.playState === 'running')
        .map((a) => {
          const what =
            a instanceof CSSAnimation
              ? `CSS animation "${a.animationName}"`
              : a instanceof CSSTransition
                ? `CSS transition of ${a.transitionProperty}`
                : 'script animation';
          const target = a.effect instanceof KeyframeEffect ? a.effect.target : null;
          const classes = target === null ? '' : Array.from(target.classList).join('.');
          const where = target === null ? 'nothing' : `${target.localName}.${classes}`;
          return `${what} on ${where}`;
        });
    }

    const underAMinute = (timers: Iterable<Timer>): string[] =>
      Array.from(timers)
        .filter((t) => t.delay < minuteMs)
        .map(describe);

    function pending(): string[] {
      const out = running();
      for (const source of pendingFrames.values()) {
        out.push(`frame callback from ${source}`);
      }
      out.push(...underAMinute(pendingTimeouts.values()), ...underAMinute(liveIntervals.values()));
      if (document.fonts.status !== 'loaded') {
        out.push('fonts loading');
      }
      if (!Array.from(document.images).every((img) => img.complete)) {
        out.push('images loading');
      }
      return out;
    }

    const probe: IdleProbe = {
      mark: () => log.length,
      since(mark) {
        const delta: ProbeDelta = { frames: {}, timers: [], timerCallbacks: [], idleCallbacks: 0 };
        for (const event of log.slice(mark)) {
          switch (event.type) {
            case 'frame':
              delta.frames[event.source] = (delta.frames[event.source] ?? 0) + 1;
              break;
            case 'timer':
              if (event.delay < minuteMs) {
                delta.timers.push(`${event.kind} ${event.delay} ms from ${event.source}`);
              }
              break;
            case 'fired':
              delta.timerCallbacks.push(`${event.kind} ${event.delay} ms from ${event.source}`);
              break;
            case 'idle':
              delta.idleCallbacks += 1;
              break;
          }
        }
        return delta;
      },
      pending,
      settled(quietMs, timeoutMs) {
        const start = performance.now();
        let quietSince: number | null = null;
        return new Promise((resolve) => {
          const poll = (): void => {
            const now = performance.now();
            const busy = pending();
            if (busy.length > 0) {
              quietSince = null;
            } else {
              quietSince ??= now;
              if (now - quietSince >= quietMs) {
                resolve([]);
                return;
              }
            }
            if (now - start >= timeoutMs) {
              resolve(busy);
              return;
            }
            nativeSetTimeout(poll, 20);
          };
          poll();
        });
      },
      running,
      intervals: () => underAMinute(liveIntervals.values()),
      devTimers: () => devTimers,
    };
    (window as unknown as ProbeWindow).__idleProbe = probe;
  }, MINUTE_MS);
}

/** What one watch of a page saw. */
interface Measured {
  /** How long the page was watched. */
  windowMs: number;
  layouts: number;
  styleRecalcs: number;
  scriptMs: number;
  taskMs: number;
  delta: ProbeDelta;
  running: string[];
  intervals: string[];
  /** What was still pending when the page was given up on settling; `[]` when it settled. */
  unsettled: string[];
  /** From the end of the setup to the settled condition, the quiet hold included. */
  settleMs: number;
  devTimers: number;
}

async function metrics(cdp: CDPSession): Promise<Map<string, number>> {
  const { metrics: list } = await cdp.send('Performance.getMetrics');
  return new Map(list.map((m) => [m.name, m.value]));
}

/** Wait for the settled condition: `[]`, or what was still pending after `timeoutMs`. */
function settle(page: Page, timeoutMs = SETTLE_TIMEOUT_MS): Promise<string[]> {
  return page.evaluate(
    ([quiet, timeout]) => (window as unknown as ProbeWindow).__idleProbe.settled(quiet, timeout),
    [QUIET_FOR_MS, timeoutMs] as const,
  );
}

function probeMark(page: Page): Promise<number> {
  return page.evaluate(() => (window as unknown as ProbeWindow).__idleProbe.mark());
}

function probeSince(page: Page, mark: number): Promise<ProbeDelta> {
  return page.evaluate((m) => (window as unknown as ProbeWindow).__idleProbe.since(m), mark);
}

/** One watch of a page: how long it may take to settle, then how long it is watched. */
interface Watch {
  settleTimeoutMs: number;
  windowMs: number;
}

const FULL_WATCH: Watch = { settleTimeoutMs: SETTLE_TIMEOUT_MS, windowMs: WINDOW_MS };
/** For the controls, which never settle: give up on settling soon, watch briefly. */
const CONTROL_WATCH: Watch = { settleTimeoutMs: 500, windowMs: 1_000 };

/** Wait for the settled condition, then watch the page for `windowMs`. */
async function measure(
  page: Page,
  cdp: CDPSession,
  { settleTimeoutMs, windowMs }: Watch = FULL_WATCH,
): Promise<Measured> {
  const settling = Date.now();
  const unsettled = await settle(page, settleTimeoutMs);
  const settleMs = Date.now() - settling;
  const before = await metrics(cdp);
  const mark = await probeMark(page);
  // The window itself: nothing to wait for, by definition.
  await page.waitForTimeout(windowMs);
  const delta = await probeSince(page, mark);
  const after = await metrics(cdp);
  // After the second metrics read: `getAnimations` may flush style itself.
  const { running, intervals, devTimers } = await page.evaluate(() => {
    const probe = (window as unknown as ProbeWindow).__idleProbe;
    return { running: probe.running(), intervals: probe.intervals(), devTimers: probe.devTimers() };
  });
  const change = (name: string): number => (after.get(name) ?? 0) - (before.get(name) ?? 0);
  return {
    windowMs,
    layouts: change('LayoutCount'),
    styleRecalcs: change('RecalcStyleCount'),
    scriptMs: change('ScriptDuration') * 1000,
    taskMs: change('TaskDuration') * 1000,
    delta,
    running,
    intervals,
    unsettled,
    settleMs,
    devTimers,
  };
}

/** Everything in `m` that is busy work; `[]` for a quiet page. */
function busyWork(m: Measured): string[] {
  const out: string[] = [];
  if (m.unsettled.length > 0) {
    out.push(`never settled: ${m.unsettled.join('; ')}`);
  }
  if (m.layouts > 0) {
    out.push(`${m.layouts} layouts`);
  }
  if (m.styleRecalcs > 0) {
    out.push(`${m.styleRecalcs} style recalculations`);
  }
  const frames = Object.entries(m.delta.frames);
  if (frames.length > 0) {
    out.push(`frame callbacks: ${frames.map(([source, n]) => `${n}× ${source}`).join('; ')}`);
  }
  if (m.delta.timers.length > 0) {
    out.push(`timers set: ${m.delta.timers.join('; ')}`);
  }
  if (m.delta.timerCallbacks.length > 0) {
    out.push(`timers fired: ${m.delta.timerCallbacks.join('; ')}`);
  }
  if (m.intervals.length > 0) {
    out.push(`intervals under a minute: ${m.intervals.join('; ')}`);
  }
  if (m.delta.idleCallbacks > 0) {
    out.push(`${m.delta.idleCallbacks} idle callbacks`);
  }
  if (m.running.length > 0) {
    out.push(`running: ${m.running.join('; ')}`);
  }
  if (m.scriptMs > SCRIPT_BUDGET_MS) {
    out.push(`script ${m.scriptMs.toFixed(1)} ms`);
  }
  if (m.taskMs > TASK_BUDGET_MS) {
    out.push(`tasks ${m.taskMs.toFixed(1)} ms`);
  }
  return out;
}

function summary(name: string, m: Measured): string {
  const frames = Object.values(m.delta.frames).reduce((a, b) => a + b, 0);
  return (
    `[idle] ${name}: settled in ${m.settleMs} ms; layouts ${m.layouts}, style ${m.styleRecalcs}, ` +
    `script ${m.scriptMs.toFixed(2)} ms, tasks ${m.taskMs.toFixed(2)} ms, frames ${frames}, ` +
    `timers ${m.delta.timers.length}, animations ${m.running.length}, ` +
    `dev-client timers ${m.devTimers} (over ${m.windowMs} ms)`
  );
}

interface Scenario {
  name: string;
  /** Query parameters after `mock=1`, e.g. `position=left`. */
  query?: string;
  viewport?: { width: number; height: number };
  /**
   * Drive the dock into the state to watch. `measureNow` watches it there
   * and then (for the attention control).
   */
  setup?: (page: Page, measureNow: (watch: Watch) => Promise<Measured>) => Promise<void>;
  /** How the final watch runs; default `FULL_WATCH`. */
  watch?: Watch;
  /** What the final watch must show; default: nothing at all (`busyWork` is empty). */
  check?: (m: Measured) => void;
}

/** Open the dock in mock mode and wait until every app icon is on screen. */
async function openDock(page: Page, query: string | undefined): Promise<void> {
  await page.goto(`/dock.html?mock=1${query === undefined ? '' : `&${query}`}`);
  // Generous: the scenarios load at once, and the first ones meet a cold dev server.
  await expect(page.locator(`${PANEL}[data-ready="true"]`)).toBeVisible({ timeout: 15_000 });
  // Identities resolve ~300 ms after start; every app item then has an icon.
  await expect(page.locator(`${APP_SLOT}:not([data-icon="loaded"])`)).toHaveCount(0);
  await page.evaluate(() => document.fonts.ready.then(() => undefined));
}

async function runScenario(browser: Browser, scenario: Scenario): Promise<Measured> {
  const context = await browser.newContext(
    scenario.viewport === undefined ? {} : { viewport: scenario.viewport },
  );
  try {
    const page = await context.newPage();
    await installIdleProbe(page);
    const cdp = await context.newCDPSession(page);
    await cdp.send('Performance.enable');
    await openDock(page, scenario.query);
    expect(await page.evaluate(() => document.visibilityState)).toBe('visible');
    await scenario.setup?.(page, (watch) => measure(page, cdp, watch));
    return await measure(page, cdp, scenario.watch);
  } finally {
    await context.close();
  }
}

/**
 * Run every scenario at once, each in its own context, and check each
 * one's window. A scenario that throws fails the test after the others ran.
 */
async function expectQuiet(browser: Browser, scenarios: readonly Scenario[]): Promise<void> {
  const results = await Promise.allSettled(scenarios.map((s) => runScenario(browser, s)));
  const errors: unknown[] = [];
  results.forEach((result, i) => {
    const scenario = scenarios[i]!;
    if (result.status === 'rejected') {
      errors.push(result.reason);
      return;
    }
    console.info(summary(scenario.name, result.value));
    if (scenario.check === undefined) {
      expect.soft(busyWork(result.value), scenario.name).toEqual([]);
    } else {
      scenario.check(result.value);
    }
  });
  if (errors.length > 0) {
    throw errors[0];
  }
}

/** Some groups of `Settings`, each with some of its fields. */
type SettingsPatch = {
  [K in keyof Settings]?: Settings[K] extends object ? Partial<Settings[K]> : Settings[K];
};

/** Change some settings through the mock's backend, as the settings window would. */
function patchSettings(page: Page, patch: SettingsPatch): Promise<void> {
  return page.evaluate(async (p) => {
    const mock = (window as unknown as DemoWindow).__dockMock!;
    const settings = (await mock.getSettings()) as unknown as Record<string, unknown>;
    for (const [key, value] of Object.entries(p)) {
      const current = settings[key];
      settings[key] =
        typeof value === 'object' && value !== null && !Array.isArray(value)
          ? { ...(current as object), ...value }
          : value;
    }
    await mock.updateSettings(settings as unknown as Settings);
  }, patch);
}

function presetScenario(theme: ThemePreset): Scenario {
  return {
    name: `the ${theme} preset`,
    setup: async (page) => {
      await patchSettings(page, { appearance: { theme } });
      await expect(page.locator('.dock-stage')).toHaveAttribute('data-theme', theme);
    },
  };
}

/** The magnification scale a slot renders with. */
function scaleOf(page: Page, index: number): Promise<number> {
  return page
    .locator(APP_SLOT)
    .nth(index)
    .evaluate((el) => Number((el as HTMLElement).style.getPropertyValue('--scale') || '1'));
}

function pinnedKeys(page: Page): Promise<string[]> {
  return page.$$eval(PINNED, (els) => els.map((el) => (el as HTMLElement).dataset.key ?? ''));
}

function callsOfType(page: Page, type: string): Promise<number> {
  return page.evaluate(
    (t) =>
      ((window as unknown as DemoWindow).__dockMock?.calls ?? []).filter((c) => c.type === t)
        .length,
    type,
  );
}

/**
 * Magnification on (the default): the pointer sweeps along every app icon,
 * the last one magnifies and shows its pill, then the pointer leaves.
 */
function hoverScenario(edge: 'bottom' | 'left'): Scenario {
  const vertical = edge === 'left';
  return {
    name: `magnification on, after a hover over the icons ends (${edge})`,
    query: `position=${edge}`,
    ...(vertical ? { viewport: SIDE_VIEWPORT } : {}),
    setup: async (page) => {
      const slots = page.locator(APP_SLOT);
      const first = (await slots.first().boundingBox())!;
      const last = (await slots.last().boundingBox())!;
      const lastIndex = (await slots.count()) - 1;
      const mark = await probeMark(page);
      if (vertical) {
        const x = first.x + first.width / 2;
        await page.mouse.move(x, first.y + 4);
        await page.mouse.move(x, last.y + last.height - 4, { steps: 20 });
      } else {
        const y = first.y + first.height / 2;
        await page.mouse.move(first.x + 4, y);
        await page.mouse.move(last.x + last.width - 4, y, { steps: 20 });
      }
      await expect.poll(() => scaleOf(page, lastIndex)).toBeGreaterThan(1.5);
      await expect(page.locator('.tooltip')).toHaveCount(1);
      // Away into the transparent part of the window, beyond the pill.
      const away = vertical ? { x: SIDE_VIEWPORT.width - 10, y: 10 } : AWAY;
      await page.mouse.move(away.x, away.y);
      await expect.poll(() => scaleOf(page, lastIndex)).toBe(1);
      await expect(page.locator('.tooltip')).toHaveCount(0);
      // The probe saw the dock's own work while it was busy: the
      // magnification's frames and, after the leave, the settle timer.
      expect(await settle(page)).toEqual([]);
      const seen = await probeSince(page, mark);
      expect(Object.keys(seen.frames).join('\n')).toMatch(/scheduleFrame .*Dock\.svelte/);
      expect(seen.timers.join('\n')).toMatch(/timeout 160 ms from .*Dock\.svelte/);
    },
  };
}

function setAutoHide(page: Page, mode: Settings['autoHide']['mode']): Promise<void> {
  return patchSettings(page, { autoHide: { mode, delayMs: 250 } });
}

test.describe('the dock page goes quiet once it has settled', () => {
  test('at rest on every edge and with each theme preset', async ({ browser }) => {
    await expectQuiet(browser, [
      { name: 'bottom, the standard preset (the default)' },
      { name: 'top', query: 'position=top' },
      { name: 'left', query: 'position=left', viewport: SIDE_VIEWPORT },
      { name: 'right', query: 'position=right', viewport: SIDE_VIEWPORT },
      presetScenario('solid'),
      presetScenario('minimal'),
    ]);
  });

  test('after a hover, a launch bounce, a toast and a drag have ended', async ({ browser }) => {
    await expectQuiet(browser, [
      hoverScenario('bottom'),
      hoverScenario('left'),
      {
        name: 'after a launch bounce ends',
        setup: async (page) => {
          // Notepad: pinned, not running; a middle click launches it.
          await page.locator(APP_SLOT).nth(3).hover();
          await page.mouse.down({ button: 'middle' });
          await page.mouse.up({ button: 'middle' });
          await expect.poll(() => callsOfType(page, 'launch')).toBe(1);
          await page.mouse.move(AWAY.x, AWAY.y);
          // The bounce did play (1.2 s of Web Animations on the bouncer).
          await expect
            .poll(() =>
              page
                .locator(APP_SLOT)
                .nth(3)
                .locator('.bouncer')
                .evaluate((el) => el.getAnimations().length),
            )
            .toBeGreaterThan(0);
        },
      },
      {
        name: 'after a toast expires',
        setup: async (page) => {
          // Settled first: no icon request is in flight to take the failure.
          expect(await settle(page)).toEqual([]);
          // Hover first: entering the panel sends `set_dock_hover`, which must
          // not be the command that fails.
          const code = page.locator(APP_SLOT).nth(5);
          await code.hover();
          await page.evaluate(() =>
            (window as unknown as DemoWindow).__dockMock?.failNextCommand('window not found: 4'),
          );
          await code.click();
          const toast = page.getByRole('status');
          await expect(toast).toHaveText('Code failed: window not found: 4');
          await page.mouse.move(AWAY.x, AWAY.y);
          // It goes away by itself after 4 s.
          await expect(toast).toHaveCount(0, { timeout: 6_000 });
        },
      },
      {
        name: 'after a drag-reorder ends (the pointer resting where it dropped)',
        setup: async (page) => {
          const pinned = page.locator(PINNED);
          const before = await pinnedKeys(page);
          const a = (await pinned.nth(0).boundingBox())!;
          const b = (await pinned.nth(2).boundingBox())!;
          await page.mouse.move(a.x + a.width / 2, a.y + a.height / 2);
          await page.mouse.down();
          await page.mouse.move(b.x + b.width / 2 + 6, a.y + a.height / 2 - 12, { steps: 12 });
          await expect(pinned.nth(0)).toHaveAttribute('data-drag', 'lifted');
          await page.mouse.up();
          await expect
            .poll(() => pinnedKeys(page))
            .toEqual([before[1], before[2], before[0], before[3], before[4]]);
          await expect(page.locator('[data-drag]')).toHaveCount(0);
        },
      },
      {
        name: 'control: a frame loop and a 1 s interval are seen',
        setup: async (page) => {
          await page.evaluate(() => {
            const box = document.body.appendChild(document.createElement('div'));
            let width = 0;
            const spin = (): void => {
              // A style write and a layout read every frame.
              width = (width + 1) % 8;
              box.style.width = `${width}px`;
              void box.offsetWidth;
              requestAnimationFrame(spin);
            };
            requestAnimationFrame(spin);
            setInterval(() => undefined, 1_000);
          });
        },
        watch: CONTROL_WATCH,
        check: (m) => {
          const busy = busyWork(m).join('\n');
          expect.soft(busy, 'the probe sees the loop').toMatch(/never settled/);
          expect.soft(m.layouts, 'the loop lays out').toBeGreaterThan(0);
          expect.soft(m.styleRecalcs, 'the loop recalculates style').toBeGreaterThan(0);
          expect.soft(m.scriptMs, 'the loop runs script').toBeGreaterThan(0);
          expect.soft(busy, 'the frame callbacks').toMatch(/frame callbacks: \d+×/);
          expect.soft(busy, 'the interval').toMatch(/intervals under a minute: interval 1000 ms/);
          expect.soft(busy, 'its callback').toMatch(/timers fired: interval 1000 ms/);
        },
      },
      {
        name: 'after an attention wiggle ends (the window activated)',
        setup: async (page, measureNow) => {
          // Busy by design while the app asks for attention: an
          // infinite CSS animation, which the probe must see.
          await page.evaluate((id) => {
            (window as unknown as DemoWindow).__dockMock?.pushAttention(id, true);
          }, EXPLORER_WINDOW);
          await expect(page.locator(EXPLORER)).toHaveAttribute('data-attention', 'true');
          const wiggling = await measureNow(CONTROL_WATCH);
          // (Svelte scopes the keyframes' name: "svelte-<hash>-wiggle".)
          expect(wiggling.unsettled.join('; ')).toMatch(/CSS animation "[\w-]*wiggle"/);
          expect(wiggling.running.join('; ')).toMatch(
            /CSS animation "[\w-]*wiggle" on div\.wiggler/,
          );
          // Switched to without the dock (Alt+Tab): the flag clears.
          await page.evaluate((id) => {
            (window as unknown as DemoWindow).__dockMock?.pushForeground(id);
          }, EXPLORER_WINDOW);
          await expect(page.locator(EXPLORER)).not.toHaveAttribute('data-attention');
        },
      },
    ]);
  });

  test('with auto-hide: shown under a resting pointer, slid out, and back from the strip', async ({
    browser,
  }) => {
    await expectQuiet(browser, [
      {
        name: 'auto-hide on, shown: the pointer resting on an icon',
        setup: async (page) => {
          await page.locator(APP_SLOT).nth(3).hover();
          await setAutoHide(page, 'always');
          await expect(page.locator(PANEL)).toHaveAttribute('data-autohide', 'shown');
          await expect.poll(() => scaleOf(page, 3)).toBeGreaterThan(1.5);
        },
      },
      {
        name: 'auto-hide on, slid out',
        setup: async (page) => {
          await page.mouse.move(AWAY.x, AWAY.y);
          await setAutoHide(page, 'always');
          await expect(page.locator(PANEL)).toHaveAttribute('data-autohide', 'hidden');
          await expect(page.locator(PANEL)).toBeHidden();
        },
      },
      {
        name: 'auto-hide on, brought back from the strip: the pointer resting on the edge',
        setup: async (page) => {
          await page.mouse.move(AWAY.x, AWAY.y);
          await setAutoHide(page, 'always');
          await expect(page.locator(PANEL)).toHaveAttribute('data-autohide', 'hidden');
          // The strip runs under the resting panel, which is centred.
          const viewport = page.viewportSize()!;
          await page.mouse.move(viewport.width / 2, viewport.height - 1);
          await expect(page.locator(PANEL)).toHaveAttribute('data-autohide', 'shown');
          await expect(page.locator(PANEL)).toBeVisible();
        },
      },
    ]);
  });
});
