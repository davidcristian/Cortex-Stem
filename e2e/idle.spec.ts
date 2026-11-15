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
 *   `setInterval` and `requestIdleCallback` before any page code runs: in
 *   the window no frame, timer or idle callback of page code may run and
 *   no timer may be set, and at its end nothing may repeat faster than
 *   once a minute;
 * - `document.getAnimations()`: nothing may be running (an infinite CSS
 *   animation is busy work, even when the compositor plays it).
 *
 * Settled means: fonts and images loaded, and for `QUIET_FOR_MS` straight no
 * running animation, no pending frame or idle callback, no timer due before
 * the window would end, and nothing that repeats faster than once a minute.
 * Work that recurs once a minute (the clock item's tick) is allowed:
 * the settle waits until its next run is past the window. A page that never
 * gets there is measured anyway after `SETTLE_TIMEOUT_MS`, and the report
 * says what was still pending.
 *
 * How often a timer repeats: an interval, at its delay. A timeout due on a
 * minute of the wall clock (how a minute clock aims its tick), at most once
 * a minute. Any other timeout, from the time the last timeout set at the
 * same call site fell due to the time this one does (a chain that re-arms
 * for a fixed deadline gets a delay just under its period; measuring due
 * time to due time leaves out how late each run was); while nothing from
 * that call site has fired yet, at its delay (conservative: a one-shot
 * under a minute counts as repeating until it has run).
 *
 * Scenarios run concurrently (one context each), so the whole file takes a
 * few windows' time, not one per scenario. Controls show that the probe
 * sees busy work: a synthetic frame loop with an interval, a 10 s polling
 * chain that never fires in the window, an idle-callback loop, and the
 * attention wiggle. The wiggle runs until the window is activated,
 * by design, so it cannot go quiet; it is held to the bound it keeps
 * instead (compositor only: no layout, script or callbacks).
 *
 * What is measured is the dev build: `pnpm e2e:screenshots` runs these
 * tests against `pnpm dev` (unbundled modules, Svelte in dev mode), not
 * the bundle the app loads (`dist/`). Vite's dev client there
 * (`/@vite/client`, a 30 s WebSocket ping interval) is not part of the
 * bundle: its timers are counted apart and never fail a scenario. To
 * measure the bundle, serve it and point the tests at it (outside CI the
 * config reuses a server that is already up): `pnpm build`, then
 * `pnpm preview --port 1436 --strictPort`, then
 * `STEM_DEV_PORT=1436 pnpm exec playwright test e2e/idle.spec.ts`.
 */

import { expect, test, type Browser, type CDPSession, type Page } from '@playwright/test';

import type { Settings } from '../src/lib/ipc/generated/Settings';
import type { ThemeInfo } from '../src/lib/ipc/generated/ThemeInfo';
import type { ThemePreset } from '../src/lib/ipc/generated/ThemePreset';

/** How long a settled page is watched. */
const WINDOW_MS = 3_000;
/**
 * Added to the window when the settle looks ahead for timers falling due:
 * the probe's calls between the settle and the window's start, and its end.
 */
const WINDOW_MARGIN_MS = 1_000;
/** A timer that repeats less often than this is not busy work. */
const MINUTE_MS = 60_000;
/**
 * A timeout due this close to a minute of the wall clock is a minute
 * clock's tick, which computes its delay from `new Date()` just before it
 * sets the timer.
 */
const MINUTE_ALIGN_MS = 50;
/**
 * A chain measured from due time to due time may come out this much under
 * its period: its code may aim at the wall clock, which can drift from the
 * page's monotonic clock by a few ms over a minute.
 */
const PERIOD_SLACK_MS = 100;
/** How long the settled condition must hold before the window opens. */
const QUIET_FOR_MS = 250;
/** The most a scenario may take to settle; past it the page is measured as it is. */
const SETTLE_TIMEOUT_MS = 10_000;
/**
 * JavaScript time allowed in a 3 s window. A settled page measures 0 (the
 * probe's evaluations come through DevTools and are not counted); the
 * frame loop control, 1 ms of script in each frame, about 190 ms, and it
 * must trip this budget.
 */
const SCRIPT_BUDGET_MS = 3;
/**
 * Main-thread task time allowed in a 3 s window. A settled page measured
 * 0.7–2.2 ms on a Windows 11 PC, and up to 3.8 ms while that PC was
 * loaded (the probe's evaluations and the CDP calls themselves); the frame
 * loop control about 230 ms, and it must trip this budget. The margin is
 * for loaded hosts.
 */
const TASK_BUDGET_MS = 25;
/**
 * Style recalculations a running attention wiggle may cost in a window.
 * Measured: 1 over 3 s and 1 over 9 s, so not one per 2 s iteration; a
 * main-thread animation would cost one a frame (about 180).
 */
const WIGGLE_STYLE_RECALCS = 2;

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
/** The clock item, shown with `items.clock`. */
const CLOCK = '.slot[data-kind="clock"]';
/** The battery item, shown with `items.battery` on the demo's laptop. */
const BATTERY = '.slot[data-kind="battery"]';
/** A live timer the probe takes as once a minute because it is due on a minute of the wall clock. */
const MINUTE_TIMER =
  /^timeout \d+ ms from .*, every 60000 ms \(due on a minute of the wall clock\)$/;

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
    /** Windows' theme changed (`dock://theme`). */
    setTheme: (theme: ThemeInfo) => void;
  };
}

/** One window of the probe's log, summarised (`IdleProbe.since`). */
interface ProbeDelta {
  /** Frame callbacks of page code that ran, counted by the code that asked for them. */
  frames: Record<string, number>;
  /** Timers that page code set, whatever their delay: "timeout 160 ms from <source>". */
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
  /**
   * What keeps the page from being settled right now, for a window that
   * would end `horizonMs` from now; empty once it is.
   */
  pending(horizonMs: number): string[];
  /**
   * `[]` once `pending(horizonMs)` has been empty for `quietMs` straight,
   * else the last non-empty `pending` after `timeoutMs`. Polls with the
   * native timers, which the probe does not count.
   */
  settled(quietMs: number, timeoutMs: number, horizonMs: number): Promise<string[]>;
  /** Running animations, described. */
  running(): string[];
  /** Live intervals and pending timeouts of page code that repeat faster than once a minute. */
  repeaters(): string[];
  /** Every live interval and pending timeout of page code, with how often it repeats and why. */
  live(): string[];
  /** Timers Vite's dev client set so far (not counted against the page). */
  devTimers(): number;
}

/** The rule's numbers, handed to the init script. */
interface ProbeRule {
  minuteMs: number;
  alignMs: number;
  slackMs: number;
}

interface ProbeWindow {
  __idleProbe: IdleProbe;
}

/**
 * Wrap the page's frame, timer and idle-callback APIs before anything else
 * runs (Svelte's scheduler and the app capture them at module load).
 */
async function installIdleProbe(page: Page): Promise<void> {
  const rule: ProbeRule = {
    minuteMs: MINUTE_MS,
    alignMs: MINUTE_ALIGN_MS,
    slackMs: PERIOD_SLACK_MS,
  };
  await page.addInitScript(({ minuteMs, alignMs, slackMs }: ProbeRule) => {
    type Kind = 'timeout' | 'interval';
    type ProbeEvent =
      | { type: 'frame'; source: string }
      | { type: 'timer'; text: string }
      | { type: 'fired'; text: string }
      | { type: 'idle' };
    interface Timer {
      kind: Kind;
      delay: number;
      /** The two frames that set it, for reports. */
      source: string;
      /** The frame that called the timer API: a chain's key. */
      site: string;
      /** `performance.now()` when it falls due (its next run, for an interval). */
      due: number;
      /** How often it repeats, as far as the probe can tell. */
      period: number;
      /** How `period` was found. */
      basis: string;
    }
    type Callback = (...args: unknown[]) => void;

    const nativeRaf = window.requestAnimationFrame.bind(window);
    const nativeCancelRaf = window.cancelAnimationFrame.bind(window);
    const nativeSetTimeout = window.setTimeout.bind(window);
    const nativeSetInterval = window.setInterval.bind(window);
    const nativeClearTimeout = window.clearTimeout.bind(window);
    const nativeClearInterval = window.clearInterval.bind(window);
    const nativeIdle = window.requestIdleCallback?.bind(window);
    const nativeCancelIdle = window.cancelIdleCallback?.bind(window);

    const log: ProbeEvent[] = [];
    const pendingFrames = new Map<number, string>();
    const pendingIdle = new Map<number, string>();
    const pendingTimeouts = new Map<number, Timer>();
    const liveIntervals = new Map<number, Timer>();
    /** When the last timeout set at each call site that has fired fell due. */
    const lastFiredDue = new Map<string, number>();
    let devTimers = 0;

    /**
     * The code that called the wrapper: two frames for reports, the first
     * for a chain's key (origin and query strings dropped).
     */
    function caller(): { source: string; site: string; dev: boolean } {
      const stack = new Error().stack ?? '';
      // "Error", this function, the wrapper, then the caller.
      const frames = stack
        .split('\n')
        .slice(3, 5)
        .map((line) =>
          line
            .trim()
            .replace(/^at /, '')
            .replaceAll(window.location.origin, '')
            .replace(/\?[^:)]*/g, ''),
        );
      const source = frames.join(' < ');
      return {
        source: source || 'unknown',
        site: frames[0] ?? 'unknown',
        dev: stack.includes('/@vite/client'),
      };
    }

    /** A new timer, with how often it repeats (the rule in the file's header). */
    function timerOf(kind: Kind, delay: number, source: string, site: string): Timer {
      const due = performance.now() + delay;
      const timer = { kind, delay, source, site, due };
      if (kind === 'interval') {
        return { ...timer, period: delay, basis: 'an interval' };
      }
      const offMinute = (Date.now() + delay) % minuteMs;
      if (Math.min(offMinute, minuteMs - offMinute) <= alignMs) {
        return { ...timer, period: minuteMs, basis: 'due on a minute of the wall clock' };
      }
      const last = lastFiredDue.get(site);
      if (last !== undefined) {
        return { ...timer, period: due - last, basis: 'from the last one set there falling due' };
      }
      return { ...timer, period: delay, basis: 'its delay: nothing set there has run yet' };
    }

    const describe = (t: Timer): string => `${t.kind} ${Math.round(t.delay)} ms from ${t.source}`;
    const fast = (t: Timer): boolean => t.period < minuteMs - slackMs;
    const repeating = (t: Timer): string =>
      `${describe(t)}, every ${Math.round(t.period)} ms (${t.basis})`;
    const timers = (): Timer[] => [...pendingTimeouts.values(), ...liveIntervals.values()];

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

    function wrapTimer(kind: Kind): typeof window.setTimeout {
      const native = kind === 'timeout' ? nativeSetTimeout : nativeSetInterval;
      return ((handler: TimerHandler, timeout?: number, ...args: unknown[]): number => {
        const { source, site, dev } = caller();
        if (dev) {
          devTimers += 1;
          return native(handler, timeout, ...args);
        }
        const delay = Math.max(0, Number(timeout) || 0);
        const timer = timerOf(kind, delay, source, site);
        log.push({ type: 'timer', text: describe(timer) });
        if (typeof handler !== 'function') {
          // A string handler: counted, not followed (the page has none).
          return native(handler, timeout, ...args);
        }
        const id = native(() => {
          if (kind === 'timeout') {
            pendingTimeouts.delete(id);
            lastFiredDue.set(site, timer.due);
          } else {
            timer.due = performance.now() + delay;
          }
          log.push({ type: 'fired', text: describe(timer) });
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
    if (nativeIdle !== undefined && nativeCancelIdle !== undefined) {
      window.requestIdleCallback = (callback, options): number => {
        const { source } = caller();
        const id = nativeIdle((deadline) => {
          pendingIdle.delete(id);
          log.push({ type: 'idle' });
          callback(deadline);
        }, options);
        pendingIdle.set(id, source);
        return id;
      };
      window.cancelIdleCallback = (id: number): void => {
        pendingIdle.delete(id);
        nativeCancelIdle(id);
      };
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

    function pending(horizonMs: number): string[] {
      const out = running();
      for (const source of pendingFrames.values()) {
        out.push(`frame callback from ${source}`);
      }
      // An idle callback runs as soon as the page is idle, so in the window.
      for (const source of pendingIdle.values()) {
        out.push(`idle callback from ${source}`);
      }
      const now = performance.now();
      for (const t of timers()) {
        if (t.due - now < horizonMs) {
          out.push(`${describe(t)}, due in ${Math.max(0, Math.round(t.due - now))} ms`);
        } else if (fast(t)) {
          out.push(repeating(t));
        }
      }
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
              delta.timers.push(event.text);
              break;
            case 'fired':
              delta.timerCallbacks.push(event.text);
              break;
            case 'idle':
              delta.idleCallbacks += 1;
              break;
          }
        }
        return delta;
      },
      pending,
      settled(quietMs, timeoutMs, horizonMs) {
        const start = performance.now();
        let quietSince: number | null = null;
        return new Promise((resolve) => {
          const poll = (): void => {
            const now = performance.now();
            const busy = pending(horizonMs);
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
      repeaters: () => timers().filter(fast).map(repeating),
      live: () => timers().map(repeating),
      devTimers: () => devTimers,
    };
    (window as unknown as ProbeWindow).__idleProbe = probe;
  }, rule);
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
  /** Still live at the window's end and repeating faster than once a minute. */
  repeaters: string[];
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

/**
 * Wait for the settled condition before a window of `windowMs`: `[]`, or
 * what was still pending after `timeoutMs`.
 */
function settle(
  page: Page,
  timeoutMs = SETTLE_TIMEOUT_MS,
  windowMs = WINDOW_MS,
): Promise<string[]> {
  return page.evaluate(
    ([quiet, timeout, horizon]) =>
      (window as unknown as ProbeWindow).__idleProbe.settled(quiet, timeout, horizon),
    [QUIET_FOR_MS, timeoutMs, windowMs + WINDOW_MARGIN_MS] as const,
  );
}

function probeMark(page: Page): Promise<number> {
  return page.evaluate(() => (window as unknown as ProbeWindow).__idleProbe.mark());
}

function probeSince(page: Page, mark: number): Promise<ProbeDelta> {
  return page.evaluate((m) => (window as unknown as ProbeWindow).__idleProbe.since(m), mark);
}

function liveTimers(page: Page): Promise<string[]> {
  return page.evaluate(() => (window as unknown as ProbeWindow).__idleProbe.live());
}

/** Whether the page is the dev build, whose code keeps its names (the bundle's are minified). */
function isDevBuild(page: Page): Promise<boolean> {
  return page.evaluate(() => document.querySelector('script[src="/@vite/client"]') !== null);
}

/** One watch of a page: how long it may take to settle, then how long it is watched. */
interface Watch {
  settleTimeoutMs: number;
  windowMs: number;
}

const FULL_WATCH: Watch = { settleTimeoutMs: SETTLE_TIMEOUT_MS, windowMs: WINDOW_MS };
/** For the controls, which never settle: give up on settling soon, watch briefly. */
const CONTROL_WATCH: Watch = { settleTimeoutMs: 500, windowMs: 1_000 };
/**
 * For busy work held to the window's budgets (they are per `WINDOW_MS`):
 * give up on settling soon, watch a full window.
 */
const BUSY_WATCH: Watch = { settleTimeoutMs: 500, windowMs: WINDOW_MS };

/** Wait for the settled condition, then watch the page for `windowMs`. */
async function measure(
  page: Page,
  cdp: CDPSession,
  { settleTimeoutMs, windowMs }: Watch = FULL_WATCH,
): Promise<Measured> {
  const settling = Date.now();
  const unsettled = await settle(page, settleTimeoutMs, windowMs);
  const settleMs = Date.now() - settling;
  const before = await metrics(cdp);
  const mark = await probeMark(page);
  // The window itself: nothing to wait for, by definition.
  await page.waitForTimeout(windowMs);
  const delta = await probeSince(page, mark);
  const after = await metrics(cdp);
  // After the second metrics read: `getAnimations` may flush style itself.
  const { running, repeaters, devTimers } = await page.evaluate(() => {
    const probe = (window as unknown as ProbeWindow).__idleProbe;
    return { running: probe.running(), repeaters: probe.repeaters(), devTimers: probe.devTimers() };
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
    repeaters,
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
  if (m.repeaters.length > 0) {
    out.push(`repeating faster than once a minute: ${m.repeaters.join('; ')}`);
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
    `timers ${m.delta.timers.length}, idle callbacks ${m.delta.idleCallbacks}, ` +
    `animations ${m.running.length}, ` +
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
  // Every scenario settles and is watched for 3 s in its own context, all at
  // once; on a loaded host (other builds, a 2-core CI runner) nine of them
  // took longer than the default 60 s, so the budget grows with the count.
  test.setTimeout(Math.max(60_000, 20_000 * scenarios.length));
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

/** Windows' theme as the mock starts with it (`DEFAULT_THEME` in src/lib/ipc/mock.ts). */
const OS_THEME: ThemeInfo = {
  accent: '#0078D4',
  dark: true,
  highContrast: false,
  transparency: true,
};

/**
 * One fact of Windows' theme changed, as `dock://theme` reports it
 * (light mode, a contrast theme, transparency effects off): each switches
 * the page to other CSS (no blur, opaque fills, forced lines).
 */
function osThemeScenario(
  name: string,
  theme: Partial<ThemeInfo>,
  applied: (page: Page) => Promise<void>,
): Scenario {
  return {
    name,
    setup: async (page) => {
      await page.evaluate(
        (t) => {
          (window as unknown as DemoWindow).__dockMock?.setTheme(t);
        },
        { ...OS_THEME, ...theme },
      );
      await applied(page);
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
      const frames = Object.keys(seen.frames).join('\n');
      const timers = seen.timers.join('\n');
      expect(frames).not.toBe('');
      expect(timers).toMatch(/timeout 160 ms from /);
      // Only the dev build names the code; the bundle's names are minified.
      if (await isDevBuild(page)) {
        expect(frames).toMatch(/scheduleFrame .*Dock\.svelte/);
        expect(timers).toMatch(/timeout 160 ms from .*Dock\.svelte/);
      }
    },
  };
}

function setAutoHide(page: Page, mode: Settings['autoHide']['mode']): Promise<void> {
  return patchSettings(page, { autoHide: { mode, delayMs: 250 } });
}

test.describe('the dock page goes quiet once it has settled', () => {
  test('at rest on every edge, with each theme preset and with each OS theme fact', async ({
    browser,
  }) => {
    await expectQuiet(browser, [
      { name: 'bottom, the standard preset (the default)' },
      { name: 'top', query: 'position=top' },
      { name: 'left', query: 'position=left', viewport: SIDE_VIEWPORT },
      { name: 'right', query: 'position=right', viewport: SIDE_VIEWPORT },
      presetScenario('solid'),
      presetScenario('minimal'),
      osThemeScenario('light mode', { dark: false }, async (page) => {
        await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
      }),
      osThemeScenario('a contrast theme', { highContrast: true }, async (page) => {
        await expect(page.locator('html')).toHaveAttribute('data-high-contrast', '');
        await expect(page.locator(PANEL)).toHaveCSS('backdrop-filter', 'none');
      }),
      osThemeScenario('transparency effects off', { transparency: false }, async (page) => {
        await expect
          .poll(() =>
            page
              .locator('.dock-stage')
              .evaluate((el) => (el as HTMLElement).style.getPropertyValue('--panel-opacity')),
          )
          .toBe('0.95');
      }),
    ]);
  });

  test('after a hover, a launch bounce, a toast and a drag have ended', async ({ browser }) => {
    await expectQuiet(browser, [
      hoverScenario('bottom'),
      hoverScenario('left'),
      {
        // A settings change under a resting pointer works the scales
        // out again, once, and nothing repeats after it; nor after
        // magnification is turned on under it.
        name: 'magnification on, the pointer resting on an icon through a settings change',
        setup: async (page) => {
          await page.locator(APP_SLOT).nth(3).hover();
          await expect.poll(() => scaleOf(page, 3)).toBeGreaterThan(1.5);
          // As Rust reports it: a `dock://settings` and no new state, since
          // the items stay the same (the mock would send one).
          await page.evaluate(() => {
            (window as unknown as { __dockMock: Record<string, unknown> }).__dockMock.emitState =
              () => undefined;
          });
          await patchSettings(page, { iconSize: 56, magnification: { maxScale: 1.3 } });
          await expect.poll(() => scaleOf(page, 3)).toBeLessThanOrEqual(1.3);
          // Off, the pointer out and back while nothing magnifies, then on.
          await patchSettings(page, { magnification: { enabled: false } });
          await expect.poll(() => scaleOf(page, 3)).toBe(1);
          await page.mouse.move(10, 10);
          await page.locator(APP_SLOT).nth(3).hover();
          await patchSettings(page, { magnification: { enabled: true } });
          await expect.poll(() => scaleOf(page, 3)).toBeGreaterThan(1.1);
          await page.evaluate(() => {
            delete (window as unknown as { __dockMock: Record<string, unknown> }).__dockMock
              .emitState;
          });
          await expect(page.locator('.tooltip')).toHaveCount(1);
        },
      },
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
              // A style write and a layout read every frame, and 1 ms of
              // script: about 6 % of a core at 60 Hz, far over both budgets
              // on any host.
              width = (width + 1) % 8;
              box.style.width = `${width}px`;
              void box.offsetWidth;
              const until = performance.now() + 1;
              while (performance.now() < until) {
                // Spin.
              }
              requestAnimationFrame(spin);
            };
            requestAnimationFrame(spin);
            setInterval(() => undefined, 1_000);
          });
        },
        // A full window: the time budgets are per `WINDOW_MS`.
        watch: BUSY_WATCH,
        check: (m) => {
          const busy = busyWork(m).join('\n');
          expect.soft(busy, 'the probe sees the loop').toMatch(/never settled/);
          expect.soft(m.layouts, 'the loop lays out').toBeGreaterThan(0);
          expect.soft(m.styleRecalcs, 'the loop recalculates style').toBeGreaterThan(0);
          // Both budgets trip, so the checks on them can fail.
          expect.soft(m.scriptMs, 'the loop runs script').toBeGreaterThan(SCRIPT_BUDGET_MS);
          expect.soft(m.taskMs, 'the loop takes main-thread time').toBeGreaterThan(TASK_BUDGET_MS);
          expect.soft(busy, 'over the script budget').toMatch(/(^|\n)script \d+\.\d ms/);
          expect.soft(busy, 'over the task budget').toMatch(/(^|\n)tasks \d+\.\d ms/);
          expect.soft(busy, 'the frame callbacks').toMatch(/frame callbacks: \d+×/);
          expect
            .soft(busy, 'the interval')
            .toMatch(/repeating faster than once a minute: interval 1000 ms/);
          expect.soft(busy, 'its callback').toMatch(/timers fired: interval 1000 ms/);
        },
      },
      {
        name: 'after an attention wiggle ends (the window activated)',
        setup: async (page, measureNow) => {
          // Busy by design while the app asks for attention, for as
          // long as the window is not activated: an infinite CSS animation,
          // which the probe must see.
          await page.evaluate((id) => {
            (window as unknown as DemoWindow).__dockMock?.pushAttention(id, true);
          }, EXPLORER_WINDOW);
          await expect(page.locator(EXPLORER)).toHaveAttribute('data-attention', 'true');
          const wiggling = await measureNow(BUSY_WATCH);
          console.info(summary('the attention wiggle while it runs', wiggling));
          // (Svelte scopes the keyframes' name: "svelte-<hash>-wiggle".)
          expect(wiggling.unsettled.join('; ')).toMatch(/CSS animation "[\w-]*wiggle"/);
          expect(wiggling.running).toHaveLength(1);
          expect(wiggling.running[0]).toMatch(/^CSS animation "[\w-]*wiggle" on div\.wiggler/);
          // The bound it keeps: the compositor plays a transform, and the
          // main thread does nothing for it. No layout, no script, no frame,
          // timer or idle callback, the task budget of a settled page, and
          // at most WIGGLE_STYLE_RECALCS style recalculations a window.
          const bound = 'the running wiggle keeps its bound';
          expect.soft(wiggling.layouts, bound).toBe(0);
          expect.soft(wiggling.styleRecalcs, bound).toBeLessThanOrEqual(WIGGLE_STYLE_RECALCS);
          expect.soft(wiggling.scriptMs, bound).toBeLessThanOrEqual(SCRIPT_BUDGET_MS);
          expect.soft(wiggling.taskMs, bound).toBeLessThanOrEqual(TASK_BUDGET_MS);
          expect.soft(wiggling.delta, bound).toEqual({
            frames: {},
            timers: [],
            timerCallbacks: [],
            idleCallbacks: 0,
          });
          expect.soft(wiggling.repeaters, bound).toEqual([]);
          // Switched to without the dock (Alt+Tab): the flag clears.
          await page.evaluate((id) => {
            (window as unknown as DemoWindow).__dockMock?.pushForeground(id);
          }, EXPLORER_WINDOW);
          await expect(page.locator(EXPLORER)).not.toHaveAttribute('data-attention');
        },
      },
    ]);
  });

  test('timers and idle callbacks: work once a minute is quiet, a polling chain or an idle loop is not', async ({
    browser,
  }) => {
    await expectQuiet(browser, [
      {
        name: 'the clock and battery items on: the clock ticks once a minute, the battery is still',
        setup: async (page) => {
          await patchSettings(page, { items: { clock: true, battery: true } });
          const clock = page.locator(CLOCK);
          await expect(clock).toBeVisible();
          await expect(clock.locator('text.time')).toHaveText(/\d/);
          await expect(page.locator(BATTERY).locator('text.percent')).toHaveText(/^\d+%$/);
          // The page coming back into view (after a sleep) reads the time
          // again and re-arms the tick in place of the pending one.
          await page.evaluate(() => document.dispatchEvent(new Event('visibilitychange')));
          expect(await settle(page)).toEqual([]);
          // The clock aims its tick at the next minute of the wall clock
          // (`setTimeout(tick, msUntilNextMinute(now))`, a delay under a
          // minute every time): one timer is live, and the rule takes it as
          // once a minute, so the window below must be quiet with it.
          const live = await liveTimers(page);
          const minute = live.filter((t) => MINUTE_TIMER.test(t));
          expect(minute, live.join('\n')).toHaveLength(1);
          if (await isDevBuild(page)) {
            expect(minute[0]).toMatch(/from .*DockItem\.svelte/);
          }
        },
      },
      {
        name: 'the clock item on, then off again: its timer goes with it',
        setup: async (page) => {
          await patchSettings(page, { items: { clock: true } });
          await expect(page.locator(CLOCK).locator('text.time')).toHaveText(/\d/);
          expect((await liveTimers(page)).filter((t) => MINUTE_TIMER.test(t))).toHaveLength(1);
          await patchSettings(page, { items: { clock: false } });
          // Gone once it has shrunk away and unmounted.
          await expect(page.locator(CLOCK)).toHaveCount(0);
          expect(await settle(page)).toEqual([]);
          const live = await liveTimers(page);
          expect(
            live.filter((t) => MINUTE_TIMER.test(t)),
            live.join('\n'),
          ).toEqual([]);
          if (await isDevBuild(page)) {
            expect(live.filter((t) => t.includes('DockItem.svelte'))).toEqual([]);
          }
        },
      },
      {
        name: 'a timeout re-armed for a deadline a minute on, with a delay under a minute',
        setup: async (page) => {
          const mark = await probeMark(page);
          await page.evaluate(() => {
            const out = document.body.appendChild(document.createElement('output'));
            let deadline = performance.now() + 100;
            const arm = (): void => {
              setTimeout(run, deadline - performance.now());
            };
            const run = (): void => {
              // 200 ms of work before the re-arm, as on a busy main thread:
              // its delay is then under a minute by more than the slack,
              // while its period stays a minute.
              const until = performance.now() + 200;
              while (performance.now() < until) {
                // Spin.
              }
              out.textContent = String(deadline);
              deadline += 60_000;
              arm();
            };
            arm();
          });
          expect(await settle(page)).toEqual([]);
          // It did run once and re-arm with a delay of 59.0–59.9 s.
          const seen = await probeSince(page, mark);
          expect(seen.timerCallbacks.join('\n')).toMatch(/timeout \d+ ms from arm/);
          expect(seen.timers.join('\n')).toMatch(/timeout 59[0-8]\d{2} ms from arm/);
        },
      },
      {
        name: 'control: a 10 s polling chain is seen, though it never runs in the window',
        setup: async (page) => {
          await page.evaluate(() => {
            const poll = (delay: number): void => {
              setTimeout(() => poll(10_000), delay);
            };
            poll(100);
          });
        },
        watch: CONTROL_WATCH,
        check: (m) => {
          const busy = busyWork(m).join('\n');
          expect
            .soft(busy, 'it keeps the page from settling')
            .toMatch(
              /never settled: timeout 10000 ms from poll .*every 10\d{3} ms \(from the last/,
            );
          expect
            .soft(busy, 'it is still live at the end')
            .toMatch(/repeating faster than once a minute: timeout 10000 ms from poll/);
          expect.soft(m.delta.timerCallbacks, 'it did not run in the window').toEqual([]);
        },
      },
      {
        name: 'control: an idle-callback loop is seen',
        setup: async (page) => {
          await page.evaluate(() => {
            const idle = (): void => {
              requestIdleCallback(idle);
            };
            idle();
          });
        },
        watch: CONTROL_WATCH,
        check: (m) => {
          const busy = busyWork(m).join('\n');
          expect
            .soft(busy, 'it keeps the page from settling')
            .toMatch(/never settled: .*idle callback from idle/);
          expect.soft(busy, 'it runs in the window').toMatch(/\d+ idle callbacks/);
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
