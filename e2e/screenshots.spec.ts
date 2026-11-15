import { mkdirSync, writeFileSync } from 'node:fs';

import { expect, test, type Locator, type Page } from '@playwright/test';

import type { AppInfo } from '../src/lib/ipc/generated/AppInfo';
import type { DockState } from '../src/lib/ipc/generated/DockState';
import type { MonitorInfo } from '../src/lib/ipc/generated/MonitorInfo';
import type { PowerStatus } from '../src/lib/ipc/generated/PowerStatus';
import type { RawWindow } from '../src/lib/ipc/generated/RawWindow';
import type { Rect } from '../src/lib/ipc/generated/Rect';
import type { Settings } from '../src/lib/ipc/generated/Settings';
import type { ThemeInfo } from '../src/lib/ipc/generated/ThemeInfo';
import type { ThemePreset } from '../src/lib/ipc/generated/ThemePreset';
import type { UserCss } from '../src/lib/ipc/generated/UserCss';

const OUT = 'e2e/output';
const APP_SLOT = '.slot[data-kind="app"]';

// The dock's layout constants (src/dock/Dock.svelte) the assertions below rely on.
const ICON = 48;
const SLOT_GAP = 8;
const PANEL_PADDING = 8;

/** `FileDragEvent` of `src/lib/ipc/backend.ts` (structural: e2e has no `$lib` alias). */
type FileDrag =
  | { type: 'enter' | 'drop'; paths: string[]; position: { x: number; y: number } }
  | { type: 'over'; position: { x: number; y: number } }
  | { type: 'leave' };

/** What the pages expose in mock mode (`src/lib/ipc/demo.ts`; structural, for `page.evaluate`). */
interface DemoWindow {
  __dockMock?: {
    calls: Array<{ type: string } & Record<string, unknown>>;
    failNextCommand: (message: string) => void;
    simulateFileDrag: (event: FileDrag) => void;
    pinTarget: (target: unknown) => Promise<string>;
    deleteToRecycleBin: (paths: string[]) => Promise<void>;
    /** `RecycleBinChanged` as the platform's poll reports it. */
    pushRecycleBin: (state: { itemCount: number; sizeBytes: number }) => void;
    getSettings: () => Promise<Settings>;
    getDockState: () => Promise<DockState>;
    updateSettings: (settings: Settings) => Promise<Settings>;
    pushWindow: (raw: RawWindow) => void;
    pushWindowRemoved: (id: number) => void;
    pushForeground: (id: number | null) => void;
    /** `HSHELL_FLASH` as the platform reports it (`WindowAttention`). */
    pushAttention: (id: number, flashing: boolean) => void;
    /** `FullscreenChanged` as the platform reports it, with the window's monitor. */
    setFullscreen: (active: boolean, monitor?: string | null) => void;
    /** A window moved or resized: its new frame in screen px. */
    moveWindow: (id: number, rect: Rect | null) => void;
    /** Where the engine believes this page's dock panel rests (screen px). */
    dockPanel: (label?: string) => Rect | null;
    /** The label of the dock window this page stands for. */
    pageLabel: string;
    /** `dock://theme`: what Windows reports for accent, mode, contrast and transparency. */
    setTheme: (theme: ThemeInfo) => void;
    /** `dock://show`: the tray or the shortcut pressed while the panel is auto-hidden. */
    requestShow: () => void;
    /**
     * A press of the focus shortcut: `dock://release-focus` while the dock has the
     * keyboard from an earlier press, else `dock://show` if the panel is slid out, then
     * `dock://focus`.
     */
    requestFocus: () => void;
    /** What `get_app_info` answers from now on. */
    setAppInfo: (info: Partial<AppInfo>) => void;
    /** Change the user's dock.css on "disk" (`null` deletes it); read at the next reload. */
    writeUserCss: (css: string | null) => void;
    /** `reload_user_css`: read it again and send `dock://user-css`. */
    reloadUserCss: () => Promise<UserCss>;
    /** The mock's dock.css on "disk" (`null`: none). */
    userCssOnDisk: () => string | null;
    /** `PowerChanged` as the events thread reports it: only a different reading counts. */
    setPower: (status: PowerStatus) => void;
  };
}

/** What `installMediaQueryShim` adds to the page. */
interface MediaQueryShimWindow {
  __fireMediaQueryChanges?: () => number;
}

/**
 * Chromium's device emulation (`Emulation.setDeviceMetricsOverride`)
 * changes `devicePixelRatio` and what a fresh `matchMedia('(resolution:
 * …dppx)')` answers, but does not notify the `MediaQueryList`s a page
 * already holds, which a real DPI change does (the dock's watcher,
 * src/lib/dock/dpr.ts, relies on that `change` event and assumes WebView2
 * sends it). Installed before the page loads, this records every list and
 * its `change` listeners; `__fireMediaQueryChanges` then dispatches `change`
 * to the lists whose answer flipped, and says how many did. The dock's own
 * path runs for real; only the browser's missing notification is stood in
 * for.
 */
async function installMediaQueryShim(page: Page): Promise<void> {
  await page.addInitScript(() => {
    interface Entry {
      list: MediaQueryList;
      listeners: Set<EventListenerOrEventListenerObject>;
      matches: boolean;
    }
    const native = window.matchMedia.bind(window);
    const entries: Entry[] = [];
    window.matchMedia = (query: string): MediaQueryList => {
      const list = native(query);
      const entry: Entry = { list, listeners: new Set(), matches: list.matches };
      const add = list.addEventListener.bind(list);
      const remove = list.removeEventListener.bind(list);
      list.addEventListener = ((
        type: string,
        listener: EventListenerOrEventListenerObject,
        options?: boolean | AddEventListenerOptions,
      ): void => {
        if (type === 'change') {
          entry.listeners.add(listener);
        }
        add(type, listener, options);
      }) as typeof list.addEventListener;
      list.removeEventListener = ((
        type: string,
        listener: EventListenerOrEventListenerObject,
        options?: boolean | EventListenerOptions,
      ): void => {
        if (type === 'change') {
          entry.listeners.delete(listener);
        }
        remove(type, listener, options);
      }) as typeof list.removeEventListener;
      entries.push(entry);
      return list;
    };
    (window as unknown as MediaQueryShimWindow).__fireMediaQueryChanges = (): number => {
      let fired = 0;
      for (const entry of entries) {
        const matches = native(entry.list.media).matches;
        if (matches === entry.matches) {
          continue;
        }
        entry.matches = matches;
        const event = new MediaQueryListEvent('change', { media: entry.list.media, matches });
        for (const listener of [...entry.listeners]) {
          if (typeof listener === 'function') {
            listener.call(entry.list, event);
          } else {
            listener.handleEvent(event);
          }
        }
        fired += 1;
      }
      return fired;
    };
  });
}

/** The mock monitor's work area (`demoMonitor` in src/lib/ipc/mock.ts). */
const WORK_AREA: Rect = { x: 0, y: 0, width: 2560, height: 1392 };

/** The mock's demo theme facts (`DEFAULT_THEME` in src/lib/ipc/mock.ts). */
const OS_THEME: ThemeInfo = {
  accent: '#0078D4',
  dark: true,
  highContrast: false,
  transparency: true,
};

/** Report new OS theme facts through the mock, as `dock://theme` would. */
function setOsTheme(page: Page, theme: Partial<ThemeInfo>): Promise<void> {
  return page.evaluate(
    (t) => {
      (window as unknown as DemoWindow).__dockMock?.setTheme(t);
    },
    { ...OS_THEME, ...theme },
  );
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

/** Running animations anywhere in the page (CSS animations and transitions, Web Animations). */
function runningAnimations(page: Page): Promise<number> {
  return page.evaluate(
    () => document.getAnimations().filter((a) => a.playState === 'running').length,
  );
}

/** The folders `open_app_folder` was asked for so far, in order. */
function folderCalls(page: Page): Promise<unknown[]> {
  return page.evaluate(() =>
    ((window as unknown as DemoWindow).__dockMock?.calls ?? [])
      .filter((c) => c.type === 'openAppFolder')
      .map((c) => c.folder),
  );
}

/** Window `id` flashes its taskbar button (`HSHELL_FLASH` → `WindowAttention`), or stops. */
function flash(page: Page, id: number, flashing = true): Promise<void> {
  return page.evaluate(
    ([i, f]) => {
      (window as unknown as DemoWindow).__dockMock?.pushAttention(i, f);
    },
    [id, flashing] as const,
  );
}

/** Window `id` became the foreground window by other means than the dock (Alt+Tab, a click on it). */
function foreground(page: Page, id: number | null): Promise<void> {
  return page.evaluate((i) => {
    (window as unknown as DemoWindow).__dockMock?.pushForeground(i);
  }, id);
}

/**
 * A fullscreen app took the screen (`FullscreenChanged { active: true }`)
 * of `monitor` (`null`, the default, when the platform could not tell: the
 * primary monitor's dock), or gave it back.
 */
function fullscreen(page: Page, active: boolean, monitor: string | null = null): Promise<void> {
  return page.evaluate(
    ([a, m]) => {
      (window as unknown as DemoWindow).__dockMock?.setFullscreen(a, m);
    },
    [active, monitor] as const,
  );
}

/**
 * The stock taskbar is auto-hidden (by Windows' own setting, or by Stem):
 * every monitor's work area reaches its bottom edge, as `get_monitors` then
 * reports it, so the dock window's bottom edge is the screen's. (The mock's
 * monitors keep a visible taskbar's 48 px, like a default desktop.)
 * Installed before the page loads: the mock is wrapped the moment the page
 * exposes it (src/lib/ipc/demo.ts, before mounting), before the store asks.
 */
async function withTaskbarAutoHidden(page: Page): Promise<void> {
  await page.addInitScript(() => {
    interface Monitors {
      getMonitors: () => Promise<MonitorInfo[]>;
    }
    let mock: Monitors | undefined;
    Object.defineProperty(window, '__dockMock', {
      configurable: true,
      get: () => mock,
      set: (value: Monitors) => {
        const original = value.getMonitors.bind(value);
        value.getMonitors = async () =>
          (await original()).map((m) => ({
            ...m,
            workArea: { ...m.workArea, height: m.bounds.y + m.bounds.height - m.workArea.y },
          }));
        mock = value;
      },
    });
  });
}

/** Window `id` was moved or resized to `rect` (screen px of the mock monitor), as the platform reports it. */
function moveWindow(page: Page, id: number, rect: Rect | null): Promise<void> {
  return page.evaluate(
    ([i, r]) => {
      (window as unknown as DemoWindow).__dockMock?.moveWindow(i, r);
    },
    [id, rect] as const,
  );
}

/** Where the mock's engine believes the dock's panel rests (screen px), or `null`. */
function dockPanel(page: Page): Promise<Rect | null> {
  return page.evaluate(() => (window as unknown as DemoWindow).__dockMock?.dockPanel() ?? null);
}

/** One computed style property of the first element `locator` matches, or of its pseudo-element. */
function computed(
  locator: Locator,
  property: string,
  pseudo: string | null = null,
): Promise<string> {
  return locator
    .first()
    .evaluate((el, [p, ps]) => getComputedStyle(el, ps).getPropertyValue(p), [
      property,
      pseudo,
    ] as const);
}

/** The mock's recorded side effects of one type so far. */
function callsOfType(page: Page, type: string): Promise<number> {
  return page.evaluate(
    (t) =>
      ((window as unknown as DemoWindow).__dockMock?.calls ?? []).filter((c) => c.type === t)
        .length,
    type,
  );
}

interface HitRegionCall {
  panel: Rect;
  envelope: Rect;
}

/** Every `set_dock_hit_region` the mock recorded so far, oldest first (the two rects only). */
function hitRegions(page: Page): Promise<HitRegionCall[]> {
  return page.evaluate(() =>
    ((window as unknown as DemoWindow).__dockMock?.calls ?? [])
      .filter((c) => c.type === 'setDockHitRegion')
      .map((c) => ({ panel: c.panel, envelope: c.envelope }) as unknown as HitRegionCall),
  );
}

/** The `hidden` flag of every `set_dock_auto_hidden` the mock recorded so far, oldest first. */
function autoHiddenReports(page: Page): Promise<boolean[]> {
  return page.evaluate(() =>
    ((window as unknown as DemoWindow).__dockMock?.calls ?? [])
      .filter((c) => c.type === 'setDockAutoHidden')
      .map((c) => c.hidden as boolean),
  );
}

/** Every `set_dock_content_size` the mock recorded so far, oldest first. */
function contentSizes(page: Page): Promise<Array<{ width: number; height: number }>> {
  return page.evaluate(
    () =>
      ((window as unknown as DemoWindow).__dockMock?.calls ?? []).filter(
        (c) => c.type === 'setDockContentSize',
      ) as unknown as Array<{ width: number; height: number }>,
  );
}

/** One `set_dock_panel_rect` with its provenance (docs/ipc/report-provenance.md). */
interface PanelRectCall {
  rect: Rect;
  edge: string;
  viewport: { width: number; height: number };
}

/** Every `set_dock_panel_rect` the mock recorded so far, oldest first. */
function panelRects(page: Page): Promise<PanelRectCall[]> {
  return page.evaluate(() =>
    ((window as unknown as DemoWindow).__dockMock?.calls ?? [])
      .filter((c) => c.type === 'setDockPanelRect')
      .map((c) => ({ rect: c.rect, edge: c.edge, viewport: c.viewport }) as PanelRectCall),
  );
}

/**
 * The three geometry reports the mock recorded from call `from` on, oldest
 * first: each one's type and the edge it names.
 */
function geometryReports(page: Page, from = 0): Promise<Array<{ type: string; edge: unknown }>> {
  return page.evaluate(
    (m) =>
      ((window as unknown as DemoWindow).__dockMock?.calls ?? [])
        .slice(m)
        .filter(
          (c) =>
            c.type === 'setDockContentSize' ||
            c.type === 'setDockHitRegion' ||
            c.type === 'setDockPanelRect',
        )
        .map((c) => ({ type: c.type, edge: c.edge })),
    from,
  );
}

/**
 * How many side effects the mock has recorded once every command the page
 * issued so far has run (the mock runs them in order, so one more command
 * waits for them all): a mark for `geometryReports`.
 */
function callCount(page: Page): Promise<number> {
  return page.evaluate(async () => {
    const mock = (window as unknown as DemoWindow).__dockMock;
    await mock?.getSettings();
    return (mock?.calls ?? []).length;
  });
}

function contains(outer: Rect, inner: Rect): boolean {
  return (
    inner.x >= outer.x &&
    inner.y >= outer.y &&
    inner.x + inner.width <= outer.x + outer.width &&
    inner.y + inner.height <= outer.y + outer.height
  );
}

/** A window the mock has never seen: pushing it adds a running, unpinned app. */
const PAINT_WINDOW: RawWindow = {
  id: 99,
  title: 'Untitled - Paint',
  className: 'MSPaintApp',
  processId: 1099,
  exePath: 'C:\\Windows\\System32\\mspaint.exe',
  appUserModelId: null,
  isVisible: true,
  isMinimized: false,
  isCloaked: false,
  onCurrentDesktop: true,
  rect: null,
  monitor: 'mock-primary',
  iconKey: null,
};
const PAINT_SLOT = `${APP_SLOT}[data-key="app:exe:c:/windows/system32/mspaint.exe"]`;
/** Code's only window in the demo scenario; closing it removes the (unpinned) item. */
const CODE_WINDOW_ID = 4;
const CODE_KEY = 'app:exe:c:/users/me/appdata/local/programs/microsoft vs code/code.exe';

test.beforeAll(() => {
  mkdirSync(OUT, { recursive: true });
});

/** Open the dock in mock mode and wait until every app icon is on screen. */
async function openDock(page: Page, query = 'mock=1'): Promise<void> {
  await page.goto(`/dock.html?${query}`);
  await expect(page.locator('.dock-panel[data-ready="true"]')).toBeVisible();
  // Identities resolve ~300 ms after start; every app item then has an icon.
  await expect(page.locator(`${APP_SLOT}:not([data-icon="loaded"])`)).toHaveCount(0);
  await page.waitForFunction(() =>
    Array.from(document.images).every((img) => img.complete && img.naturalWidth > 0),
  );
}

/** The magnification scale a slot currently renders with. */
function scaleOf(page: Page, index: number): Promise<number> {
  return page
    .locator(APP_SLOT)
    .nth(index)
    .evaluate((el) => Number(el.style.getPropertyValue('--scale') || '1'));
}

/** One app slot's two bitmaps: their natural widths, which is displayed, and the switch. */
interface IconPair {
  /** Natural width of `img.rest`, the bitmap drawn 1:1 at rest (`null` when absent). */
  rest: number | null;
  /** Natural width of `img.magnified`, the ladder bitmap (`null` when the rest one serves both). */
  magnified: number | null;
  /** Natural widths of the images actually displayed: exactly one at any time. */
  shown: number[];
  /** `data-magnified` on the slot, set by the dock's magnification controller. */
  magnifiedAttr: boolean;
}

function iconPairs(page: Page): Promise<IconPair[]> {
  return page.$$eval(APP_SLOT, (slots) =>
    slots.map((slot) => {
      const rest = slot.querySelector<HTMLImageElement>('img.rest');
      const magnified = slot.querySelector<HTMLImageElement>('img.magnified');
      return {
        rest: rest?.naturalWidth ?? null,
        magnified: magnified?.naturalWidth ?? null,
        shown: [rest, magnified]
          .filter(
            (img): img is HTMLImageElement =>
              img !== null && getComputedStyle(img).display !== 'none',
          )
          .map((img) => img.naturalWidth),
        magnifiedAttr: slot.hasAttribute('data-magnified'),
      };
    }),
  );
}

/**
 * Every app slot has exactly `expected` bitmaps. The magnified ones are
 * requested behind the rest ones (`openDock` only waits for the latter), so
 * this polls until they have arrived.
 */
async function expectIconPairs(page: Page, expected: IconPair): Promise<void> {
  const count = await page.locator(APP_SLOT).count();
  expect(count).toBeGreaterThan(0);
  await expect.poll(() => iconPairs(page)).toEqual(Array.from({ length: count }, () => expected));
}

/**
 * The panel's and one icon's top-left corner in device pixels, and the
 * inline snap margins. `icon` is the first app icon's rest bitmap
 * unless another selector is given.
 */
function snapState(
  page: Page,
  iconSelector = `${APP_SLOT} img.rest`,
): Promise<{
  dpr: number;
  panel: { left: number; top: number };
  icon: { left: number; top: number };
  marginLeft: string;
  marginBottom: string;
}> {
  return page.evaluate((selector) => {
    const panel = document.querySelector<HTMLElement>('.dock-panel')!;
    const icon = document.querySelector<HTMLElement>(selector)!;
    const p = panel.getBoundingClientRect();
    const i = icon.getBoundingClientRect();
    const dpr = window.devicePixelRatio;
    return {
      dpr,
      panel: { left: p.left * dpr, top: p.top * dpr },
      icon: { left: i.left * dpr, top: i.top * dpr },
      marginLeft: panel.style.marginLeft,
      marginBottom: panel.style.marginBottom,
    };
  }, iconSelector);
}

/**
 * Chromium lays out in 1/64 CSS px, so at a fractional device pixel ratio
 * a snapped edge can miss the whole device pixel by that much (times the
 * ratio); at 1× and 2× the snap is exact (src/lib/dock/geometry.ts).
 */
function expectWholeDevicePx(value: number, dpr: number): void {
  expect(Math.abs(value - Math.round(value))).toBeLessThanOrEqual(dpr / 64);
}

test.describe('dock at 1× device pixel ratio', () => {
  test.use({ deviceScaleFactor: 1 });

  test('rest icons are exact-size bitmaps on whole pixels; magnified ones come from the ladder', async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await openDock(page);
    // Two bitmaps per app icon: 48 × 1 = 48 px for rest (drawn 1:1) and, for
    // the true magnified size 48 × 1.6 × 1 = 76.8 px, the ladder's 96.
    await expectIconPairs(page, { rest: 48, magnified: 96, shown: [48], magnifiedAttr: false });
    // The 559 px panel (9 slots × 56 + 3 separators × 13 + 16 padding) centred in
    // 1280 px would start at x = 360.5; a 1 px margin on a flex-centred box
    // moves it by half a pixel, onto 361. Nothing is fractional vertically.
    const atRest = await snapState(page);
    expect(atRest.marginLeft).toBe('1px');
    expect(atRest.panel.left).toBe(361);
    expect(atRest.panel.top).toBe(Math.round(atRest.panel.top));
    expect(atRest.icon.left).toBe(Math.round(atRest.icon.left));
    expect(atRest.icon.top).toBe(Math.round(atRest.icon.top));
    await page.screenshot({ path: `${OUT}/dock-dark-1x.png` });

    // Hovering: every slot whose box grew shows the ladder bitmap; the rest
    // bitmap is never drawn in a box larger than itself.
    await page.locator(APP_SLOT).nth(3).hover();
    await expect.poll(() => scaleOf(page, 3)).toBeGreaterThan(1.5);
    const hovered = await iconPairs(page);
    expect(hovered[3]).toMatchObject({ shown: [96], magnifiedAttr: true });
    expect(hovered[2]).toMatchObject({ shown: [96], magnifiedAttr: true });
    expect(hovered[0]).toMatchObject({ shown: [48], magnifiedAttr: false });
    for (let i = 0; i < hovered.length; i += 1) {
      expect(hovered[i]!.shown).toEqual([(await scaleOf(page, i)) > 1 ? 96 : 48]);
    }

    // Leaving: the scale is reset at once but the boxes ease back over
    // 120 ms, so the ladder bitmap stays until they are at rest again.
    await page.mouse.move(10, 10);
    const settling = await page.evaluate(async (selector) => {
      const slot = document.querySelectorAll<HTMLElement>(selector)[3]!;
      for (let i = 0; i < 10 && slot.style.getPropertyValue('--scale') !== '1'; i += 1) {
        await new Promise(requestAnimationFrame);
      }
      return {
        scale: slot.style.getPropertyValue('--scale'),
        magnifiedAttr: slot.hasAttribute('data-magnified'),
        shown: getComputedStyle(slot.querySelector('img.magnified')!).display,
        panelSettling: document.querySelector('.dock-panel')!.classList.contains('settling'),
      };
    }, APP_SLOT);
    expect(settling).toEqual({
      scale: '1',
      magnifiedAttr: true,
      shown: 'block',
      panelSettling: true,
    });
    await expect
      .poll(async () => (await iconPairs(page))[3])
      .toEqual({ rest: 48, magnified: 96, shown: [48], magnifiedAttr: false });
    expect((await iconPairs(page)).every((p) => !p.magnifiedAttr)).toBe(true);
    // Measured again after the settle: the snap is idempotent.
    expect(await snapState(page)).toEqual(atRest);
  });

  test('a scale change re-requests the icons at the new ratio and reports the geometry again', async ({
    page,
  }) => {
    await installMediaQueryShim(page);
    await page.emulateMedia({ colorScheme: 'dark' });
    await openDock(page);
    await expectIconPairs(page, { rest: 48, magnified: 96, shown: [48], magnifiedAttr: false });
    // The reports made at 1×; the dock sends each only when its numbers
    // change, so what follows is the scale change's doing.
    const sizesBefore = await contentSizes(page);
    const regionsBefore = await hitRegions(page);
    const panelsBefore = await callsOfType(page, 'setDockPanelRect');
    expect(sizesBefore.length).toBeGreaterThan(0);
    expect(regionsBefore.length).toBeGreaterThan(0);
    expect(panelsBefore).toBeGreaterThan(0);

    // The display scale goes to 200 %: the viewport keeps its CSS size,
    // `devicePixelRatio` changes and the `(resolution: 1dppx)` query stops
    // matching. Chromium's device emulation (CDP; Playwright has no
    // page-level API for it, so the context's own emulation is overridden
    // through a session) does that much, but it does not fire the lists'
    // `change` event as a real DPI change does: the shim installed above
    // dispatches it to the one list that flipped, the dock's.
    const cdp = await page.context().newCDPSession(page);
    await cdp.send('Emulation.setDeviceMetricsOverride', {
      width: 1280,
      height: 240,
      deviceScaleFactor: 2,
      mobile: false,
    });
    await expect.poll(() => page.evaluate(() => window.devicePixelRatio)).toBe(2);
    expect(
      await page.evaluate(() =>
        (window as unknown as MediaQueryShimWindow).__fireMediaQueryChanges!(),
      ),
    ).toBe(1);

    // Every slot keeps its 1× bitmap on screen until its 2× one has arrived
    // (never the loading placeholder in between): 96 px at rest, drawn 1:1.
    const swap = await page.evaluate(async (selector) => {
      let placeholderSeen = false;
      for (let frame = 0; frame < 600; frame += 1) {
        const slots = Array.from(document.querySelectorAll<HTMLElement>(selector));
        if (slots.some((slot) => slot.dataset.icon !== 'loaded')) {
          placeholderSeen = true;
        }
        const rests = slots.map(
          (slot) => slot.querySelector<HTMLImageElement>('img.rest')?.naturalWidth ?? 0,
        );
        if (rests.every((width) => width === 96)) {
          return { placeholderSeen, done: true };
        }
        await new Promise(requestAnimationFrame);
      }
      return { placeholderSeen, done: false };
    }, APP_SLOT);
    expect(swap).toEqual({ placeholderSeen: false, done: true });
    // 48 × 2 = 96 px at rest; 48 × 1.6 × 2 = 153.6 → the ladder's 192 magnified.
    await expectIconPairs(page, { rest: 96, magnified: 192, shown: [96], magnifiedAttr: false });

    // The panel is measured again on the new device pixel grid and
    // the three reports go out once more. Their CSS numbers are the ones
    // from 1× (the viewport did not change); Rust converts them at the new
    // scale.
    await expect.poll(() => contentSizes(page)).toHaveLength(sizesBefore.length + 1);
    expect((await contentSizes(page)).at(-1)).toEqual(sizesBefore.at(-1));
    await expect.poll(() => hitRegions(page)).toHaveLength(regionsBefore.length + 1);
    expect((await hitRegions(page)).at(-1)).toEqual(regionsBefore.at(-1));
    await expect.poll(() => callsOfType(page, 'setDockPanelRect')).toBe(panelsBefore + 1);
    const atRest = await snapState(page);
    expect(atRest.dpr).toBe(2);
    expectWholeDevicePx(atRest.panel.left, 2);
    expectWholeDevicePx(atRest.panel.top, 2);
    expectWholeDevicePx(atRest.icon.left, 2);
    expectWholeDevicePx(atRest.icon.top, 2);
    // `page.screenshot` puts the context's own emulation (1×) back; the
    // session that set 2× captures the page as it is: 2560 × 480 device px.
    const shot = await cdp.send('Page.captureScreenshot', { format: 'png' });
    writeFileSync(`${OUT}/dock-dark-1x-to-2x.png`, Buffer.from(shot.data, 'base64'));
    expect(await page.evaluate(() => window.devicePixelRatio)).toBe(2);

    // Hovering draws the new ladder bitmap.
    await page.locator(APP_SLOT).nth(3).hover();
    await expect.poll(() => scaleOf(page, 3)).toBeGreaterThan(1.5);
    expect((await iconPairs(page))[3]).toMatchObject({ shown: [192], magnifiedAttr: true });
  });
});

test.describe('dock at 1.5× device pixel ratio', () => {
  // An odd viewport height puts the panel's top edge (241 − 8 − 64 = 169 CSS
  // px = 253.5 device px) on a half pixel, so the vertical snap is exercised
  // as well as the horizontal one (360.5 CSS px = 540.75 device px).
  test.use({ deviceScaleFactor: 1.5, viewport: { width: 1280, height: 241 } });

  test('snaps the panel both ways and draws 72 px rest bitmaps', async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await openDock(page);
    // 48 × 1.5 = 72 px at rest; 48 × 1.6 × 1.5 = 115.2 → the ladder's 128 magnified.
    await expectIconPairs(page, { rest: 72, magnified: 128, shown: [72], magnifiedAttr: false });
    // The first slot (Start) sits right after the panel's 8 px padding: its
    // icon box is on whole device pixels exactly when the panel is.
    const atRest = await snapState(page, '.slot .icon');
    expect(atRest.dpr).toBe(1.5);
    // margin-left = 2 × (541 − 540.75) / 1.5; margin-bottom = 8 − (254 − 253.5) / 1.5
    // (inline style lengths come back with six significant digits).
    expect(parseFloat(atRest.marginLeft)).toBeCloseTo(1 / 3, 4);
    expect(parseFloat(atRest.marginBottom)).toBeCloseTo(8 - 1 / 3, 4);
    expectWholeDevicePx(atRest.panel.left, 1.5);
    expectWholeDevicePx(atRest.panel.top, 1.5);
    expectWholeDevicePx(atRest.icon.left, 1.5);
    expectWholeDevicePx(atRest.icon.top, 1.5);
    // Known and accepted at 150 %: a separator
    // is 13 CSS px = 19.5 device px wide, so every icon after one sits on a
    // half device pixel. Slots themselves are 56 CSS px = 84 device px.
    const firstApp = await snapState(page);
    expect(Math.abs(firstApp.icon.left - Math.round(firstApp.icon.left))).toBeCloseTo(0.5, 1);
    expectWholeDevicePx(firstApp.icon.top, 1.5);
    await page.screenshot({ path: `${OUT}/dock-dark-1.5x.png` });

    await page.locator(APP_SLOT).nth(3).hover();
    await expect.poll(() => scaleOf(page, 3)).toBeGreaterThan(1.5);
    expect((await iconPairs(page))[3]).toMatchObject({ shown: [128], magnifiedAttr: true });
    await page.mouse.move(10, 10);
    await expect
      .poll(async () => (await iconPairs(page))[3])
      .toMatchObject({ shown: [72], magnifiedAttr: false });
    expect(await snapState(page, '.slot .icon')).toEqual(atRest);
  });
});

test.describe('dock in mock mode', () => {
  test('dark theme at rest', async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await openDock(page);
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
    // At 2×: 96 px rest bitmaps (48 × 2, drawn 1:1) and 192 px magnified
    // ones (48 × 1.6 × 2 = 153.6 → the ladder's 192).
    await expectIconPairs(page, { rest: 96, magnified: 192, shown: [96], magnifiedAttr: false });
    // 360.5 CSS px is 721 device px at 2×: already whole, nothing to snap.
    const atRest = await snapState(page);
    expect(atRest.marginLeft).toBe('');
    expect(atRest.panel.left).toBe(721);
    expect(atRest.panel.top).toBe(Math.round(atRest.panel.top));
    expect(atRest.icon.left).toBe(Math.round(atRest.icon.left));
    await page.screenshot({ path: `${OUT}/dock-dark.png` });
  });

  test('the panel is padded evenly, with the running dots inside the bottom padding', async ({
    page,
  }) => {
    await openDock(page);
    const panel = (await page.locator('.dock-panel').boundingBox())!;
    expect(panel.height).toBe(ICON + 2 * PANEL_PADDING);
    const explorer = page.locator(APP_SLOT).nth(0);
    const icon = (await explorer.locator('.icon').boundingBox())!;
    expect(icon.height).toBe(ICON);
    expect(icon.y - panel.y).toBe(PANEL_PADDING);
    expect(panel.y + panel.height - (icon.y + icon.height)).toBe(PANEL_PADDING);
    // Explorer is running: its dot sits below the icon, inside the panel.
    const dots = (await explorer.locator('.dots').boundingBox())!;
    expect(dots.y).toBeGreaterThanOrEqual(icon.y + icon.height + 2);
    expect(dots.y + dots.height).toBeLessThanOrEqual(panel.y + panel.height - 2);
    // A separator is centred on the icons.
    const separator = (await page.locator('[role="separator"]').first().boundingBox())!;
    expect(separator.y + separator.height / 2).toBeCloseTo(icon.y + icon.height / 2, 0);
  });

  test('reports where it accepts clicks (docs/ipc/hit-region.md, v1.3)', async ({ page }) => {
    await openDock(page);
    // One report at startup; the identity snapshot that follows changes no geometry.
    await expect.poll(() => hitRegions(page)).toHaveLength(1);
    const [region] = await hitRegions(page);
    const box = (await page.locator('.dock-panel').boundingBox())!;
    const viewport = page.viewportSize()!;
    const windowRect = { x: 0, y: 0, width: viewport.width, height: viewport.height };
    // The panel: its bounding box rounded outwards to whole CSS px.
    const x = Math.floor(box.x);
    const y = Math.floor(box.y);
    expect(region!.panel).toEqual({
      x,
      y,
      width: Math.ceil(box.x + box.width) - x,
      height: Math.ceil(box.y + box.height) - y,
    });
    // The envelope: contains the panel, lies in the window, grows up and sideways only.
    expect(contains(region!.envelope, region!.panel)).toBe(true);
    expect(contains(windowRect, region!.panel)).toBe(true);
    expect(contains(windowRect, region!.envelope)).toBe(true);
    expect(region!.envelope.y + region!.envelope.height).toBe(
      region!.panel.y + region!.panel.height,
    );
    expect(region!.envelope.x).toBeLessThan(region!.panel.x);
    expect(region!.envelope.y).toBeLessThan(region!.panel.y);
    expect(region!.envelope.x + region!.envelope.width).toBeGreaterThan(
      region!.panel.x + region!.panel.width,
    );
    // The envelope ends 2 px above a fully magnified icon's top edge
    // (48 px icons at 1.6×: 28.8 px above the slot, which sits 8 px below the
    // panel's top edge), not in an empty band where clicks would be swallowed.
    expect(region!.panel.y - region!.envelope.y).toBe(Math.ceil(48 * 0.6 - 8 + 2));
  });

  test('app items animate in and out and the panel follows', async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await openDock(page);
    const panel = page.locator('.dock-panel');
    const widthBefore = (await panel.boundingBox())!.width;
    await expect(page.locator(APP_SLOT)).toHaveCount(7);
    const reportsBefore = (await hitRegions(page)).length;

    // Closing Code's only window removes its item: the slot stays while it
    // shrinks away, marked `data-leaving` and inert.
    const leaving = await page.evaluate(async (id) => {
      (window as unknown as DemoWindow).__dockMock?.pushWindowRemoved(id);
      for (let i = 0; i < 3; i += 1) {
        await new Promise(requestAnimationFrame);
      }
      const el = document.querySelector<HTMLElement>('.slot[data-leaving]');
      return el === null
        ? null
        : {
            key: el.dataset.key,
            inert: el.inert,
            pointerEvents: getComputedStyle(el).pointerEvents,
            width: el.getBoundingClientRect().width,
          };
    }, CODE_WINDOW_ID);
    expect(leaving).not.toBeNull();
    expect(leaving!.key).toBe(CODE_KEY);
    expect(leaving!.inert).toBe(true);
    expect(leaving!.pointerEvents).toBe('none');
    expect(leaving!.width).toBeLessThan(ICON + SLOT_GAP);
    // Gone once the animation ended; the panel is one slot narrower.
    await expect(page.locator('[data-leaving]')).toHaveCount(0);
    await expect(page.locator(APP_SLOT)).toHaveCount(6);
    await expect(page.locator(`[data-key="${CODE_KEY}"]`)).toHaveCount(0);
    await expect
      .poll(async () => (await panel.boundingBox())!.width)
      .toBe(widthBefore - (ICON + SLOT_GAP));
    // The new rest geometry was reported once the animation had ended.
    await expect.poll(() => hitRegions(page)).toHaveLength(reportsBefore + 1);

    // A window of an app the dock has never seen: its item grows into place.
    const entering = await page.evaluate(
      async ([raw, selector]) => {
        (window as unknown as DemoWindow).__dockMock?.pushWindow(raw as RawWindow);
        for (let i = 0; i < 3; i += 1) {
          await new Promise(requestAnimationFrame);
        }
        const el = document.querySelector<HTMLElement>(selector as string);
        return el === null
          ? null
          : { animations: el.getAnimations().length, width: el.getBoundingClientRect().width };
      },
      [PAINT_WINDOW, PAINT_SLOT] as const,
    );
    expect(entering).not.toBeNull();
    expect(entering!.width).toBeLessThan(ICON + SLOT_GAP);
    expect(entering!.animations).toBeGreaterThan(0);
    const paint = page.locator(PAINT_SLOT);
    await expect(paint).toHaveAttribute('aria-label', /Paint|mspaint/);
    await expect.poll(async () => (await paint.boundingBox())!.width).toBe(ICON + SLOT_GAP);
    await expect(page.locator(APP_SLOT)).toHaveCount(7);
    expect((await panel.boundingBox())!.width).toBe(widthBefore);
    await expect.poll(() => hitRegions(page)).toHaveLength(reportsBefore + 2);
  });

  test('light theme at rest', async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'light' });
    await openDock(page);
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
    await page.screenshot({ path: `${OUT}/dock-light.png` });
  });

  test('hovering the fourth app icon magnifies it and shows its tooltip', async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await openDock(page);
    const fourth = page.locator(APP_SLOT).nth(3);
    await fourth.hover();
    await expect(page.locator('.tooltip')).toHaveText('notepad');
    await expect.poll(() => scaleOf(page, 3)).toBeGreaterThan(1.5);
    expect(await scaleOf(page, 2)).toBeGreaterThan(1);
    expect(await scaleOf(page, 2)).toBeLessThan(await scaleOf(page, 3));
    // The magnified box shows the 192 px ladder bitmap, not the 96 px rest one.
    await expect
      .poll(async () => (await iconPairs(page))[3])
      .toMatchObject({ shown: [192], magnifiedAttr: true });
    await page.screenshot({ path: `${OUT}/dock-hover.png` });

    // Leaving resets everything.
    await page.mouse.move(10, 10);
    await expect.poll(() => scaleOf(page, 3)).toBe(1);
    await expect(page.locator('.tooltip')).toHaveCount(0);
    await expect
      .poll(async () => (await iconPairs(page))[3])
      .toMatchObject({ shown: [96], magnifiedAttr: false });
  });

  test('twenty apps', async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await openDock(page, 'mock=many');
    await expect(page.locator(APP_SLOT)).toHaveCount(20);
    await page.screenshot({ path: `${OUT}/dock-many.png` });
  });

  test('reduced motion disables magnification and the item animations', async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'dark', reducedMotion: 'reduce' });
    await openDock(page);
    await page.locator(APP_SLOT).nth(3).hover();
    await expect(page.locator('.tooltip')).toHaveText('notepad');
    expect(await scaleOf(page, 3)).toBe(1);
    // No magnification: the 48 × 2 = 96 px rest bitmap is the only one.
    await expectIconPairs(page, { rest: 96, magnified: null, shown: [96], magnifiedAttr: false });
    await page.screenshot({ path: `${OUT}/dock-reduced-motion.png` });

    // Items come and go at once: no leaving slot ever lingers.
    const gone = await page.evaluate(async (id) => {
      (window as unknown as DemoWindow).__dockMock?.pushWindowRemoved(id);
      await new Promise(requestAnimationFrame);
      return document.querySelectorAll('.slot[data-leaving]').length;
    }, CODE_WINDOW_ID);
    expect(gone).toBe(0);
    await expect(page.locator(APP_SLOT)).toHaveCount(6);
  });

  test('every item is labelled and reachable', async ({ page }) => {
    await openDock(page);
    await expect(page.getByRole('toolbar', { name: 'Stem' })).toBeVisible();
    const buttons = page.locator('[role="button"]');
    const count = await buttons.count();
    expect(count).toBe(9); // Start + 7 apps + Recycle Bin
    for (let i = 0; i < count; i += 1) {
      const button = buttons.nth(i);
      await expect(button).toHaveAttribute('aria-label', /\S/);
      // One tab stop, the first item until the focus has been elsewhere.
      await expect(button).toHaveAttribute('tabindex', i === 0 ? '0' : '-1');
    }
    await expect(page.locator('[role="separator"]')).toHaveCount(3);

    // Keyboard: Tab reaches the first item, Enter activates it (Start → shell action).
    await page.keyboard.press('Tab');
    await expect(buttons.first()).toBeFocused();
    await page.keyboard.press('Enter');
    await expect.poll(() => callsOfType(page, 'shell')).toBe(1);

    // Ctrl+, opens the settings window while the dock has focus.
    await page.keyboard.press('Control+,');
    await expect.poll(() => callsOfType(page, 'openSettings')).toBe(1);
  });

  test.describe('drag to reorder pinned apps', () => {
    const PINNED = '.slot[data-pinned="true"]';
    const pinnedKeys = (page: Page): Promise<string[]> =>
      page.$$eval(PINNED, (els) => els.map((el) => (el as HTMLElement).dataset.key ?? ''));
    const transformOf = (page: Page, index: number): Promise<string> =>
      page
        .locator(PINNED)
        .nth(index)
        .evaluate((el) => (el as HTMLElement).style.transform);

    /**
     * Press on pinned slot `from` and drag to just past the centre of pinned
     * slot `over`; returns where the pointer is then (viewport CSS px).
     */
    async function dragPinned(
      page: Page,
      from: number,
      over: number,
    ): Promise<{ x: number; y: number }> {
      const a = (await page.locator(PINNED).nth(from).boundingBox())!;
      const b = (await page.locator(PINNED).nth(over).boundingBox())!;
      const past = over > from ? 6 : -6;
      await page.mouse.move(a.x + a.width / 2, a.y + a.height / 2);
      await page.mouse.down();
      const to = { x: b.x + b.width / 2 + past, y: a.y + a.height / 2 - 12 };
      await page.mouse.move(to.x, to.y, { steps: 12 });
      return to;
    }

    test('lifts the icon, opens a gap, and drops it in the new order', async ({ page }) => {
      await page.emulateMedia({ colorScheme: 'dark' });
      await openDock(page);
      const before = await pinnedKeys(page);
      expect(before).toHaveLength(5);

      await dragPinned(page, 0, 2);
      const lifted = page.locator(PINNED).nth(0);
      await expect(lifted).toHaveAttribute('data-drag', 'lifted');
      // The two icons between the origin and the gap shifted one slot (48 + 8 px) left.
      await expect.poll(() => transformOf(page, 1)).toBe('translateX(-56px)');
      await expect.poll(() => transformOf(page, 2)).toBe('translateX(-56px)');
      expect(await transformOf(page, 3)).toBe('');
      expect(await scaleOf(page, 1)).toBe(1); // no magnification while dragging
      await expect(page.locator('.tooltip')).toHaveCount(0);
      // While the drag is on, the whole window accepts clicks (docs/ipc/hit-region.md, v1.3).
      const viewport = page.viewportSize()!;
      const whole = { x: 0, y: 0, width: viewport.width, height: viewport.height };
      const atRest = (await hitRegions(page))[0]!;
      await expect.poll(async () => (await hitRegions(page)).at(-1)?.envelope).toEqual(whole);
      expect((await hitRegions(page)).at(-1)!.panel).toEqual(atRest.panel);
      // The window is asked to grow to the work area's height so the
      // lifted icon can be carried anywhere; the width request is unchanged.
      const restSize = (await contentSizes(page))[0]!;
      await expect.poll(async () => (await contentSizes(page)).at(-1)?.height).toBe(4096);
      expect((await contentSizes(page)).at(-1)!.width).toBe(restSize.width);
      await page.screenshot({ path: `${OUT}/dock-drag.png` });

      await page.mouse.up();
      await expect
        .poll(() => pinnedKeys(page))
        .toEqual([before[1], before[2], before[0], before[3], before[4]]);
      // The snapshot rendered the new order and every inline transform is gone.
      await expect.poll(() => transformOf(page, 2)).toBe('');
      await expect(page.locator('[data-drag]')).toHaveCount(0);
      // The release that ended the drag was not a click: nothing launched.
      expect(await callsOfType(page, 'launch')).toBe(0);
      // The real hit region is back once the DOM settled (same geometry as before).
      await expect.poll(async () => (await hitRegions(page)).at(-1)).toEqual(atRest);
      // And so is the window's resting height.
      await expect.poll(async () => (await contentSizes(page)).at(-1)).toEqual(restSize);
    });

    test('Escape puts everything back', async ({ page }) => {
      await openDock(page);
      const before = await pinnedKeys(page);
      const atRest = (await hitRegions(page))[0]!;
      await dragPinned(page, 3, 1);
      await expect.poll(() => transformOf(page, 1)).toBe('translateX(56px)');
      await expect.poll(() => hitRegions(page)).toHaveLength(2);
      await page.keyboard.press('Escape');
      await expect(page.locator('[data-drag]')).toHaveCount(0);
      await expect.poll(() => transformOf(page, 1)).toBe('');
      await page.mouse.up();
      expect(await pinnedKeys(page)).toEqual(before);
      // A cancelled drag restores the real hit region too.
      await expect.poll(async () => (await hitRegions(page)).at(-1)).toEqual(atRest);
    });

    test('releasing far from the dock cancels', async ({ page }) => {
      await openDock(page);
      const before = await pinnedKeys(page);
      await dragPinned(page, 0, 2);
      await page.mouse.move(640, 20, { steps: 6 });
      await page.mouse.up();
      await expect(page.locator('[data-drag]')).toHaveCount(0);
      await expect.poll(() => transformOf(page, 1)).toBe('');
      expect(await pinnedKeys(page)).toEqual(before);
    });

    test('the hit region and the hover state survive the window growing for the drag', async ({
      page,
    }) => {
      await openDock(page);
      const viewport = page.viewportSize()!;
      const before = await pinnedKeys(page);
      const atRest = (await hitRegions(page))[0]!;
      const restSize = (await contentSizes(page))[0]!;
      await expect.poll(async () => (await panelRects(page)).length).toBeGreaterThan(0);
      const restPanel = (await panelRects(page)).at(-1)!;
      const onScreen = await dockPanel(page);
      expect(restPanel).toEqual({ rect: atRest.panel, edge: 'bottom', viewport });
      // Rust grows the window to the work area's height with its bottom
      // edge fixed; the viewport's origin moves up by the growth.
      const GROWTH = 720;
      const shifted = (r: Rect): Rect => ({ ...r, y: r.y + GROWTH });

      const pointer = await dragPinned(page, 0, 2);
      await expect(page.locator(PINNED).nth(0)).toHaveAttribute('data-drag', 'lifted');
      // Reported at drag start, before the window grew: the panel where it
      // is in this viewport, the envelope the whole window.
      await expect
        .poll(async () => (await hitRegions(page)).at(-1))
        .toEqual({
          panel: atRest.panel,
          envelope: { x: 0, y: 0, width: viewport.width, height: viewport.height },
        });
      await expect.poll(async () => (await contentSizes(page)).at(-1)?.height).toBe(4096);

      // The window grows: the panel and the pointer are GROWTH px further
      // down in the new viewport, and the region says so (the panel at its
      // rest y would be far above its real place, and the tracker would
      // turn the window click-through while the icon is lifted).
      await page.setViewportSize({ width: viewport.width, height: viewport.height + GROWTH });
      await expect
        .poll(async () => (await hitRegions(page)).at(-1))
        .toEqual({
          panel: shifted(atRest.panel),
          envelope: { x: 0, y: 0, width: viewport.width, height: viewport.height + GROWTH },
        });
      const grown = (await page.locator('.dock-panel').boundingBox())!;
      expect(Math.floor(grown.y)).toBe(atRest.panel.y + GROWTH);
      // The panel rect goes again with the viewport it is relative to now,
      // and the app layer's conversion puts it where it was.
      const grownViewport = { width: viewport.width, height: viewport.height + GROWTH };
      await expect
        .poll(async () => (await panelRects(page)).at(-1))
        .toEqual({ rect: shifted(atRest.panel), edge: 'bottom', viewport: grownViewport });
      expect(await dockPanel(page)).toEqual(onScreen);
      await page.mouse.move(pointer.x, pointer.y + GROWTH);
      await expect(page.locator(PINNED).nth(0)).toHaveAttribute('data-drag', 'lifted');

      // Drop. The snapshot with the new order arrives before Rust has
      // shrunk the window back, so the measurement that follows it
      // describes the tall viewport: the panel where it is in that one.
      await page.mouse.up();
      await expect
        .poll(() => pinnedKeys(page))
        .toEqual([before[1], before[2], before[0], before[3], before[4]]);
      await expect.poll(async () => (await contentSizes(page)).at(-1)).toEqual(restSize);
      await expect
        .poll(async () => (await hitRegions(page)).at(-1))
        .toEqual({
          panel: shifted(atRest.panel),
          envelope: shifted(atRest.envelope),
        });
      // The pointer is still over the panel and moves a little: hovering.
      await page.mouse.move(pointer.x + 2, pointer.y + GROWTH);
      await expect.poll(() => scaleOf(page, 2)).toBeGreaterThan(1);

      // The window shrinks back. No pointerleave ever comes (a window that
      // turned click-through gets no mouse messages), so the resize itself
      // drops the hover state and measures again: the rest region is back.
      await page.setViewportSize(viewport);
      await expect.poll(async () => (await hitRegions(page)).at(-1)).toEqual(atRest);
      await expect.poll(async () => (await panelRects(page)).at(-1)).toEqual(restPanel);
      expect(await dockPanel(page)).toEqual(onScreen);
      expect(await scaleOf(page, 2)).toBe(1);
      await expect(page.locator('.tooltip')).toHaveCount(0);
      const reports = (await hitRegions(page)).length;

      // An item change after that, with the pointer never having left the
      // panel, is still reported once its animation has ended.
      await page.evaluate((raw) => {
        (window as unknown as DemoWindow).__dockMock?.pushWindow(raw as RawWindow);
      }, PAINT_WINDOW);
      await expect(page.locator(PAINT_SLOT)).toHaveCount(1);
      await expect.poll(() => hitRegions(page)).toHaveLength(reports + 1);
      const widened = (await hitRegions(page)).at(-1)!;
      expect(widened.panel.width).toBe(atRest.panel.width + ICON + SLOT_GAP);
      expect(widened.panel.y).toBe(atRest.panel.y);
    });

    test('running, unpinned apps and fixed items cannot be dragged', async ({ page }) => {
      await openDock(page);
      const before = await pinnedKeys(page);
      for (const index of [0, 6]) {
        // Start (fixed) and Code (running, not pinned).
        const box = (await page.locator('[role="button"]').nth(index).boundingBox())!;
        await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
        await page.mouse.down();
        await page.mouse.move(box.x + box.width / 2 + 120, box.y + box.height / 2, { steps: 8 });
        await page.waitForTimeout(200);
        await expect(page.locator('[data-drag]')).toHaveCount(0);
        await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2, { steps: 4 });
        await page.mouse.up();
      }
      expect(await pinnedKeys(page)).toEqual(before);
    });
  });

  test('the running indicator dots reflect the window count', async ({ page }) => {
    await openDock(page);
    const dots = (index: number) => page.locator(APP_SLOT).nth(index).locator('.dots i');
    await expect(dots(0)).toHaveCount(1); // explorer, one window
    await expect(dots(1)).toHaveCount(2); // firefox, two windows
    await expect(dots(3)).toHaveCount(0); // notepad, pinned only
    await expect(page.locator(APP_SLOT).nth(6).locator('.dots')).toHaveClass(/dim/); // Spotify minimised
  });

  test('a failed command shows a toast that goes away by itself', async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await openDock(page);
    // Every icon request must be in before the failure is armed: the cache
    // issues the magnified bitmaps a few at a time behind the rest ones
    // (`openDock` waits only for the latter), and a `get_icon` that took the
    // failure would retry quietly instead of showing a toast.
    await expectIconPairs(page, { rest: 96, magnified: 192, shown: [96], magnifiedAttr: false });
    // Hover first: entering the panel sends `set_dock_hover`, which must not
    // be the command that fails.
    const code = page.locator(APP_SLOT).nth(5);
    await code.hover();
    await page.evaluate(() =>
      (window as unknown as DemoWindow).__dockMock?.failNextCommand('window not found: 4'),
    );
    await code.click();
    const toast = page.getByRole('status');
    await expect(toast).toHaveText('Code failed: window not found: 4');
    await page.screenshot({ path: `${OUT}/dock-toast.png` });
    // Auto-dismiss after 4 s; it never captures the pointer.
    await expect(toast).toHaveCSS('pointer-events', 'none');
    await expect(toast).toHaveCount(0, { timeout: 6_000 });
  });

  test('a right-click asks the backend for a native menu', async ({ page }) => {
    await openDock(page);
    // An item: the request names it and carries the pointer position in CSS px.
    const firefox = page.locator(APP_SLOT).nth(1);
    const slot = (await firefox.boundingBox())!;
    await firefox.click({ button: 'right' });
    await expect.poll(() => callsOfType(page, 'showItemMenu')).toBe(1);
    const request = await page.evaluate(() =>
      ((window as unknown as DemoWindow).__dockMock?.calls ?? []).find(
        (c) => c.type === 'showItemMenu',
      ),
    );
    expect(request).toMatchObject({
      itemKey: 'app:exe:c:/program files/mozilla firefox/firefox.exe',
    });
    expect(request?.x).toBeGreaterThan(slot.x);
    expect(request?.x).toBeLessThan(slot.x + slot.width);
    expect(request?.y).toBeGreaterThan(slot.y);
    expect(request?.y).toBeLessThan(slot.y + slot.height);

    // The panel's own padding (no item under the pointer): the dock menu.
    const panel = (await page.locator('.dock-panel').boundingBox())!;
    await page.mouse.click(panel.x + 3, panel.y + panel.height / 2, { button: 'right' });
    await expect.poll(() => callsOfType(page, 'showDockMenu')).toBe(1);

    // A separator: no menu at all.
    await page.locator('[role="separator"]').first().click({ button: 'right' });
    await page.waitForTimeout(150);
    expect(await callsOfType(page, 'showItemMenu')).toBe(1);
    expect(await callsOfType(page, 'showDockMenu')).toBe(1);

    // The transparent stage outside the panel takes clicks during a drag and
    // when click-through is off: never the browser's own menu there.
    // dispatchEvent returns false when a listener called preventDefault.
    const browserMenuAllowed = await page.evaluate(() =>
      document.querySelector('.dock-stage')!.dispatchEvent(
        new MouseEvent('contextmenu', {
          bubbles: true,
          cancelable: true,
          clientX: 10,
          clientY: 10,
        }),
      ),
    );
    expect(browserMenuAllowed).toBe(false);
    expect(await callsOfType(page, 'showDockMenu')).toBe(1);
  });

  test('clicking the active app minimises it; the mock reports window sizing', async ({ page }) => {
    await openDock(page);
    const firefox = page.locator(APP_SLOT).nth(1);
    await expect(firefox).toHaveAttribute('data-active', 'true');
    // Firefox has two windows: the click cycles to the second one.
    await firefox.click();
    await expect(firefox).toHaveAttribute('data-active', 'true');
    const code = page.locator(APP_SLOT).nth(5);
    await code.click();
    await expect(code).toHaveAttribute('data-active', 'true');
    await expect(firefox).not.toHaveAttribute('data-active', 'true');

    const sizes = await page.evaluate(() =>
      ((window as unknown as DemoWindow).__dockMock?.calls ?? []).filter(
        (c) => c.type === 'setDockContentSize',
      ),
    );
    expect(sizes.length).toBeGreaterThanOrEqual(1);
  });
});

test.describe('theme sync', () => {
  const PANEL = '.dock-panel';

  test('follows the accent, light/dark and transparency of Windows live; the theme setting overrides light/dark', async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await openDock(page);
    const html = page.locator('html');
    const panel = page.locator(PANEL);
    const firstDot = page.locator(`${APP_SLOT} .dots i`).first();
    const startTile = page.locator('.slot[data-kind="launcher"] svg rect').first();
    await expect(html).toHaveAttribute('data-theme', 'dark');
    await expect(firstDot).toHaveCSS('background-color', 'rgb(0, 120, 212)');
    await expect(panel).toHaveCSS('background-color', 'rgba(28, 28, 30, 0.72)');

    // Windows switches to light mode and an orange accent: the dock follows at once.
    await setOsTheme(page, { dark: false, accent: '#FF8C00' });
    await expect(html).toHaveAttribute('data-theme', 'light');
    await expect(firstDot).toHaveCSS('background-color', 'rgb(255, 140, 0)');
    expect(await computed(startTile, 'fill')).toBe('rgb(255, 140, 0)');
    await expect(panel).toHaveCSS('background-color', 'rgba(244, 244, 246, 0.72)');

    // Transparency effects off: the panel is (nearly) opaque, whatever the opacity setting.
    await setOsTheme(page, { dark: false, accent: '#FF8C00', transparency: false });
    await expect(panel).toHaveCSS('background-color', 'rgba(244, 244, 246, 0.95)');
    await page.screenshot({ path: `${OUT}/dock-light-accent-opaque.png` });

    // The theme setting wins over Windows' mode, both ways; "system" follows it again.
    await patchSettings(page, { theme: 'dark' });
    await expect(html).toHaveAttribute('data-theme', 'dark');
    await patchSettings(page, { theme: 'system' });
    await expect(html).toHaveAttribute('data-theme', 'light');
    await setOsTheme(page, { dark: true });
    await expect(html).toHaveAttribute('data-theme', 'dark');
    await patchSettings(page, { theme: 'light' });
    await expect(html).toHaveAttribute('data-theme', 'light');
  });

  test('a contrast theme: an opaque, unblurred panel; solid lines, separators and tooltips', async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await openDock(page);
    const html = page.locator('html');
    const panel = page.locator(PANEL);
    await setOsTheme(page, { highContrast: true });
    await expect(html).toHaveAttribute('data-high-contrast', '');
    await expect(panel).toHaveCSS('background-color', 'rgb(28, 28, 30)');
    await expect(panel).toHaveCSS('backdrop-filter', 'none');
    await expect(panel).toHaveCSS('box-shadow', 'none');
    expect(await computed(panel, 'border-top-width', '::before')).toBe('2px');
    expect(await computed(panel, 'border-top-color', '::before')).toBe('rgb(255, 255, 255)');
    await expect(page.locator('[role="separator"]').first()).toHaveCSS(
      'background-color',
      'rgb(255, 255, 255)',
    );
    await page.locator(APP_SLOT).nth(3).hover();
    const tooltip = page.locator('.tooltip');
    await expect(tooltip).toHaveText('notepad');
    await expect(tooltip).toHaveCSS('background-color', 'rgb(28, 28, 30)');
    await expect(tooltip).toHaveCSS('border-top-color', 'rgb(255, 255, 255)');
    await page.screenshot({ path: `${OUT}/dock-high-contrast.png` });

    // A light contrast theme: black lines on the light panel.
    await setOsTheme(page, { highContrast: true, dark: false });
    await expect(html).toHaveAttribute('data-theme', 'light');
    await expect(panel).toHaveCSS('background-color', 'rgb(244, 244, 246)');
    await expect(tooltip).toHaveCSS('border-top-color', 'rgb(0, 0, 0)');

    // The contrast theme is turned off: translucent again.
    await page.mouse.move(640, 10);
    await setOsTheme(page, {});
    await expect(html).not.toHaveAttribute('data-high-contrast');
    await expect(panel).toHaveCSS('background-color', 'rgba(28, 28, 30, 0.72)');
    await expect(panel).not.toHaveCSS('backdrop-filter', 'none');
  });

  test('in forced colours (a contrast theme in WebView2) the dots, separators and tooltip arrow stay visible', async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme: 'dark', forcedColors: 'active' });
    await openDock(page);
    await setOsTheme(page, { highContrast: true });
    const panel = page.locator(PANEL);
    await expect(panel).toHaveCSS('backdrop-filter', 'none');
    // The browser paints backgrounds in the theme's Canvas colour; these opt
    // out, or they would vanish into the panel.
    const canvas = await computed(panel, 'background-color');
    const dot = page.locator(`${APP_SLOT} .dots i`);
    expect(await computed(dot.first(), 'background-color')).not.toBe(canvas);
    const activeDot = page.locator(`${APP_SLOT}[data-active] .dots i`).first();
    expect(await computed(activeDot, 'background-color')).not.toBe(canvas);
    expect(await computed(activeDot, 'background-color')).not.toBe(
      await computed(dot.first(), 'background-color'),
    );
    expect(await computed(page.locator('[role="separator"]'), 'background-color')).not.toBe(canvas);
    await page.locator(APP_SLOT).nth(3).hover();
    const tooltip = page.locator('.tooltip');
    await expect(tooltip).toHaveText('notepad');
    // The arrow is one coloured border, not a block of four.
    expect(await computed(tooltip, 'border-left-color', '::after')).toBe('rgba(0, 0, 0, 0)');
    expect(await computed(tooltip, 'border-top-color', '::after')).not.toBe('rgba(0, 0, 0, 0)');
    // The attention pill too.
    await flash(page, 1);
    const pill = page.locator(`${APP_SLOT}[data-attention] .dots i`);
    await expect(pill).toHaveCount(1);
    expect(await computed(pill, 'background-color')).not.toBe(canvas);
    await page.screenshot({ path: `${OUT}/dock-forced-colors.png`, animations: 'disabled' });
  });
});

test.describe('Start, Task View and Show Desktop', () => {
  const KINDS = [
    { kind: 'launcher', key: 'fixed:launcher', title: 'Start', action: 'openStartMenu' },
    { kind: 'taskView', key: 'fixed:task-view', title: 'Task View', action: 'openTaskView' },
    {
      kind: 'showDesktop',
      key: 'fixed:show-desktop',
      title: 'Show Desktop',
      action: 'toggleShowDesktop',
    },
  ] as const;
  const LEFT = KINDS.map(({ kind }) => `.slot[data-kind="${kind}"]`).join(', ');

  /** The keys of the rendered slots (items and separators), left to right. */
  function keys(page: Page): Promise<string[]> {
    return page
      .locator('.dock-panel > .slot:not([data-leaving])')
      .evaluateAll((els) => els.map((el) => (el as HTMLElement).dataset.key ?? ''));
  }

  /** What the mock recorded for one call type, oldest first. */
  function recorded(page: Page, type: string, field: string): Promise<unknown[]> {
    return page.evaluate(
      ([t, f]) =>
        ((window as unknown as DemoWindow).__dockMock?.calls ?? [])
          .filter((c) => c.type === t)
          .map((c) => c[f]),
      [type, field] as const,
    );
  }

  test('come and go with their settings, live, each with its icon, click and menu', async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await openDock(page);
    // The defaults: Start only, then a separator.
    expect((await keys(page)).slice(0, 2)).toEqual(['fixed:launcher', 'sep:1']);
    await expect(page.locator(LEFT)).toHaveCount(1);

    // Task View and Show Desktop are switched on (the settings window does
    // this through `update_settings`): they grow in after Start.
    await patchSettings(page, { items: { taskView: true, showDesktop: true } });
    await expect
      .poll(async () => (await keys(page)).slice(0, 4))
      .toEqual(['fixed:launcher', 'fixed:task-view', 'fixed:show-desktop', 'sep:1']);
    for (const { kind, title } of KINDS) {
      const slot = page.locator(`.slot[data-kind="${kind}"]`);
      await expect(slot).toHaveAttribute('aria-label', title);
      await expect(slot).toHaveAttribute('data-icon', 'shell');
      // Start is the toolbar's one tab stop; the others are reached by arrow.
      await expect(slot).toHaveAttribute('tabindex', kind === 'launcher' ? '0' : '-1');
      // The built-in vector art fills the icon box.
      await expect.poll(async () => (await slot.locator('svg').boundingBox())?.width).toBe(ICON);
    }
    await expect.poll(() => runningAnimations(page)).toBe(0);
    await page.screenshot({ path: `${OUT}/dock-fixed-items.png` });

    // A click on each asks the backend for its primary action; the engine
    // maps the item to its shell action. One at a time: the mock's commands
    // settle after a random latency, so their order is only that of the clicks
    // when each has landed before the next.
    for (const [i, { kind }] of KINDS.entries()) {
      await page.locator(`.slot[data-kind="${kind}"]`).click();
      await expect
        .poll(() => recorded(page, 'shell', 'action'))
        .toEqual(KINDS.slice(0, i + 1).map(({ action }) => action));
    }
    // A right-click on each asks for its native menu (built by Rust).
    for (const [i, { kind }] of KINDS.entries()) {
      await page.locator(`.slot[data-kind="${kind}"]`).click({ button: 'right' });
      await expect
        .poll(() => recorded(page, 'showItemMenu', 'itemKey'))
        .toEqual(KINDS.slice(0, i + 1).map(({ key }) => key));
    }

    // All three off, live: the left group and its separator go.
    await page.mouse.move(640, 10);
    await patchSettings(page, { items: { launcher: false, taskView: false, showDesktop: false } });
    await expect(page.locator(LEFT)).toHaveCount(0);
    await expect.poll(async () => (await keys(page))[0]).toMatch(/^app:/);
    await expect(page.locator('[role="separator"]')).toHaveCount(2);
  });
});

test.describe('Quick Settings and Notification Center', () => {
  const KINDS = [
    {
      kind: 'quickSettings',
      key: 'fixed:quick-settings',
      title: 'Quick Settings',
      action: 'openQuickSettings',
    },
    {
      kind: 'notificationCenter',
      key: 'fixed:notification-center',
      title: 'Notification Center',
      action: 'openNotificationCenter',
    },
  ] as const;
  const BOTH = KINDS.map(({ kind }) => `.slot[data-kind="${kind}"]`).join(', ');
  /** The dock's last slots with both on: one separator, the two, the bin. */
  const TRAILING = ['sep:3', ...KINDS.map(({ key }) => key), 'fixed:recycle-bin'];

  /** The keys of the rendered slots (items and separators), in dock order. */
  function keys(page: Page): Promise<string[]> {
    return page
      .locator('.dock-panel > .slot:not([data-leaving])')
      .evaluateAll((els) => els.map((el) => (el as HTMLElement).dataset.key ?? ''));
  }

  /** What the mock recorded for one call type, oldest first. */
  function recorded(page: Page, type: string, field: string): Promise<unknown[]> {
    return page.evaluate(
      ([t, f]) =>
        ((window as unknown as DemoWindow).__dockMock?.calls ?? [])
          .filter((c) => c.type === t)
          .map((c) => c[f]),
      [type, field] as const,
    );
  }

  /** The focused item's key, or `null` when the focus is not on an item. */
  function focusedKey(page: Page): Promise<string | null> {
    return page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.key ?? null);
  }

  /** Both on, as the settings window would; wait until they are drawn and still. */
  async function switchOn(page: Page): Promise<void> {
    await patchSettings(page, { items: { quickSettings: true, notificationCenter: true } });
    await expect.poll(async () => (await keys(page)).slice(-4)).toEqual(TRAILING);
    await expect.poll(() => runningAnimations(page)).toBe(0);
  }

  test('are off by default; switched on they sit before the Recycle Bin with their icons, clicks, Enter and menus', async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await openDock(page);
    await expect(page.locator(BOTH)).toHaveCount(0);
    expect((await keys(page)).slice(-2)).toEqual(['sep:3', 'fixed:recycle-bin']);

    await switchOn(page);
    for (const { kind, title } of KINDS) {
      const slot = page.locator(`.slot[data-kind="${kind}"]`);
      await expect(slot).toHaveAttribute('aria-label', title);
      await expect(slot).toHaveAttribute('data-icon', 'shell');
      // Start stays the toolbar's one tab stop; these are reached by arrow.
      await expect(slot).toHaveAttribute('tabindex', '-1');
      // The page's own vector art, filling the icon box.
      await expect.poll(async () => (await slot.locator('svg').boundingBox())?.width).toBe(ICON);
      await expect(slot.locator('svg path, svg rect, svg circle').first()).toBeAttached();
    }
    // The two are drawn differently from each other and from the fallback tile.
    const art = await page
      .locator(BOTH)
      .evaluateAll((els) => els.map((el) => el.querySelector('svg')!.innerHTML));
    expect(art[0]).not.toBe(art[1]);
    await page.screenshot({ path: `${OUT}/dock-quick-settings-dark.png` });

    // A click on each asks for its primary action; the engine maps the item
    // to its shell action. One at a time (the mock's latency is random).
    for (const [i, { kind }] of KINDS.entries()) {
      await page.locator(`.slot[data-kind="${kind}"]`).click();
      await expect
        .poll(() => recorded(page, 'shell', 'action'))
        .toEqual(KINDS.slice(0, i + 1).map(({ action }) => action));
    }

    // Enter on each from the keyboard does the same: End is the bin, the
    // arrow before it Notification Center, then Quick Settings.
    await page.mouse.move(640, 10);
    await page.keyboard.press('Tab');
    await page.keyboard.press('End');
    expect(await focusedKey(page)).toBe('fixed:recycle-bin');
    await page.keyboard.press('ArrowLeft');
    expect(await focusedKey(page)).toBe('fixed:notification-center');
    await page.keyboard.press('Enter');
    await expect
      .poll(async () => (await recorded(page, 'shell', 'action')).slice(2))
      .toEqual(['openNotificationCenter']);
    await page.keyboard.press('ArrowLeft');
    expect(await focusedKey(page)).toBe('fixed:quick-settings');
    await page.keyboard.press('Enter');
    await expect
      .poll(async () => (await recorded(page, 'shell', 'action')).slice(2))
      .toEqual(['openNotificationCenter', 'openQuickSettings']);
    await page.keyboard.press('Tab');

    // A right-click on each asks for its native menu (built by Rust).
    for (const [i, { kind }] of KINDS.entries()) {
      await page.locator(`.slot[data-kind="${kind}"]`).click({ button: 'right' });
      await expect
        .poll(() => recorded(page, 'showItemMenu', 'itemKey'))
        .toEqual(KINDS.slice(0, i + 1).map(({ key }) => key));
    }

    // Both off, live: they go, and the separator before the bin stays.
    await page.mouse.move(640, 10);
    await patchSettings(page, { items: { quickSettings: false, notificationCenter: false } });
    await expect(page.locator(BOTH)).toHaveCount(0);
    await expect
      .poll(async () => (await keys(page)).slice(-2))
      .toEqual(['sep:3', 'fixed:recycle-bin']);
  });

  test('in the light theme', async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'light' });
    await openDock(page);
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
    await switchOn(page);
    await page.screenshot({ path: `${OUT}/dock-quick-settings-light.png` });
  });

  test('on a left dock: in the column just above the Recycle Bin', async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await page.setViewportSize({ width: 360, height: 900 });
    await openDock(page, 'mock=1&position=left');
    await switchOn(page);
    const order = await page.$$eval('.dock-panel > .slot:not([data-leaving])', (els) =>
      els.map((el) => {
        const r = el.getBoundingClientRect();
        return {
          key: (el as HTMLElement).dataset.key ?? '',
          x: r.left,
          y: r.top,
          bottom: r.bottom,
        };
      }),
    );
    const tail = order.slice(-4);
    expect(tail.map((s) => s.key)).toEqual(TRAILING);
    // One column, top to bottom, every slot inside the window.
    for (let i = 1; i < order.length; i += 1) {
      expect(order[i]!.y).toBeGreaterThan(order[i - 1]!.y);
    }
    expect(order.at(-1)!.bottom).toBeLessThanOrEqual(900);
    const xs = new Set(
      await page
        .locator(BOTH)
        .evaluateAll((els) => els.map((el) => Math.round(el.getBoundingClientRect().left))),
    );
    expect(xs.size).toBe(1);
    await page.screenshot({ path: `${OUT}/dock-quick-settings-left.png` });
  });
});

test.describe('clock and battery', () => {
  // A fixed place and time: Sunday 15 November 2026, 10:42 in Berlin, read
  // in US English, so the faces and names are the same on every host.
  test.use({ locale: 'en-US', timezoneId: 'Europe/Berlin' });
  const NOW = '2026-11-15T10:42:00+01:00';
  const CLOCK = '.slot[data-kind="clock"]';
  const BATTERY = '.slot[data-kind="battery"]';
  /** The dock's last slots with both on: one separator, battery, clock, the bin. */
  const TRAILING = ['sep:3', 'fixed:battery', 'fixed:clock', 'fixed:recycle-bin'];
  /** The full date and time, as `Intl` writes it (a narrow no-break space before AM is `\s`). */
  const FULL = /^Sunday, November 15, 2026 at 10:42\sAM$/;
  /** The demo's laptop (`DEMO_POWER` in src/lib/ipc/mock.ts). */
  const ON_BATTERY: PowerStatus = {
    hasBattery: true,
    percent: 76,
    charging: false,
    onAc: false,
    saver: false,
  };
  const CHARGING: PowerStatus = { ...ON_BATTERY, percent: 64, charging: true, onAc: true };
  const SAVER: PowerStatus = { ...ON_BATTERY, percent: 18, saver: true };

  /** The keys of the rendered slots (items and separators), in dock order. */
  function keys(page: Page): Promise<string[]> {
    return page
      .locator('.dock-panel > .slot:not([data-leaving])')
      .evaluateAll((els) => els.map((el) => (el as HTMLElement).dataset.key ?? ''));
  }

  /** What the mock recorded for one call type, oldest first. */
  function recorded(page: Page, type: string, field: string): Promise<unknown[]> {
    return page.evaluate(
      ([t, f]) =>
        ((window as unknown as DemoWindow).__dockMock?.calls ?? [])
          .filter((c) => c.type === t)
          .map((c) => c[f]),
      [type, field] as const,
    );
  }

  /** A new power reading, as the events thread reports it. */
  async function setPower(page: Page, status: PowerStatus): Promise<void> {
    await page.evaluate((s) => (window as unknown as DemoWindow).__dockMock!.setPower(s), status);
  }

  /** The battery face's fill width in face units (of 32), or `null` without a fill. */
  function fillWidth(page: Page): Promise<number | null> {
    return page
      .locator(`${BATTERY} svg`)
      .evaluate((svg) => Number(svg.querySelector('rect.fill')?.getAttribute('width') ?? NaN))
      .then((w) => (Number.isNaN(w) ? null : w));
  }

  /** Both on, as the settings window would; wait until they are drawn and still. */
  async function switchOn(page: Page): Promise<void> {
    await patchSettings(page, { items: { battery: true, clock: true } });
    await expect.poll(async () => (await keys(page)).slice(-4)).toEqual(TRAILING);
    await expect.poll(() => runningAnimations(page)).toBe(0);
  }

  test('are off by default; on, they sit before the Recycle Bin with their faces, names, clicks and menus', async ({
    page,
  }) => {
    await page.clock.setFixedTime(NOW);
    await page.emulateMedia({ colorScheme: 'dark' });
    await openDock(page);
    await expect(page.locator(`${CLOCK}, ${BATTERY}`)).toHaveCount(0);
    expect((await keys(page)).slice(-2)).toEqual(['sep:3', 'fixed:recycle-bin']);

    await switchOn(page);
    const clock = page.locator(CLOCK);
    const battery = page.locator(BATTERY);
    // The clock: its name is the full date and time; the face the time over
    // the short weekday and day.
    await expect(clock).toHaveAttribute('aria-label', FULL);
    await expect(clock).toHaveAttribute('data-icon', 'shell');
    await expect(clock.locator('text.time')).toHaveText(/^10:42\sAM$/);
    await expect(clock.locator('text.date')).toHaveText('Sun 15');
    // The battery: Rust's title; the face fills to the percent and writes it.
    await expect(battery).toHaveAttribute('aria-label', 'Battery 76 %');
    await expect(battery.locator('text.percent')).toHaveText('76%');
    expect(await fillWidth(page)).toBeCloseTo(32 * 0.76, 5);
    await expect(battery.locator('.glyph')).toHaveCount(0);
    await expect(battery.locator('rect.fill.saver')).toHaveCount(0);
    for (const slot of [clock, battery]) {
      // Start stays the toolbar's one tab stop; these are reached by arrow.
      await expect(slot).toHaveAttribute('tabindex', '-1');
      await expect.poll(async () => (await slot.locator('svg').boundingBox())?.width).toBe(ICON);
    }
    await page.screenshot({ path: `${OUT}/dock-clock-battery-dark.png` });

    // The clock's title pill is the full date and time too.
    await clock.hover();
    await expect(page.locator('.tooltip')).toHaveText(FULL);
    await battery.hover();
    await expect(page.locator('.tooltip')).toHaveText('Battery 76 %');
    await page.mouse.move(640, 10);
    await expect(page.locator('.tooltip')).toHaveCount(0);

    // A click on each runs its panel: the clock Notification Center (with
    // the calendar), the battery Quick Settings. One at a time.
    await clock.click();
    await expect.poll(() => recorded(page, 'shell', 'action')).toEqual(['openNotificationCenter']);
    await battery.click();
    await expect
      .poll(() => recorded(page, 'shell', 'action'))
      .toEqual(['openNotificationCenter', 'openQuickSettings']);
    // A right-click asks Rust for the item's menu.
    await clock.click({ button: 'right' });
    await battery.click({ button: 'right' });
    await expect
      .poll(() => recorded(page, 'showItemMenu', 'itemKey'))
      .toEqual(['fixed:clock', 'fixed:battery']);
    await page.mouse.move(640, 10);

    // Charging: a bolt, the new percent, the title says so.
    await setPower(page, CHARGING);
    await expect(battery).toHaveAttribute('aria-label', 'Battery 64 %, charging');
    await expect(battery.locator('text.percent')).toHaveText('64%');
    await expect(battery.locator('path.glyph')).toHaveCount(1);
    expect(await fillWidth(page)).toBeCloseTo(32 * 0.64, 5);
    await expect.poll(() => runningAnimations(page)).toBe(0);
    await page.screenshot({ path: `${OUT}/dock-clock-battery-charging-dark.png` });
    const bolt = await battery.locator('path.glyph').getAttribute('d');

    // Plugged in at 100 % without charging: a plug instead of the bolt.
    await setPower(page, { ...CHARGING, percent: 100, charging: false });
    await expect(battery).toHaveAttribute('aria-label', 'Battery 100 %, plugged in');
    await expect(battery.locator('path.glyph')).toHaveCount(1);
    expect(await battery.locator('path.glyph').getAttribute('d')).not.toBe(bolt);
    expect(await fillWidth(page)).toBeCloseTo(32, 5);

    // Battery saver: the amber fill.
    await setPower(page, SAVER);
    await expect(battery).toHaveAttribute('aria-label', 'Battery 18 %, battery saver on');
    await expect(battery.locator('rect.fill.saver')).toHaveCount(1);
    await expect(battery.locator('.glyph')).toHaveCount(0);
    await page.screenshot({ path: `${OUT}/dock-clock-battery-saver-dark.png` });

    // An unknown charge: no fill, a dash, the title without a percent.
    await setPower(page, { ...ON_BATTERY, percent: null });
    await expect(battery).toHaveAttribute('aria-label', 'Battery');
    await expect(battery.locator('text.percent')).toHaveText('–');
    expect(await fillWidth(page)).toBeNull();

    // A desktop PC: no battery, so no battery item; the clock stays.
    await setPower(page, { ...ON_BATTERY, hasBattery: false, percent: null, onAc: true });
    await expect(battery).toHaveCount(0);
    await expect
      .poll(async () => (await keys(page)).slice(-3))
      .toEqual(['sep:3', 'fixed:clock', 'fixed:recycle-bin']);

    // Both off, live: they go, the separator before the bin stays.
    await patchSettings(page, { items: { battery: false, clock: false } });
    await expect(page.locator(`${CLOCK}, ${BATTERY}`)).toHaveCount(0);
    await expect
      .poll(async () => (await keys(page)).slice(-2))
      .toEqual(['sep:3', 'fixed:recycle-bin']);
  });

  test('in the light theme, charging and with battery saver', async ({ page }) => {
    await page.clock.setFixedTime(NOW);
    await page.emulateMedia({ colorScheme: 'light' });
    await openDock(page);
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
    await switchOn(page);
    await setPower(page, CHARGING);
    await expect(page.locator(BATTERY)).toHaveAttribute('aria-label', 'Battery 64 %, charging');
    await page.screenshot({ path: `${OUT}/dock-clock-battery-charging-light.png` });
    await setPower(page, SAVER);
    await expect(page.locator(`${BATTERY} rect.fill.saver`)).toHaveCount(1);
    await page.screenshot({ path: `${OUT}/dock-clock-battery-saver-light.png` });
  });

  for (const scheme of ['dark', 'light'] as const) {
    test(`on a left dock (${scheme}): in the column just above the Recycle Bin`, async ({
      page,
    }) => {
      await page.clock.setFixedTime(NOW);
      await page.emulateMedia({ colorScheme: scheme });
      await page.setViewportSize({ width: 360, height: 900 });
      await openDock(page, 'mock=1&position=left');
      await switchOn(page);
      await setPower(page, scheme === 'dark' ? CHARGING : SAVER);
      const order = await page.$$eval('.dock-panel > .slot:not([data-leaving])', (els) =>
        els.map((el) => {
          const r = el.getBoundingClientRect();
          return {
            key: (el as HTMLElement).dataset.key ?? '',
            x: r.left,
            y: r.top,
            bottom: r.bottom,
          };
        }),
      );
      expect(order.slice(-4).map((s) => s.key)).toEqual(TRAILING);
      for (let i = 1; i < order.length; i += 1) {
        expect(order[i]!.y).toBeGreaterThan(order[i - 1]!.y);
      }
      expect(order.at(-1)!.bottom).toBeLessThanOrEqual(900);
      const xs = new Set(
        await page
          .locator(`${CLOCK}, ${BATTERY}`)
          .evaluateAll((els) => els.map((el) => Math.round(el.getBoundingClientRect().left))),
      );
      expect(xs.size).toBe(1);
      await expect(page.locator(`${CLOCK} text.time`)).toHaveText(/^10:42\sAM$/);
      await expect.poll(() => runningAnimations(page)).toBe(0);
      await page.screenshot({ path: `${OUT}/dock-clock-battery-left-${scheme}.png` });
    });
  }

  test('the clock turns over at the minute and catches up when the page is shown again', async ({
    page,
  }) => {
    // Time flows from a minute before, so the page loads normally; then it
    // is paused just before 10:43 and moved by hand.
    await page.clock.install({ time: '2026-11-15T10:41:00+01:00' });
    await page.emulateMedia({ colorScheme: 'dark' });
    await openDock(page);
    await patchSettings(page, { items: { clock: true } });
    const clock = page.locator(CLOCK);
    await expect(clock).toBeVisible();
    await page.clock.pauseAt('2026-11-15T10:42:59+01:00');
    await expect(clock.locator('text.time')).toHaveText(/^10:42\sAM$/);
    await expect(clock).toHaveAttribute('aria-label', FULL);

    // The minute boundary: the one timer fires and the face moves on.
    await page.clock.runFor(1_500);
    await expect(clock.locator('text.time')).toHaveText(/^10:43\sAM$/);
    await expect(clock).toHaveAttribute('aria-label', /10:43\sAM$/);
    await page.clock.runFor(60_000);
    await expect(clock.locator('text.time')).toHaveText(/^10:44\sAM$/);

    // A jump the timers did not see (the machine slept): nothing changes
    // until the page is shown again, then the face is right at once.
    await page.clock.setSystemTime('2026-11-15T13:07:20+01:00');
    await expect(clock.locator('text.time')).toHaveText(/^10:44\sAM$/);
    await page.evaluate(() => document.dispatchEvent(new Event('visibilitychange')));
    await expect(clock.locator('text.time')).toHaveText(/^1:07\sPM$/);
    await expect(clock).toHaveAttribute('aria-label', /at 1:07\sPM$/);
    // And from there it keeps to the minute.
    await page.clock.runFor(40_000);
    await expect(clock.locator('text.time')).toHaveText(/^1:08\sPM$/);
  });
});

test.describe('attention', () => {
  const EXPLORER = `${APP_SLOT}[data-key="app:exe:c:/windows/explorer.exe"]`;
  const EXPLORER_WINDOW = 1;
  const SPOTIFY = `${APP_SLOT}[data-key="app:exe:c:/program files/spotify/spotify.exe"]`;
  /** Spotify's only window, minimised in the demo. */
  const SPOTIFY_WINDOW = 6;
  /** `--attention` of the dark theme (src/app.css). */
  const ATTENTION_DARK = 'rgb(247, 99, 12)';

  /** The name of the running animation on an item's wiggler, or `null`. */
  function wiggle(page: Page, slot: string): Promise<string | null> {
    return page
      .locator(slot)
      .locator('.wiggler')
      .evaluate((el) => {
        const running = el.getAnimations().find((a) => a.playState === 'running');
        return running instanceof CSSAnimation ? running.animationName : null;
      });
  }

  test('an app that flashes wiggles until its window is activated; nothing runs before or after', async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await openDock(page);
    const explorer = page.locator(EXPLORER);
    // At rest nothing animates (docs/development/frontend-rules.md: the dock idles at ~0 % CPU).
    await expect.poll(() => runningAnimations(page)).toBe(0);
    await expect(page.locator('[data-attention]')).toHaveCount(0);

    await flash(page, EXPLORER_WINDOW);
    await expect(explorer).toHaveAttribute('data-attention', 'true');
    await expect(explorer).toHaveAttribute('aria-label', /needs attention/);
    await expect(explorer.locator('.wiggler')).toHaveClass(/attention/);
    await expect.poll(() => wiggle(page, EXPLORER)).toMatch(/wiggle/);
    // That one animation and nothing else.
    expect(await runningAnimations(page)).toBe(1);
    // The still cue: one pill, three dots wide, in the attention colour.
    await expect(explorer.locator('.dots')).toHaveClass(/attention/);
    await expect(explorer.locator('.dots i')).toHaveCount(1);
    await expect(explorer.locator('.dots i')).toHaveCSS('background-color', ATTENTION_DARK);
    await expect(explorer.locator('.dots i')).toHaveCSS('width', '12px');
    await page.screenshot({ path: `${OUT}/dock-attention.png`, animations: 'disabled' });

    // It keeps going (past one 2 s cycle) until the window is activated…
    await page.waitForTimeout(2_500);
    expect(await wiggle(page, EXPLORER)).toMatch(/wiggle/);
    // …here by a click on the item: the engine activates the window and the
    // next snapshot clears the flag.
    await explorer.click();
    await expect(explorer).toHaveAttribute('data-active', 'true');
    await expect(explorer).not.toHaveAttribute('data-attention');
    await expect(explorer.locator('.wiggler')).not.toHaveClass(/attention/);
    await expect(explorer.locator('.dots')).not.toHaveClass(/attention/);
    await expect(explorer.locator('.dots i')).toHaveCSS('width', '4px');
    await page.mouse.move(640, 10);
    await expect.poll(() => runningAnimations(page)).toBe(0);

    // A minimised app flashing in the background, then switched to without
    // the dock (Alt+Tab): the pill is not dimmed like its dot was, and it
    // stops too.
    const spotify = page.locator(SPOTIFY);
    await flash(page, SPOTIFY_WINDOW);
    await expect(spotify).toHaveAttribute('data-attention', 'true');
    await expect(spotify.locator('.dots i')).toHaveCSS('background-color', ATTENTION_DARK);
    await expect(spotify.locator('.dots i')).toHaveCSS('opacity', '1');
    await foreground(page, SPOTIFY_WINDOW);
    await expect(spotify).not.toHaveAttribute('data-attention');
    await expect.poll(() => runningAnimations(page)).toBe(0);
  });

  test('under reduced motion it does not wiggle; its pill still says so, even with the running indicators off', async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme: 'dark', reducedMotion: 'reduce' });
    await openDock(page);
    await patchSettings(page, { behavior: { runningIndicators: false } });
    await expect(page.locator('.dots i')).toHaveCount(0);

    await flash(page, EXPLORER_WINDOW);
    const explorer = page.locator(EXPLORER);
    await expect(explorer).toHaveAttribute('data-attention', 'true');
    await expect(explorer.locator('.wiggler')).not.toHaveClass(/attention/);
    // One indicator in the whole dock: the attention pill.
    await expect(page.locator('.dots i')).toHaveCount(1);
    await expect(explorer.locator('.dots i')).toHaveCSS('width', '12px');
    await expect(explorer.locator('.dots i')).toHaveCSS('background-color', ATTENTION_DARK);
    expect(await runningAnimations(page)).toBe(0);
    await page.screenshot({ path: `${OUT}/dock-attention-reduced-motion.png` });

    await foreground(page, EXPLORER_WINDOW);
    await expect(page.locator('.dots i')).toHaveCount(0);
  });
});

/** Tauri's file drag events, as the mock delivers them: viewport CSS px. */
function fileDrag(page: Page, event: FileDrag): Promise<void> {
  return page.evaluate((e) => {
    (window as unknown as DemoWindow).__dockMock?.simulateFileDrag(e);
  }, event);
}

test.describe('drop files on the dock to pin', () => {
  const PINNED = '.slot[data-pinned="true"]';
  const GAP = '.drop-gap';
  /** A Start-menu shortcut and a program, as Explorer hands them over, and a document. */
  const LNK =
    'C:\\Users\\me\\AppData\\Roaming\\Microsoft\\Windows\\Start Menu\\Programs\\Paint.lnk';
  const EXE = 'C:\\Tools\\PuTTY\\putty.exe';
  const TXT = 'C:\\Users\\me\\Documents\\notes.txt';
  /** The mock's canonical id for a dropped path (`identify_target` of mock.rs). */
  const idOf = (path: string): string => `exe:${path.replace(/\\/g, '/').toLowerCase()}`;
  const appKey = (id: string): string => `app:${id}`;
  const SLOT = ICON + SLOT_GAP;

  const pinnedKeys = (page: Page): Promise<string[]> =>
    page.$$eval(PINNED, (els) => els.map((el) => (el as HTMLElement).dataset.key ?? ''));

  /** What the UI asked to pin: the targets of `pin_target`, the orders of `reorder_pinned`. */
  function pinRequests(page: Page): Promise<{ targets: unknown[]; orders: unknown[] }> {
    return page.evaluate(() => {
      const calls = (window as unknown as DemoWindow).__dockMock?.calls ?? [];
      return {
        targets: calls.filter((c) => c.type === 'pinTarget').map((c) => c.target),
        orders: calls.filter((c) => c.type === 'reorderPinned').map((c) => c.order),
      };
    });
  }

  /** Halfway between the centres of pinned items `a` and `b`, at the icons' mid-height. */
  async function between(page: Page, a: number, b: number): Promise<{ x: number; y: number }> {
    const boxA = (await page.locator(PINNED).nth(a).boundingBox())!;
    const boxB = (await page.locator(PINNED).nth(b).boundingBox())!;
    return {
      x: (boxA.x + boxA.width / 2 + boxB.x + boxB.width / 2) / 2,
      y: boxA.y + boxA.height / 2,
    };
  }

  /** The keys of the slots on either side of the gap. */
  function gapNeighbours(page: Page): Promise<Array<string | null>> {
    return page
      .locator(GAP)
      .evaluate((gap) => [
        (gap.previousElementSibling as HTMLElement | null)?.dataset.key ?? null,
        (gap.nextElementSibling as HTMLElement | null)?.dataset.key ?? null,
      ]);
  }

  const gapWidth = async (page: Page): Promise<number | undefined> =>
    (await page.locator(GAP).boundingBox())?.width;
  const panelWidth = async (page: Page): Promise<number | undefined> =>
    (await page.locator('.dock-panel').boundingBox())?.width;

  test('a shortcut and a program dropped between two pinned apps are pinned there', async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await openDock(page);
    const before = await pinnedKeys(page);
    expect(before).toHaveLength(5);
    const ids = before.map((key) => key.slice('app:'.length));
    await expect.poll(() => hitRegions(page)).toHaveLength(1);
    const atRest = (await hitRegions(page))[0]!;
    const restWidth = (await panelWidth(page))!;
    const at = await between(page, 1, 2);
    const paths = [LNK, EXE];

    // Over the panel between the second and third pinned app: a one-slot gap
    // opens there and the panel widens around it.
    await fileDrag(page, { type: 'enter', paths, position: at });
    await expect(page.locator(GAP)).toHaveCount(1);
    expect(await gapNeighbours(page)).toEqual([before[1], before[2]]);
    await fileDrag(page, { type: 'over', position: { x: at.x + 3, y: at.y - 4 } });
    await expect.poll(() => gapWidth(page)).toBe(SLOT);
    await expect.poll(() => panelWidth(page)).toBeCloseTo(restWidth + SLOT, 1);
    await expect(page.locator('.drop-indicator')).toBeVisible();
    expect(await gapNeighbours(page)).toEqual([before[1], before[2]]);
    // The window keeps taking the drag over the wider panel: the envelope
    // made room for one more slot; the panel is still the resting one.
    await expect
      .poll(async () => (await hitRegions(page)).at(-1))
      .toEqual({
        panel: atRest.panel,
        envelope: {
          ...atRest.envelope,
          x: atRest.envelope.x - SLOT / 2,
          width: atRest.envelope.width + SLOT,
        },
      });
    // Nothing is asked of the backend before the drop.
    expect(await pinRequests(page)).toEqual({ targets: [], orders: [] });
    await page.screenshot({ path: `${OUT}/dock-file-drop.png` });

    await fileDrag(page, { type: 'drop', paths, position: { x: at.x + 3, y: at.y - 4 } });
    const added = [idOf(LNK), idOf(EXE)];
    // `pin_target` for each path, then one `reorder_pinned` with the new ids at the gap.
    await expect
      .poll(() => pinRequests(page))
      .toEqual({
        targets: [
          { kind: 'shortcut', path: LNK },
          { kind: 'exe', path: EXE, args: [], workingDir: null },
        ],
        orders: [[ids[0], ids[1], ...added, ids[2], ids[3], ids[4]]],
      });
    await expect
      .poll(() => pinnedKeys(page))
      .toEqual([before[0], before[1], ...added.map(appKey), before[2], before[3], before[4]]);
    // The first new icon took the gap's place: no gap is left.
    await expect(page.locator(GAP)).toHaveCount(0);
    await expect(page.locator(`${PINNED}[data-key="${appKey(added[0]!)}"]`)).toHaveAttribute(
      'data-icon',
      'loaded',
    );
    // The new rest geometry is reported: two slots wider, the envelope back to normal.
    await expect
      .poll(async () => (await hitRegions(page)).at(-1)?.panel.width)
      .toBe(atRest.panel.width + 2 * SLOT);
    expect((await hitRegions(page)).at(-1)!.envelope.width).toBe(atRest.envelope.width + 2 * SLOT);
    expect(await callsOfType(page, 'launch')).toBe(0);
  });

  test('the gap follows the drag, closes above the panel and when the drag leaves', async ({
    page,
  }) => {
    await openDock(page);
    const before = await pinnedKeys(page);
    await expect.poll(() => hitRegions(page)).toHaveLength(1);
    const atRest = (await hitRegions(page))[0]!;
    const panel = (await page.locator('.dock-panel').boundingBox())!;

    await fileDrag(page, { type: 'enter', paths: [EXE], position: await between(page, 3, 4) });
    await expect(page.locator(GAP)).toHaveCount(1);
    expect(await gapNeighbours(page)).toEqual([before[3], before[4]]);
    await expect.poll(() => gapWidth(page)).toBe(SLOT);

    // Left of the first pinned centre: the gap moves to the front of the group.
    const first = (await page.locator(PINNED).nth(0).boundingBox())!;
    const front = { x: first.x + 4, y: first.y + first.height / 2 };
    await fileDrag(page, { type: 'over', position: front });
    await expect.poll(() => gapNeighbours(page).catch(() => null)).toEqual(['sep:1', before[0]]);
    await expect(page.locator(GAP)).toHaveCount(1);

    // Up into the transparent margin above the panel (the envelope still
    // takes the drag there): no gap; back down, it opens again.
    await fileDrag(page, { type: 'over', position: { x: front.x, y: panel.y - 10 } });
    await expect(page.locator(GAP)).toHaveCount(0);
    await fileDrag(page, { type: 'over', position: front });
    await expect(page.locator(GAP)).toHaveCount(1);

    // The drag leaves the window: the gap closes, the region is the resting one again.
    await fileDrag(page, { type: 'leave' });
    await expect(page.locator(GAP)).toHaveCount(0);
    await expect.poll(async () => (await hitRegions(page)).at(-1)).toEqual(atRest);

    // A drag released above the panel pins nothing.
    const above = { x: front.x, y: panel.y - 10 };
    await fileDrag(page, { type: 'enter', paths: [EXE], position: above });
    await fileDrag(page, { type: 'drop', paths: [EXE], position: above });
    await page.waitForTimeout(300);
    await expect(page.locator(GAP)).toHaveCount(0);
    expect(await pinRequests(page)).toEqual({ targets: [], orders: [] });
    expect(await pinnedKeys(page)).toEqual(before);
  });

  test('a drag of a text file opens no gap and asks nothing', async ({ page }) => {
    await openDock(page);
    const before = await pinnedKeys(page);
    const logged: string[] = [];
    page.on('console', (message) => logged.push(message.text()));
    const at = await between(page, 1, 2);

    await fileDrag(page, { type: 'enter', paths: [TXT], position: at });
    await fileDrag(page, { type: 'over', position: { x: at.x + 5, y: at.y } });
    await page.waitForTimeout(250);
    await expect(page.locator(GAP)).toHaveCount(0);
    await fileDrag(page, { type: 'drop', paths: [TXT], position: { x: at.x + 5, y: at.y } });
    await page.waitForTimeout(300);
    await expect(page.locator(GAP)).toHaveCount(0);
    expect(await pinRequests(page)).toEqual({ targets: [], orders: [] });
    expect(await pinnedKeys(page)).toEqual(before);
    expect(logged).toContain(`drop: ${TXT} is not a shortcut or a program; not pinned`);
  });

  test('a pin that fails is reported; the other path is still pinned at the gap', async ({
    page,
  }) => {
    await openDock(page);
    const before = await pinnedKeys(page);
    const ids = before.map((key) => key.slice('app:'.length));
    // The first `pin_target` fails the way a Rust error comes back (a string).
    await page.evaluate(() => {
      const mock = (window as unknown as DemoWindow).__dockMock!;
      const pin = mock.pinTarget.bind(mock);
      let failed = false;
      mock.pinTarget = (target: unknown) => {
        if (!failed) {
          failed = true;
          return Promise.reject('the shortcut has no target');
        }
        return pin(target);
      };
    });
    const at = await between(page, 0, 1);
    await fileDrag(page, { type: 'enter', paths: [LNK, EXE], position: at });
    await expect(page.locator(GAP)).toHaveCount(1);
    await fileDrag(page, { type: 'drop', paths: [LNK, EXE], position: at });

    await expect(page.getByRole('status')).toHaveText(
      'Pinning Paint.lnk failed: the shortcut has no target',
    );
    await expect
      .poll(() => pinnedKeys(page))
      .toEqual([before[0], appKey(idOf(EXE)), ...before.slice(1)]);
    expect(await pinRequests(page)).toEqual({
      targets: [{ kind: 'exe', path: EXE, args: [], workingDir: null }],
      orders: [[ids[0], idOf(EXE), ...ids.slice(1)]],
    });
    await expect(page.locator(GAP)).toHaveCount(0);
  });
});

test.describe('drop files on the Recycle Bin', () => {
  const BIN = '.slot[data-kind="recycleBin"]';
  const PINNED = '.slot[data-pinned="true"]';
  const GAP = '.drop-gap';
  /** The bin's lid only tilts on the full icon (ShellIcon.svelte). */
  const FULL_LID = `${BIN} svg g[transform*="rotate"]`;
  const TXT = 'C:\\Users\\me\\Desktop\\notes.txt';
  const FOLDER = 'C:\\Users\\me\\Desktop\\Old photos';
  const EXE = 'C:\\Tools\\PuTTY\\putty.exe';
  const idOf = (path: string): string => `exe:${path.replace(/\\/g, '/').toLowerCase()}`;
  const SLOT = ICON + SLOT_GAP;

  const pinnedKeys = (page: Page): Promise<string[]> =>
    page.$$eval(PINNED, (els) => els.map((el) => (el as HTMLElement).dataset.key ?? ''));

  /** The paths of every `delete_to_recycle_bin` the UI sent, oldest first. */
  function recycleRequests(page: Page): Promise<unknown[]> {
    return page.evaluate(() =>
      ((window as unknown as DemoWindow).__dockMock?.calls ?? [])
        .filter((c) => c.type === 'deleteToRecycleBin')
        .map((c) => c.paths),
    );
  }

  function emptyTheBin(page: Page): Promise<void> {
    return page.evaluate(() => {
      (window as unknown as DemoWindow).__dockMock?.pushRecycleBin({ itemCount: 0, sizeBytes: 0 });
    });
  }

  /** The centre of what `selector` matches, where it is drawn now. */
  async function centreOf(page: Page, selector: string): Promise<{ x: number; y: number }> {
    const box = (await page.locator(selector).boundingBox())!;
    return { x: box.x + box.width / 2, y: box.y + box.height / 2 };
  }

  /** Halfway between the centres of pinned items `a` and `b`. */
  async function between(page: Page, a: number, b: number): Promise<{ x: number; y: number }> {
    const boxA = (await page.locator(PINNED).nth(a).boundingBox())!;
    const boxB = (await page.locator(PINNED).nth(b).boundingBox())!;
    return {
      x: (boxA.x + boxA.width / 2 + boxB.x + boxB.width / 2) / 2,
      y: boxA.y + boxA.height / 2,
    };
  }

  const highlighted = (page: Page): Promise<string[]> =>
    page.$$eval('.slot[data-drop-target]', (els) =>
      els.map((el) => (el as HTMLElement).dataset.key ?? ''),
    );

  test('a drag over the bin highlights it and opens no gap; the drop recycles every path and pins nothing', async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await openDock(page);
    await emptyTheBin(page);
    await expect(page.locator(FULL_LID)).toHaveCount(0);
    const before = await pinnedKeys(page);
    const slot = (await page.locator(BIN).boundingBox())!;
    const at = await centreOf(page, BIN);
    // A document, a folder and a program: the bin takes them all.
    const paths = [TXT, FOLDER, EXE];

    await fileDrag(page, { type: 'enter', paths, position: at });
    await expect.poll(() => highlighted(page)).toEqual(['fixed:recycle-bin']);
    await fileDrag(page, { type: 'over', position: { x: at.x + 4, y: at.y - 3 } });
    // Highlighted like a hovered item: the title pill, the icon a little larger.
    await expect(page.locator(`${BIN} .tooltip`)).toHaveText('Recycle Bin');
    await expect
      .poll(async () => (await page.locator(`${BIN} .bouncer`).boundingBox())?.height)
      .toBeCloseTo(ICON * 1.2, 1);
    // It grew upwards from where it stands, like a magnified icon.
    const grown = (await page.locator(`${BIN} .bouncer`).boundingBox())!;
    expect(grown.y + grown.height).toBeCloseTo(slot.y + slot.height, 1);
    // A transform only: the slot and the panel keep their size, nothing moved.
    expect((await page.locator(BIN).boundingBox())!.width).toBe(slot.width);
    expect((await page.locator(BIN).boundingBox())!.x).toBe(slot.x);
    // A gap would be growing by now; there is none.
    await page.waitForTimeout(250);
    await expect(page.locator(GAP)).toHaveCount(0);
    expect(await recycleRequests(page)).toEqual([]);
    await page.screenshot({ path: `${OUT}/dock-recycle-bin-drop.png` });

    await fileDrag(page, { type: 'drop', paths, position: { x: at.x + 4, y: at.y - 3 } });
    // One request with every path as dropped; the icon turns full from the
    // state Rust (here the mock) sends once the bin has them.
    await expect.poll(() => recycleRequests(page)).toEqual([paths]);
    await expect(page.locator(FULL_LID)).toHaveCount(1);
    expect(await highlighted(page)).toEqual([]);
    await expect(page.locator(`${BIN} .tooltip`)).toHaveCount(0);
    // Nothing was pinned, launched or reordered, and no gap opened.
    expect(await callsOfType(page, 'pinTarget')).toBe(0);
    expect(await callsOfType(page, 'reorderPinned')).toBe(0);
    expect(await callsOfType(page, 'launch')).toBe(0);
    expect(await pinnedKeys(page)).toEqual(before);
    await expect(page.locator(GAP)).toHaveCount(0);
    await expect(page.getByRole('status')).toHaveCount(0);
  });

  test('the highlight follows the drag; elsewhere on the panel a program still pins', async ({
    page,
  }) => {
    await openDock(page);
    const before = await pinnedKeys(page);

    // A program over the bin: the bin lights up, no gap.
    await fileDrag(page, { type: 'enter', paths: [EXE], position: await centreOf(page, BIN) });
    await expect.poll(() => highlighted(page)).toEqual(['fixed:recycle-bin']);
    await expect(page.locator(GAP)).toHaveCount(0);

    // Between two pinned apps: the bin goes dark and the gap opens.
    const pinAt = await between(page, 0, 1);
    await fileDrag(page, { type: 'over', position: pinAt });
    await expect(page.locator(GAP)).toHaveCount(1);
    await expect.poll(async () => (await page.locator(GAP).boundingBox())?.width).toBe(SLOT);
    expect(await highlighted(page)).toEqual([]);
    await expect(page.locator(`${BIN} .tooltip`)).toHaveCount(0);

    // Back to the bin where the open gap pushed it: the gap closes, the bin
    // lights up, and stays lit as the panel narrows under the pointer.
    const binNow = await centreOf(page, BIN);
    await fileDrag(page, { type: 'over', position: binNow });
    await expect.poll(() => highlighted(page)).toEqual(['fixed:recycle-bin']);
    await expect(page.locator(GAP)).toHaveCount(0);
    for (let i = 0; i < 4; i += 1) {
      await fileDrag(page, { type: 'over', position: binNow });
      await page.waitForTimeout(60);
      expect(await highlighted(page)).toEqual(['fixed:recycle-bin']);
      await expect(page.locator(GAP)).toHaveCount(0);
    }

    // The padding past the bin counts as the bin (no gap to push it there).
    const panel = (await page.locator('.dock-panel').boundingBox())!;
    const padding = { x: panel.x + panel.width - 3, y: binNow.y };
    await fileDrag(page, { type: 'over', position: padding });
    await page.waitForTimeout(200);
    expect(await highlighted(page)).toEqual(['fixed:recycle-bin']);
    await expect(page.locator(GAP)).toHaveCount(0);

    // The drag leaves: nothing stays highlighted.
    await fileDrag(page, { type: 'leave' });
    await expect.poll(() => highlighted(page)).toEqual([]);

    // A drop between two pinned apps pins there and recycles nothing.
    await fileDrag(page, { type: 'enter', paths: [EXE], position: pinAt });
    await expect(page.locator(GAP)).toHaveCount(1);
    await fileDrag(page, { type: 'drop', paths: [EXE], position: pinAt });
    await expect
      .poll(() => pinnedKeys(page))
      .toEqual([before[0], `app:${idOf(EXE)}`, ...before.slice(1)]);
    expect(await recycleRequests(page)).toEqual([]);
  });

  test('a refused delete is reported as a failed command', async ({ page }) => {
    await openDock(page);
    // The shell refuses the way a Rust error comes back (a string).
    await page.evaluate(() => {
      const mock = (window as unknown as DemoWindow).__dockMock!;
      mock.deleteToRecycleBin = (paths: string[]) =>
        Promise.reject(`${paths[0]} is open in another program`);
    });
    const locked = 'C:\\Users\\me\\Documents\\report.docx';
    const at = await centreOf(page, BIN);
    await fileDrag(page, { type: 'enter', paths: [locked], position: at });
    await fileDrag(page, { type: 'drop', paths: [locked], position: at });

    await expect(page.getByRole('status')).toHaveText(
      `Moving report.docx to the Recycle Bin failed: ${locked} is open in another program`,
    );
    expect(await highlighted(page)).toEqual([]);
    expect(await callsOfType(page, 'pinTarget')).toBe(0);
  });
});

test.describe('auto-hide', () => {
  /** Long enough to tell "counting down" from "hidden" under Playwright's own latency. */
  const DELAY = 600;
  /** The slide in each direction (ADR-0008). */
  const SLIDE_MS = 200;
  const PANEL = '.dock-panel';
  const STRIP = '.reveal-strip';

  async function setAutoHide(
    page: Page,
    mode: Settings['autoHide']['mode'],
    delayMs = DELAY,
  ): Promise<void> {
    await page.evaluate(
      async ([m, d]) => {
        const mock = (window as unknown as DemoWindow).__dockMock!;
        const settings = await mock.getSettings();
        await mock.updateSettings({ ...settings, autoHide: { mode: m, delayMs: d } });
      },
      [mode, delayMs] as const,
    );
  }

  /** Shown with auto-hide on: the resting region with the envelope grown down to take in the strip. */
  function shownRegion(atRest: HitRegionCall, viewportHeight: number): HitRegionCall {
    return {
      panel: atRest.panel,
      envelope: { ...atRest.envelope, height: viewportHeight - atRest.envelope.y },
    };
  }

  /** Hidden: the bottom 2 px under the resting panel, as both rectangles. */
  function stripRegion(atRest: HitRegionCall, viewportHeight: number): HitRegionCall {
    const strip = {
      x: atRest.panel.x,
      y: viewportHeight - 2,
      width: atRest.panel.width,
      height: 2,
    };
    return { panel: strip, envelope: { ...strip } };
  }

  const lastRegion = async (page: Page): Promise<HitRegionCall | undefined> =>
    (await hitRegions(page)).at(-1);
  const panelStyle = (page: Page): Promise<{ top: number; transform: string; opacity: string }> =>
    page.locator(PANEL).evaluate((el) => ({
      top: el.getBoundingClientRect().top,
      transform: getComputedStyle(el).transform,
      opacity: getComputedStyle(el).opacity,
    }));

  test('slides away after the delay once the pointer has left; the strip at the bottom edge brings it back', async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await openDock(page);
    const viewport = page.viewportSize()!;
    await expect.poll(() => hitRegions(page)).toHaveLength(1);
    const atRest = (await hitRegions(page))[0]!;
    const panel = page.locator(PANEL);
    await expect(panel).not.toHaveAttribute('data-autohide');
    await expect(page.locator(STRIP)).toHaveCount(0);

    // The pointer is over the dock when auto-hide comes on: it stays. The
    // envelope now reaches down to the window's bottom edge.
    await page.locator(APP_SLOT).nth(3).hover();
    await setAutoHide(page, 'always');
    await expect(panel).toHaveAttribute('data-autohide', 'shown');
    await expect.poll(() => lastRegion(page)).toEqual(shownRegion(atRest, viewport.height));
    await page.waitForTimeout(DELAY + SLIDE_MS);
    await expect(panel).toHaveAttribute('data-autohide', 'shown');

    // The pointer leaves: the countdown runs while the dock is still there…
    await page.mouse.move(640, 10);
    await expect(panel).toHaveAttribute('data-autohide', 'pending');
    await expect(panel).toBeVisible();
    // …and past the delay the panel slides out below the window's bottom edge.
    await expect(panel).toHaveAttribute('data-autohide', 'hidden');
    await expect(panel).toBeHidden();
    expect((await panelStyle(page)).top).toBeGreaterThanOrEqual(viewport.height);
    // Only the 2 px strip under the resting panel takes the pointer now.
    const strip = stripRegion(atRest, viewport.height);
    await expect.poll(() => lastRegion(page)).toEqual(strip);
    expect(await page.locator(STRIP).boundingBox()).toEqual(strip.panel);
    await page.screenshot({ path: `${OUT}/dock-autohide-hidden.png` });

    // The pointer reaches the bottom edge under the dock: it slides back in,
    // and the shown region is reported again.
    const x = strip.panel.x + strip.panel.width / 2;
    await page.mouse.move(x, viewport.height - 1);
    await expect(panel).toHaveAttribute('data-autohide', 'shown');
    await expect(panel).toBeVisible();
    await expect.poll(() => lastRegion(page)).toEqual(shownRegion(atRest, viewport.height));
    await expect.poll(async () => (await panelStyle(page)).transform).toBe('none');
    expect(Math.floor((await panelStyle(page)).top)).toBe(atRest.panel.y);
    await page.screenshot({ path: `${OUT}/dock-autohide-revealed.png` });

    // Resting on the edge keeps it up; up into the panel and away again
    // starts the same countdown.
    await page.waitForTimeout(DELAY + SLIDE_MS);
    await expect(panel).toHaveAttribute('data-autohide', 'shown');
    await page.locator(APP_SLOT).nth(2).hover();
    await page.waitForTimeout(DELAY + SLIDE_MS);
    await expect(panel).toHaveAttribute('data-autohide', 'shown');
    await page.mouse.move(640, 10);
    await expect(panel).toHaveAttribute('data-autohide', 'hidden');

    // Turning auto-hide off reveals it at once, with the plain region again.
    await setAutoHide(page, 'off');
    await expect(panel).not.toHaveAttribute('data-autohide');
    await expect(panel).toBeVisible();
    await expect(page.locator(STRIP)).toHaveCount(0);
    await expect.poll(() => lastRegion(page)).toEqual(atRest);
  });

  test('a drag never hides it; released away from the dock, it hides after the delay', async ({
    page,
  }) => {
    await openDock(page);
    const panel = page.locator(PANEL);
    const pinned = page.locator('.slot[data-pinned="true"]');
    const first = (await pinned.nth(0).boundingBox())!;
    await page.mouse.move(first.x + first.width / 2, first.y + first.height / 2);
    await setAutoHide(page, 'always');
    await expect(panel).toHaveAttribute('data-autohide', 'shown');

    // Lift the icon and carry it far above the dock, past the delay. The
    // lifted icon holds the pointer capture, so Chromium reports no leave
    // here; the panel is told the pointer left anyway (what WebView2 does
    // when the window loses the mouse), and the drag must still hold.
    await page.mouse.down();
    await page.mouse.move(640, 20, { steps: 12 });
    await expect(pinned.nth(0)).toHaveAttribute('data-drag', 'lifted');
    await panel.dispatchEvent('pointerleave');
    await page.waitForTimeout(DELAY + SLIDE_MS);
    await expect(panel).toHaveAttribute('data-autohide', 'shown');

    // Released there the drag is cancelled, and the pointer is away.
    await page.mouse.up();
    await expect(page.locator('[data-drag]')).toHaveCount(0);
    await expect(panel).toHaveAttribute('data-autohide', 'pending');
    await expect(panel).toHaveAttribute('data-autohide', 'hidden');
  });

  test('a file dragged onto the bottom edge brings it back; it stays up until the drag leaves', async ({
    page,
  }) => {
    await openDock(page);
    const viewport = page.viewportSize()!;
    await expect.poll(() => hitRegions(page)).toHaveLength(1);
    const atRest = (await hitRegions(page))[0]!;
    const panel = page.locator(PANEL);
    await setAutoHide(page, 'always');
    await expect(panel).toHaveAttribute('data-autohide', 'hidden');

    // An OLE drag produces no pointer events: the drag event on the strip reveals.
    const strip = stripRegion(atRest, viewport.height).panel;
    const paths = ['C:\\Tools\\PuTTY\\putty.exe'];
    const onStrip = { x: strip.x + strip.width / 2, y: viewport.height - 1 };
    await fileDrag(page, { type: 'enter', paths, position: onStrip });
    await expect(panel).toHaveAttribute('data-autohide', 'shown');
    await expect.poll(async () => (await panelStyle(page)).transform).toBe('none');

    // Up over the pinned apps: the gap opens, and the dock stays past the delay.
    const second = (await page.locator('.slot[data-pinned="true"]').nth(1).boundingBox())!;
    await fileDrag(page, {
      type: 'over',
      position: { x: second.x + second.width - 4, y: second.y + 10 },
    });
    await expect(page.locator('.drop-gap')).toHaveCount(1);
    await page.waitForTimeout(DELAY + SLIDE_MS);
    await expect(panel).toHaveAttribute('data-autohide', 'shown');

    // The drag leaves: the gap closes and the countdown starts.
    await fileDrag(page, { type: 'leave' });
    await expect(page.locator('.drop-gap')).toHaveCount(0);
    await expect(panel).toHaveAttribute('data-autohide', 'pending');
    await expect(panel).toHaveAttribute('data-autohide', 'hidden');
  });

  test('the bounce of a launch it started keeps it up until the icon lands', async ({ page }) => {
    await openDock(page);
    const panel = page.locator(PANEL);
    await page.locator(APP_SLOT).nth(3).hover(); // notepad: pinned, not running
    await setAutoHide(page, 'always');
    await expect(panel).toHaveAttribute('data-autohide', 'shown');

    // A middle click launches it (and bounces it for 1.2 s); the pointer leaves at once.
    await page.mouse.down({ button: 'middle' });
    await page.mouse.up({ button: 'middle' });
    await expect.poll(() => callsOfType(page, 'launch')).toBe(1);
    await page.mouse.move(640, 10);
    // Without the hold it would be hidden by now (delay 600 ms + slide).
    await page.waitForTimeout(DELAY + SLIDE_MS);
    await expect(panel).toHaveAttribute('data-autohide', 'shown');
    await expect(panel).toHaveAttribute('data-autohide', 'pending');
    await expect(panel).toHaveAttribute('data-autohide', 'hidden');
  });

  test('an app asking for attention brings it back and keeps it up until the window is activated', async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await openDock(page);
    const viewport = page.viewportSize()!;
    await expect.poll(() => hitRegions(page)).toHaveLength(1);
    const atRest = (await hitRegions(page))[0]!;
    const panel = page.locator(PANEL);
    // The pointer never came near the dock: it goes after the delay.
    await setAutoHide(page, 'always');
    await expect(panel).toHaveAttribute('data-autohide', 'hidden');
    await expect.poll(() => lastRegion(page)).toEqual(stripRegion(atRest, viewport.height));

    // Explorer flashes its taskbar button: the dock slides in, with the
    // region of a shown dock, and the item wiggles.
    await flash(page, 1);
    await expect(panel).toHaveAttribute('data-autohide', 'shown');
    await expect(panel).toBeVisible();
    await expect.poll(() => lastRegion(page)).toEqual(shownRegion(atRest, viewport.height));
    await expect(page.locator('[data-attention] .wiggler.attention')).toHaveCount(1);
    // It stays, with the pointer away, well past the delay.
    await page.waitForTimeout(DELAY * 2 + SLIDE_MS);
    await expect(panel).toHaveAttribute('data-autohide', 'shown');

    // The window is activated from elsewhere: the countdown starts.
    await foreground(page, 1);
    await expect(page.locator('[data-attention]')).toHaveCount(0);
    await expect(panel).toHaveAttribute('data-autohide', 'pending');
    await expect(panel).toHaveAttribute('data-autohide', 'hidden');
    await expect.poll(() => lastRegion(page)).toEqual(stripRegion(atRest, viewport.height));
  });

  test('reduced motion fades instead of sliding; the window shown on request brings it back', async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme: 'dark', reducedMotion: 'reduce' });
    await openDock(page);
    const viewport = page.viewportSize()!;
    await expect.poll(() => hitRegions(page)).toHaveLength(1);
    const atRest = (await hitRegions(page))[0]!;
    const panel = page.locator(PANEL);

    // No delay, and the pointer never came near the dock: it goes at once.
    await setAutoHide(page, 'always', 0);
    await expect(panel).toHaveAttribute('data-autohide', 'hidden');
    await expect(panel).toBeHidden();
    const faded = await panelStyle(page);
    expect(faded.transform).toBe('none');
    expect(faded.opacity).toBe('0');
    expect(Math.floor(faded.top)).toBe(atRest.panel.y);
    await expect.poll(() => lastRegion(page)).toEqual(stripRegion(atRest, viewport.height));

    // The strip brings it back; with no delay, leaving the strip hides it again at once.
    const strip = stripRegion(atRest, viewport.height).panel;
    await page.mouse.move(strip.x + strip.width / 2, viewport.height - 1);
    await expect(panel).toHaveAttribute('data-autohide', 'shown');
    await expect.poll(async () => (await panelStyle(page)).opacity).toBe('1');
    await page.mouse.move(640, 10);
    await expect(panel).toHaveAttribute('data-autohide', 'hidden');

    // The tray or the toggle shortcut hid and showed the window: the page
    // becomes visible again, and the dock comes back and stays until the
    // pointer has been over it.
    await page.evaluate(() => document.dispatchEvent(new Event('visibilitychange')));
    await expect(panel).toHaveAttribute('data-autohide', 'shown');
    await expect(panel).toBeVisible();
    await page.waitForTimeout(SLIDE_MS * 2);
    await expect(panel).toHaveAttribute('data-autohide', 'shown');

    // Switching to "when a window overlaps" with nothing over the dock
    // reveals it and leaves it up, with the plain region.
    await setAutoHide(page, 'whenOverlapped', 0);
    await expect(panel).not.toHaveAttribute('data-autohide');
    await expect(panel).toBeVisible();
    await expect.poll(() => lastRegion(page)).toEqual(atRest);
    await page.waitForTimeout(SLIDE_MS * 2);
    await expect(panel).toBeVisible();
  });

  test('the tray or the shortcut brings a slid-out dock back with one press; a toast meanwhile sits at the bottom edge and rides up with the panel', async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await openDock(page);
    const viewport = page.viewportSize()!;
    const panel = page.locator(PANEL);
    await expect.poll(() => hitRegions(page)).toHaveLength(1);
    const atRest = (await hitRegions(page))[0]!;
    // Shown from the start: nothing to report, the app's flag is false already.
    expect(await autoHiddenReports(page)).toEqual([]);

    // Out after the delay: one report that the panel is hidden.
    await page.locator(APP_SLOT).nth(3).hover();
    await setAutoHide(page, 'always');
    await expect(panel).toHaveAttribute('data-autohide', 'shown');
    await page.mouse.move(640, 10);
    await expect(panel).toHaveAttribute('data-autohide', 'hidden');
    await expect.poll(() => autoHiddenReports(page)).toEqual([true]);
    expect(await callsOfType(page, 'setDockAutoHidden')).toBe(1);
    // The slide and the measurements its end schedules are over before the
    // next command is made to fail.
    await page.waitForTimeout(SLIDE_MS + 100);

    // A command that fails while the dock is hidden (the item is reached
    // without the pointer: a slid-out panel takes none): its toast sits
    // within 12 px of the bottom edge instead of above the empty slot.
    await page.evaluate(
      ([selector, message]) => {
        const mock = (window as unknown as DemoWindow).__dockMock!;
        mock.failNextCommand(message);
        document
          .querySelector(selector)!
          .dispatchEvent(new PointerEvent('pointerup', { button: 0, bubbles: true }));
      },
      [`${APP_SLOT}[data-key="${CODE_KEY}"]`, 'window not found: 4'] as const,
    );
    const toast = page.getByRole('status');
    await expect(toast).toHaveText('Code failed: window not found: 4');
    const toastBottom = async (): Promise<number> => {
      const box = (await toast.boundingBox())!;
      return box.y + box.height;
    };
    await expect.poll(async () => viewport.height - (await toastBottom())).toBeLessThanOrEqual(12);
    expect(viewport.height - (await toastBottom())).toBeGreaterThan(0);
    await expect(panel).toBeHidden();
    await page.screenshot({ path: `${OUT}/dock-autohide-hidden-toast.png` });

    // The tray or the shortcut: Rust emits `dock://show` to this window
    // instead of hiding it. The panel slides back, the shown region and the
    // phase are reported, and the toast is above the panel again.
    await page.evaluate(() => {
      (window as unknown as DemoWindow).__dockMock?.requestShow();
    });
    await expect(panel).toHaveAttribute('data-autohide', 'shown');
    await expect(panel).toBeVisible();
    await expect.poll(() => autoHiddenReports(page)).toEqual([true, false]);
    await expect.poll(() => lastRegion(page)).toEqual(shownRegion(atRest, viewport.height));
    await expect.poll(async () => (await panelStyle(page)).transform).toBe('none');
    await expect
      .poll(async () => (await toastBottom()) <= (await panel.boundingBox())!.y)
      .toBe(true);
    expect(await toastBottom()).toBeLessThanOrEqual(atRest.panel.y);
    await page.screenshot({ path: `${OUT}/dock-autohide-revealed-toast.png` });

    // Like a reveal from the strip, it stays until the pointer has been over
    // it and left; no further report meanwhile.
    await page.waitForTimeout(DELAY + SLIDE_MS);
    await expect(panel).toHaveAttribute('data-autohide', 'shown');
    expect(await autoHiddenReports(page)).toEqual([true, false]);

    // A request while the panel is already shown changes nothing (Rust
    // would have hidden the window instead; the mock just emits).
    await page.evaluate(() => {
      (window as unknown as DemoWindow).__dockMock?.requestShow();
    });
    await page.waitForTimeout(SLIDE_MS);
    await expect(panel).toHaveAttribute('data-autohide', 'shown');
    expect(await autoHiddenReports(page)).toEqual([true, false]);
  });

  test('"when a window overlaps" hides the dock under a window after the delay and shows it when the window is gone', async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await openDock(page);
    const viewport = page.viewportSize()!;
    await expect.poll(() => hitRegions(page)).toHaveLength(1);
    const atRest = (await hitRegions(page))[0]!;
    const panel = page.locator(PANEL);

    // The UI reported where the panel rests, whatever the mode: on the
    // mock monitor the dock window sits on the work area's bottom edge, so
    // the viewport's y maps to the screen's bottom 240 px
    // (docs/ipc/dock-panel.md).
    const onScreen = {
      ...atRest.panel,
      y: WORK_AREA.y + WORK_AREA.height - viewport.height + atRest.panel.y,
    };
    await expect.poll(() => dockPanel(page)).toEqual(onScreen);
    expect(onScreen.y).toBeGreaterThan(WORK_AREA.height - 250);

    // The mode goes on with no window near the dock: nothing changes.
    await setAutoHide(page, 'whenOverlapped');
    await page.waitForTimeout(DELAY + SLIDE_MS);
    await expect(panel).not.toHaveAttribute('data-autohide');
    await expect(panel).toBeVisible();
    await expect(page.locator(STRIP)).toHaveCount(0);
    expect(await lastRegion(page)).toEqual(atRest);

    // Code's window is dragged down over the dock: the countdown runs with
    // the dock still there, then it slides out and the strip is the hit region.
    const over = { x: onScreen.x - 100, y: onScreen.y - 300, width: 900, height: 400 };
    await moveWindow(page, CODE_WINDOW_ID, over);
    await expect(panel).toHaveAttribute('data-autohide', 'pending');
    await expect(panel).toBeVisible();
    await expect(panel).toHaveAttribute('data-autohide', 'hidden');
    await expect(panel).toBeHidden();
    const strip = stripRegion(atRest, viewport.height);
    await expect.poll(() => lastRegion(page)).toEqual(strip);
    expect(await page.locator(STRIP).boundingBox()).toEqual(strip.panel);
    await page.screenshot({ path: `${OUT}/dock-overlapped-hidden.png` });

    // The bottom edge brings it back over the window; it stays while the
    // pointer is on it and hides again after the delay once the pointer leaves.
    await page.mouse.move(strip.panel.x + strip.panel.width / 2, viewport.height - 1);
    await expect(panel).toHaveAttribute('data-autohide', 'shown');
    await expect(panel).toBeVisible();
    await expect.poll(() => lastRegion(page)).toEqual(shownRegion(atRest, viewport.height));
    await page.locator(APP_SLOT).nth(2).hover();
    await page.waitForTimeout(DELAY + SLIDE_MS);
    await expect(panel).toHaveAttribute('data-autohide', 'shown');
    await page.screenshot({ path: `${OUT}/dock-overlapped-revealed.png` });
    await page.mouse.move(640, 10);
    await expect(panel).toHaveAttribute('data-autohide', 'pending');
    await expect(panel).toHaveAttribute('data-autohide', 'hidden');

    // The window is moved away: the dock comes back at once and stays, with
    // the plain region and no strip.
    await moveWindow(page, CODE_WINDOW_ID, { x: 260, y: 40, width: 1600, height: 1000 });
    await expect(panel).not.toHaveAttribute('data-autohide');
    await expect(panel).toBeVisible();
    await expect(page.locator(STRIP)).toHaveCount(0);
    await expect.poll(() => lastRegion(page)).toEqual(atRest);
    await page.waitForTimeout(DELAY + SLIDE_MS);
    await expect(panel).toBeVisible();
    await expect(panel).not.toHaveAttribute('data-autohide');

    // A minimised window (Spotify) moved over the dock changes nothing.
    const SPOTIFY_WINDOW_ID = 6;
    await moveWindow(page, SPOTIFY_WINDOW_ID, over);
    await page.waitForTimeout(DELAY + SLIDE_MS);
    await expect(panel).not.toHaveAttribute('data-autohide');
    await expect(panel).toBeVisible();

    // Over it again, then a window passing by during the countdown: it stays.
    await moveWindow(page, CODE_WINDOW_ID, over);
    await expect(panel).toHaveAttribute('data-autohide', 'pending');
    await moveWindow(page, CODE_WINDOW_ID, { x: 260, y: 40, width: 1600, height: 1000 });
    await expect(panel).not.toHaveAttribute('data-autohide');
    await page.waitForTimeout(DELAY + SLIDE_MS);
    await expect(panel).toBeVisible();

    // With the mode off the same move does nothing.
    await setAutoHide(page, 'off');
    await moveWindow(page, CODE_WINDOW_ID, over);
    await page.waitForTimeout(DELAY + SLIDE_MS);
    await expect(panel).not.toHaveAttribute('data-autohide');
    await expect(panel).toBeVisible();
    expect(await lastRegion(page)).toEqual(atRest);
  });

  test('a fullscreen app hides it at once with auto-hide off; with the taskbar auto-hidden the strip brings it back; the end of fullscreen shows it', async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    // The dock window's bottom edge is the screen's: the strip is there.
    await withTaskbarAutoHidden(page);
    await openDock(page);
    const viewport = page.viewportSize()!;
    await expect.poll(() => hitRegions(page)).toHaveLength(1);
    const atRest = (await hitRegions(page))[0]!;
    const panel = page.locator(PANEL);
    await expect(panel).not.toHaveAttribute('data-autohide');
    await expect(page.locator(STRIP)).toHaveCount(0);

    // A browser went fullscreen (F11): the platform's verdict arrives in the
    // snapshot and the dock is out at once, the strip its only hit region.
    await fullscreen(page, true);
    await expect(panel).toHaveAttribute('data-autohide', 'hidden');
    await expect(panel).toBeHidden();
    expect((await panelStyle(page)).top).toBeGreaterThanOrEqual(viewport.height);
    const strip = stripRegion(atRest, viewport.height);
    await expect.poll(() => lastRegion(page)).toEqual(strip);
    expect(await page.locator(STRIP).boundingBox()).toEqual(strip.panel);
    await page.screenshot({ path: `${OUT}/dock-fullscreen-hidden.png` });

    // The bottom edge under the dock brings it back over the fullscreen app.
    await page.mouse.move(strip.panel.x + strip.panel.width / 2, viewport.height - 1);
    await expect(panel).toHaveAttribute('data-autohide', 'shown');
    await expect(panel).toBeVisible();
    await expect.poll(() => lastRegion(page)).toEqual(shownRegion(atRest, viewport.height));
    await expect.poll(async () => (await panelStyle(page)).transform).toBe('none');
    // Shown, the strip fills the margin under the panel, so going up from
    // the edge is going into the panel: a pixel at a time through the
    // margin, no 0 ms countdown fires on the way.
    const band = (await page.locator(STRIP).boundingBox())!;
    expect(band.y + band.height).toBe(viewport.height);
    expect(Math.abs(band.y - (atRest.panel.y + atRest.panel.height))).toBeLessThanOrEqual(1);
    const x = strip.panel.x + strip.panel.width / 2;
    for (let y = viewport.height - 1; y >= band.y + 1; y -= 1) {
      await page.mouse.move(x, y);
    }
    await page.waitForTimeout(SLIDE_MS);
    await expect(panel).toHaveAttribute('data-autohide', 'shown');
    for (let y = band.y; y >= band.y - 6; y -= 1) {
      await page.mouse.move(x, y);
    }
    // Up into the panel: it stays, with its icons usable.
    await page.waitForTimeout(DELAY + SLIDE_MS);
    await expect(panel).toHaveAttribute('data-autohide', 'shown');
    await page.locator(APP_SLOT).nth(2).hover();
    await page.waitForTimeout(SLIDE_MS);
    await expect(panel).toHaveAttribute('data-autohide', 'shown');
    await page.screenshot({ path: `${OUT}/dock-fullscreen-revealed.png` });

    // Away again: hidden with no delay (well before the 600 ms a mode of
    // `always` would wait).
    const left = Date.now();
    await page.mouse.move(640, 10);
    await expect(panel).toHaveAttribute('data-autohide', 'hidden');
    expect(Date.now() - left).toBeLessThan(DELAY);
    await expect.poll(() => lastRegion(page)).toEqual(strip);

    // The app left fullscreen (or the foreground): back, and it stays, with
    // the plain region and no strip, auto-hide being off.
    await fullscreen(page, false);
    await expect(panel).not.toHaveAttribute('data-autohide');
    await expect(panel).toBeVisible();
    await expect(page.locator(STRIP)).toHaveCount(0);
    await expect.poll(() => lastRegion(page)).toEqual(atRest);
    await page.mouse.move(640, 10);
    await page.waitForTimeout(DELAY + SLIDE_MS);
    await expect(panel).toBeVisible();
    await expect(panel).not.toHaveAttribute('data-autohide');
  });

  test('with auto-hide always, fullscreen hides a shown dock at once and its end does not reveal a hidden one', async ({
    page,
  }) => {
    await openDock(page);
    const viewport = page.viewportSize()!;
    await expect.poll(() => hitRegions(page)).toHaveLength(1);
    const atRest = (await hitRegions(page))[0]!;
    const panel = page.locator(PANEL);

    // Shown under the pointer with auto-hide on; a fullscreen app takes the
    // screen: it stays while the pointer is there, and goes the moment the
    // pointer leaves, with no delay.
    await page.locator(APP_SLOT).nth(3).hover();
    await setAutoHide(page, 'always');
    await expect(panel).toHaveAttribute('data-autohide', 'shown');
    await fullscreen(page, true);
    await page.waitForTimeout(SLIDE_MS * 2);
    await expect(panel).toHaveAttribute('data-autohide', 'shown');
    const left = Date.now();
    await page.mouse.move(640, 10);
    await expect(panel).toHaveAttribute('data-autohide', 'hidden');
    expect(Date.now() - left).toBeLessThan(DELAY);
    await expect.poll(() => lastRegion(page)).toEqual(stripRegion(atRest, viewport.height));

    // Fullscreen ends while hidden: the mode is `always`, so it stays hidden.
    await fullscreen(page, false);
    await page.waitForTimeout(DELAY + SLIDE_MS);
    await expect(panel).toHaveAttribute('data-autohide', 'hidden');
    await expect(panel).toBeHidden();
    await expect.poll(() => lastRegion(page)).toEqual(stripRegion(atRest, viewport.height));

    // The strip still reveals, and the normal delay is back.
    const strip = stripRegion(atRest, viewport.height).panel;
    await page.mouse.move(strip.x + strip.width / 2, viewport.height - 1);
    await expect(panel).toHaveAttribute('data-autohide', 'shown');
    await page.mouse.move(640, 10);
    await expect(panel).toHaveAttribute('data-autohide', 'pending');
    await expect(panel).toHaveAttribute('data-autohide', 'hidden');
  });

  test('with auto-hide always and no delay, going up slowly from the bottom edge into the panel keeps it up', async ({
    page,
  }) => {
    await openDock(page);
    const viewport = page.viewportSize()!;
    await expect.poll(() => hitRegions(page)).toHaveLength(1);
    const atRest = (await hitRegions(page))[0]!;
    const panel = page.locator(PANEL);
    await setAutoHide(page, 'always', 0);
    await expect(panel).toHaveAttribute('data-autohide', 'hidden');
    const strip = stripRegion(atRest, viewport.height).panel;
    const x = strip.x + strip.width / 2;
    await page.mouse.move(x, viewport.height - 1);
    await expect(panel).toHaveAttribute('data-autohide', 'shown');
    // A pixel at a time through the margin under the panel and into it.
    const panelBottom = atRest.panel.y + atRest.panel.height;
    for (let y = viewport.height - 2; y >= panelBottom - 6; y -= 1) {
      await page.mouse.move(x, y);
      if (y === panelBottom + 2) {
        await page.waitForTimeout(SLIDE_MS);
        await expect(panel).toHaveAttribute('data-autohide', 'shown');
      }
    }
    await page.waitForTimeout(SLIDE_MS);
    await expect(panel).toHaveAttribute('data-autohide', 'shown');
    // Down the same way to the edge: still up; then away: out at once.
    for (let y = panelBottom - 6; y < viewport.height; y += 1) {
      await page.mouse.move(x, y);
    }
    await page.waitForTimeout(SLIDE_MS);
    await expect(panel).toHaveAttribute('data-autohide', 'shown');
    await page.mouse.move(640, 10);
    await expect(panel).toHaveAttribute('data-autohide', 'hidden');
    await expect.poll(() => lastRegion(page)).toEqual(stripRegion(atRest, viewport.height));
  });

  test('in fullscreen above a visible stock taskbar there is no strip: the hidden dock takes no pointer and the tray or the shortcut brings it back', async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    // The mock's monitor keeps the taskbar's 48 px: the dock window ends
    // above the screen's edge, inside the fullscreen picture.
    await openDock(page);
    const viewport = page.viewportSize()!;
    await expect.poll(() => hitRegions(page)).toHaveLength(1);
    const atRest = (await hitRegions(page))[0]!;
    const panel = page.locator(PANEL);
    const none = { x: 0, y: 0, width: 0, height: 0 };
    const noRegion = { panel: none, envelope: { ...none } };

    await page.mouse.move(640, 10);
    await fullscreen(page, true);
    await expect(panel).toHaveAttribute('data-autohide', 'hidden');
    await expect(panel).toBeHidden();
    await expect(page.locator(STRIP)).toHaveCount(0);
    await expect.poll(() => lastRegion(page)).toEqual(noRegion);
    // The pointer crossing the window's bottom edge does nothing.
    await page.mouse.move(atRest.panel.x + atRest.panel.width / 2, viewport.height - 1);
    await page.waitForTimeout(SLIDE_MS * 2);
    await expect(panel).toHaveAttribute('data-autohide', 'hidden');
    await page.mouse.move(640, 10);

    // The tray or the shortcut: back, with the plain region (no strip to take in).
    await page.evaluate(() => {
      (window as unknown as DemoWindow).__dockMock?.requestShow();
    });
    await expect(panel).toHaveAttribute('data-autohide', 'shown');
    await expect(panel).toBeVisible();
    await expect.poll(() => lastRegion(page)).toEqual(atRest);
    await expect(page.locator(STRIP)).toHaveCount(0);
    // Over it and away: out at once, and no region again.
    await page.locator(APP_SLOT).nth(2).hover();
    await page.mouse.move(640, 10);
    await expect(panel).toHaveAttribute('data-autohide', 'hidden');
    await expect.poll(() => lastRegion(page)).toEqual(noRegion);

    // The mode `always` has its strip, fullscreen or not; back to off, none.
    await setAutoHide(page, 'always');
    await expect(page.locator(STRIP)).toHaveCount(1);
    await expect.poll(() => lastRegion(page)).toEqual(stripRegion(atRest, viewport.height));
    await setAutoHide(page, 'off');
    await expect(page.locator(STRIP)).toHaveCount(0);
    await expect.poll(() => lastRegion(page)).toEqual(noRegion);
    await expect(panel).toHaveAttribute('data-autohide', 'hidden');

    // Fullscreen ends: back, with the plain region.
    await fullscreen(page, false);
    await expect(panel).not.toHaveAttribute('data-autohide');
    await expect(panel).toBeVisible();
    await expect.poll(() => lastRegion(page)).toEqual(atRest);
  });

  test('the dock shown on request, or revealed from the strip, still hides at the next fullscreen once the pointer is elsewhere', async ({
    page,
  }) => {
    await openDock(page);
    const panel = page.locator(PANEL);
    await page.mouse.move(640, 10);
    await fullscreen(page, true);
    await expect(panel).toHaveAttribute('data-autohide', 'hidden');
    // Shown on request: it stays until the pointer has been over it and left.
    await page.evaluate(() => {
      (window as unknown as DemoWindow).__dockMock?.requestShow();
    });
    await expect(panel).toHaveAttribute('data-autohide', 'shown');
    // Fullscreen ends before the pointer came; the next one hides it at once.
    await fullscreen(page, false);
    await expect(panel).not.toHaveAttribute('data-autohide');
    await fullscreen(page, true);
    await expect(panel).toHaveAttribute('data-autohide', 'hidden');
    await fullscreen(page, false);

    // Revealed from the strip, which goes away under the pointer when
    // fullscreen ends (no `pointerleave` comes from a removed element).
    await withTaskbarAutoHidden(page);
    await openDock(page);
    const viewport = page.viewportSize()!;
    await page.mouse.move(640, 10);
    await fullscreen(page, true);
    await expect(panel).toHaveAttribute('data-autohide', 'hidden');
    const strip = (await page.locator(STRIP).boundingBox())!;
    await page.mouse.move(strip.x + strip.width / 2, viewport.height - 1);
    await expect(panel).toHaveAttribute('data-autohide', 'shown');
    await fullscreen(page, false);
    await expect(page.locator(STRIP)).toHaveCount(0);
    await page.mouse.move(640, 10);
    await fullscreen(page, true);
    await expect(panel).toHaveAttribute('data-autohide', 'hidden');
  });

  test('a pointer resting on the dock keeps it up when fullscreen comes back', async ({ page }) => {
    await openDock(page);
    const panel = page.locator(PANEL);
    await page.locator(APP_SLOT).nth(3).hover();
    await fullscreen(page, true);
    await page.waitForTimeout(SLIDE_MS * 2);
    await expect(panel).toHaveAttribute('data-autohide', 'shown');
    await fullscreen(page, false);
    await expect(panel).not.toHaveAttribute('data-autohide');
    // The pointer has not moved: it is still over the dock when the next
    // fullscreen begins, and the dock stays until it leaves.
    await fullscreen(page, true);
    await page.waitForTimeout(SLIDE_MS * 2);
    await expect(panel).toHaveAttribute('data-autohide', 'shown');
    await page.mouse.move(640, 10);
    await expect(panel).toHaveAttribute('data-autohide', 'hidden');
  });

  test('a file dropped below the panel counts as on the dock only where the strip is', async ({
    page,
  }) => {
    const TXT = 'C:\\Users\\me\\Documents\\notes.txt';
    const panel = page.locator(PANEL);
    /** Shown on request in fullscreen, a file dragged onto the panel and dropped in the band under it. */
    async function dropBelowThePanel(): Promise<void> {
      const viewport = page.viewportSize()!;
      await page.mouse.move(640, 10);
      await fullscreen(page, true);
      await expect(panel).toHaveAttribute('data-autohide', 'hidden');
      await page.evaluate(() => {
        (window as unknown as DemoWindow).__dockMock?.requestShow();
      });
      await expect(panel).toHaveAttribute('data-autohide', 'shown');
      await expect.poll(async () => (await panelStyle(page)).transform).toBe('none');
      const box = (await panel.boundingBox())!;
      const centre = { x: box.x + box.width / 2, y: box.y + box.height / 2 };
      const bottom = box.y + box.height;
      expect(bottom).toBeLessThan(viewport.height - 1);
      // An OLE drag produces no pointer events: only the drag says where it is.
      await fileDrag(page, { type: 'enter', paths: [TXT], position: centre });
      await fileDrag(page, {
        type: 'drop',
        paths: [TXT],
        position: { x: centre.x, y: (bottom + viewport.height) / 2 },
      });
    }

    // Above a visible taskbar there is no strip to report the pointer
    // leaving the band: the drop there is off the dock, and the 0 ms
    // countdown of fullscreen hides it at once.
    await openDock(page);
    await dropBelowThePanel();
    await expect(page.locator(STRIP)).toHaveCount(0);
    await expect(panel).toHaveAttribute('data-autohide', 'hidden');

    // With the taskbar auto-hidden the band is the strip, whose
    // `pointerleave` will say when the pointer goes: the drop is on the dock.
    await withTaskbarAutoHidden(page);
    await openDock(page);
    await dropBelowThePanel();
    await expect(page.locator(STRIP)).toHaveCount(1);
    await page.waitForTimeout(SLIDE_MS * 2);
    await expect(panel).toHaveAttribute('data-autohide', 'shown');
  });

  test('under reduced motion the toasts do not slide with the panel either', async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'dark', reducedMotion: 'reduce' });
    await openDock(page);
    const panel = page.locator(PANEL);
    const toasts = page.locator('.toasts');
    await page.mouse.move(640, 10);
    expect(await computed(toasts, 'transition-property')).toBe('none');
    await setAutoHide(page, 'always', 0);
    await expect(panel).toHaveAttribute('data-autohide', 'hidden');
    await expect(page.locator('.toasts.at-edge')).toHaveCount(1);
    expect(await computed(toasts, 'transition-property')).toBe('none');
    expect(await computed(toasts, 'transition-duration')).toBe('0s');
    expect(await toasts.evaluate((el) => el.getAnimations().length)).toBe(0);
    await page.evaluate(() => {
      (window as unknown as DemoWindow).__dockMock?.requestShow();
    });
    await expect(panel).toHaveAttribute('data-autohide', 'shown');
    await expect(page.locator('.toasts.at-edge')).toHaveCount(0);
    expect(await toasts.evaluate((el) => el.getAnimations().length)).toBe(0);
  });
});

test.describe('settings window', () => {
  // The settings window is a normal, opaque window: 760×600 at 2× DPR.
  test.use({ viewport: { width: 760, height: 600 } });

  const SETTINGS_MOCK = () => (window as unknown as DemoWindow).__dockMock?.getSettings();

  async function openSettings(page: Page): Promise<void> {
    await page.goto('/settings.html?mock=1');
    await expect(page.locator('.page[data-ready="true"]')).toBeVisible();
    await expect(page.locator('.status')).toHaveText('Saved');
    // The dock resolves the pinned apps ~300 ms after start; their names and
    // icons then replace the ids and placeholders. Wait for that, so
    // the names the tests read and the screenshots are stable.
    await expect(page.locator('.pin:not([data-icon="loaded"])')).toHaveCount(0);
    await page.waitForFunction(() =>
      Array.from(document.images).every((img) => img.complete && img.naturalWidth > 0),
    );
  }

  test('no browser context menu anywhere on the page', async ({ page }) => {
    await openSettings(page);
    // dispatchEvent returns false when a listener called preventDefault.
    const allowed = await page.evaluate(() =>
      document.querySelector('h1')!.dispatchEvent(
        new MouseEvent('contextmenu', {
          bubbles: true,
          cancelable: true,
          clientX: 30,
          clientY: 30,
        }),
      ),
    );
    expect(allowed).toBe(false);
  });

  test('dark', async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await openSettings(page);
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
    await page.screenshot({ path: `${OUT}/settings-dark.png` });
    // Every section, for review: unclip the page (app.css fixes html/body/#app
    // to the window) so the scroll area is captured at its full height.
    await page.evaluate(() => {
      document.querySelectorAll<HTMLElement>('html, body, #app, .page, .content').forEach((el) => {
        el.style.height = 'auto';
        el.style.overflow = 'visible';
      });
    });
    await page.screenshot({ path: `${OUT}/settings-full.png`, fullPage: true });
  });

  test('light', async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'light' });
    await openSettings(page);
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
    await page.screenshot({ path: `${OUT}/settings-light.png` });
  });

  test('edits apply live and come back validated from the backend', async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await openSettings(page);

    // Slider with the keyboard: value label follows, the save is debounced.
    const iconSize = page.locator('#icon-size');
    await iconSize.focus();
    await page.keyboard.press('ArrowRight');
    await page.keyboard.press('ArrowRight');
    await expect(page.locator('output[for="icon-size"]')).toHaveText('50 px');
    await expect(page.locator('.status')).toHaveText('Saved');
    expect((await page.evaluate(SETTINGS_MOCK))?.iconSize).toBe(50);

    // Switch.
    const tooltips = page.locator('#tooltips');
    await expect(tooltips).toBeChecked();
    await tooltips.click();
    await expect(tooltips).not.toBeChecked();
    await expect(page.locator('.status')).toHaveText('Saved');

    // Select: the theme applies to this window as well.
    await page.locator('#theme').selectOption('light');
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
    await expect(page.locator('.status')).toHaveText('Saved');
    expect((await page.evaluate(SETTINGS_MOCK))?.theme).toBe('light');

    // Autostart is a plain switch.
    const autostart = page.locator('#autostart');
    await expect(autostart).toBeEnabled();
    await expect(autostart).not.toBeChecked();
    await autostart.click();
    await expect(autostart).toBeChecked();
    await expect(page.locator('.status')).toHaveText('Saved');
    expect((await page.evaluate(SETTINGS_MOCK))?.system.autostart).toBe(true);

    // Hide the Windows taskbar is live (the switch, no M4 tag);
    // the app restores the taskbar on exit, which the browser cannot show.
    const hideTaskbar = page.locator('#hide-taskbar');
    await expect(hideTaskbar).toBeEnabled();
    await expect(hideTaskbar).not.toBeChecked();
    await expect(page.locator('label[for="hide-taskbar"] .tag')).toHaveCount(0);
    await expect(page.locator('#hide-taskbar-description')).toContainText('while Stem runs');
    await hideTaskbar.click();
    await expect(hideTaskbar).toBeChecked();
    await expect(page.locator('.status')).toHaveText('Saved');
    expect((await page.evaluate(SETTINGS_MOCK))?.system.hideWindowsTaskbar).toBe(true);

    // Disabled, not-yet-available options stay disabled; "when a window
    // overlaps" is available, and so is every position.
    await expect(page.locator('#position option[value="left"]')).toBeEnabled();
    await expect(page.locator('#auto-hide option[value="whenOverlapped"]')).toBeEnabled();

    // Reserve screen space is live (the switch, no M4 tag) while
    // Auto-hide is off, and greyed out while the dock hides itself: a dock
    // that auto-hides reserves nothing (Rust applies the same rule).
    const reserve = page.locator('#reserve-space');
    await expect(reserve).toBeEnabled();
    await expect(reserve).not.toBeChecked();
    await expect(page.locator('label[for="reserve-space"] .tag')).toHaveCount(0);
    await expect(page.locator('#reserve-space-description')).toContainText('Auto-hide is off');
    await reserve.click();
    await expect(reserve).toBeChecked();
    await expect(page.locator('.status')).toHaveText('Saved');
    expect((await page.evaluate(SETTINGS_MOCK))?.system.reserveScreenSpace).toBe(true);
    await page.locator('#auto-hide').selectOption('always');
    await expect(reserve).toBeDisabled();
    await expect(reserve).toBeChecked();
    await expect(page.locator('.status')).toHaveText('Saved');
    expect((await page.evaluate(SETTINGS_MOCK))?.autoHide.mode).toBe('always');
    await page.locator('#auto-hide').selectOption('off');
    await expect(reserve).toBeEnabled();
    await expect(page.locator('.status')).toHaveText('Saved');
    expect((await page.evaluate(SETTINGS_MOCK))?.autoHide.mode).toBe('off');

    // One dock per monitor: *All monitors* can be chosen and the
    // per-monitor filter is a live switch (no M3 tags left), which applies
    // only with a dock per monitor: greyed out with the primary's only.
    const monitors = page.locator('#monitors');
    const currentMonitor = page.locator('#current-monitor');
    await expect(monitors).toHaveValue('primary');
    await expect(currentMonitor).toBeDisabled();
    await expect(page.locator('#current-monitor-description')).toContainText(
      'one dock per monitor',
    );
    await expect(page.locator('#monitors option[value="all"]')).toBeEnabled();
    await expect(page.locator('#monitors option[value="all"]')).toHaveText('All monitors');
    await monitors.selectOption('all');
    await expect(page.locator('.status')).toHaveText('Saved');
    expect((await page.evaluate(SETTINGS_MOCK))?.monitors).toBe('all');
    await expect(currentMonitor).toBeEnabled();
    await expect(currentMonitor).not.toBeChecked();
    await expect(page.locator('label[for="current-monitor"] .tag')).toHaveCount(0);
    await expect(page.locator('label[for="monitors"] .tag')).toHaveCount(0);
    await currentMonitor.click();
    await expect(currentMonitor).toBeChecked();
    await expect(page.locator('.status')).toHaveText('Saved');
    expect((await page.evaluate(SETTINGS_MOCK))?.behavior.showOnlyCurrentMonitor).toBe(true);

    // Backdrop: only the built-in blur can be chosen; the two system
    // materials stay listed, greyed and labelled.
    const backdrop = page.locator('#backdrop');
    await expect(backdrop).toHaveValue('css');
    await expect(page.locator('#backdrop option[value="css"]')).toBeEnabled();
    const acrylic = page.locator('#backdrop option[value="acrylic"]');
    const mica = page.locator('#backdrop option[value="mica"]');
    await expect(acrylic).toBeDisabled();
    await expect(acrylic).toHaveText('Acrylic (not yet available)');
    await expect(mica).toBeDisabled();
    await expect(mica).toHaveText('Mica (not yet available)');
    await expect(page.locator('#backdrop-description')).toContainText('not available yet');
    // A settings file that already names a material still shows its value
    // (the stored value round-trips; nothing is rewritten).
    await patchSettings(page, { appearance: { backdrop: 'acrylic' } });
    await expect(backdrop).toHaveValue('acrylic');
    expect((await page.evaluate(SETTINGS_MOCK))?.appearance.backdrop).toBe('acrylic');
  });

  test('the position offers every edge, and a choice is saved', async ({ page }) => {
    await openSettings(page);
    const position = page.locator('#position');
    await expect(position).toHaveValue('bottom');
    const options = position.locator('option');
    await expect(options).toHaveText(['Bottom', 'Left', 'Right', 'Top']);
    for (let i = 0; i < 4; i += 1) {
      await expect(options.nth(i)).toBeEnabled();
    }
    for (const edge of ['left', 'right', 'top', 'bottom'] as const) {
      await position.selectOption(edge);
      await expect(page.locator('.status')).toHaveText('Saved');
      expect((await page.evaluate(SETTINGS_MOCK))?.position).toBe(edge);
    }
  });

  test('auto-hide offers always and "when a window overlaps", and the notes say what each does', async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await openSettings(page);
    const saved = async (): Promise<Settings['autoHide'] | undefined> =>
      (await page.evaluate(SETTINGS_MOCK))?.autoHide;
    const mode = page.locator('#auto-hide');
    await expect(mode).toHaveValue('off');
    await expect(page.locator('#auto-hide option[value="always"]')).toBeEnabled();
    const overlapped = page.locator('#auto-hide option[value="whenOverlapped"]');
    await expect(overlapped).toBeEnabled();
    await expect(overlapped).toHaveText('When a window overlaps');
    await expect(overlapped).not.toHaveText(/not yet|acts as Off/);
    const description = page.locator('#auto-hide-description');
    await expect(description).toContainText('When a window overlaps: only while a window covers');
    await expect(description).toContainText('bottom edge below it');
    await expect(description).toContainText('click-through on');
    await expect(description).not.toContainText(/not yet/);
    await expect(page.locator('#auto-hide-delay-description')).toContainText(
      'after a window starts to overlap',
    );

    await mode.selectOption('always');
    await expect.poll(async () => (await saved())?.mode).toBe('always');
    await expect(page.locator('.status')).toHaveText('Saved');

    // The delay is in milliseconds, 50 at a time from the keyboard; it
    // applies to both modes.
    const delay = page.locator('#auto-hide-delay');
    await expect(page.locator('output[for="auto-hide-delay"]')).toHaveText('250 ms');
    await delay.focus();
    await page.keyboard.press('ArrowRight');
    await expect(page.locator('output[for="auto-hide-delay"]')).toHaveText('300 ms');
    await expect.poll(async () => (await saved())?.delayMs).toBe(300);
    await expect(page.locator('.status')).toHaveText('Saved');

    await mode.selectOption('whenOverlapped');
    await expect.poll(async () => (await saved())?.mode).toBe('whenOverlapped');
    await expect(mode).toHaveValue('whenOverlapped');
    await expect(page.locator('.status')).toHaveText('Saved');
    expect((await saved())?.delayMs).toBe(300);

    await page.locator('#auto-hide').scrollIntoViewIfNeeded();
    await page.screenshot({ path: `${OUT}/settings-autohide.png` });
  });

  test('the toggle shortcut applies on Enter and on blur, Escape reverts', async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await openSettings(page);
    const field = page.locator('#toggle-shortcut');
    const saved = async (): Promise<string | undefined> =>
      (await page.evaluate(SETTINGS_MOCK))?.behavior.toggleShortcut;
    await expect(field).toHaveValue('Ctrl+Alt+D');
    await expect(field).toHaveAccessibleDescription(/Ctrl\+Alt\+D/);
    await field.scrollIntoViewIfNeeded();
    await page.screenshot({ path: `${OUT}/settings-shortcut.png` });

    // Typing alone saves nothing; Escape puts the saved value back.
    await field.fill('Ctrl+Shift+F9');
    await expect(page.locator('.status')).toHaveText('Saved');
    expect(await saved()).toBe('Ctrl+Alt+D');
    await field.press('Escape');
    await expect(field).toHaveValue('Ctrl+Alt+D');

    // Enter applies the trimmed value.
    await field.fill('  Ctrl+Shift+F9 ');
    await field.press('Enter');
    await expect(page.locator('.status')).toHaveText('Saved');
    await expect.poll(saved).toBe('Ctrl+Shift+F9');
    await expect(field).toHaveValue('Ctrl+Shift+F9');

    // Leaving the box applies too; empty means no shortcut.
    await field.fill('');
    await page.locator('#tooltips').focus();
    await expect.poll(saved).toBe('');
    await expect(field).toHaveValue('');
    await expect(field).toHaveAttribute('placeholder', 'None');
    await expect(page.locator('.status')).toHaveText('Saved');
  });

  test('the focus shortcut has its own row under the toggle; it applies on Enter and on blur, Escape reverts', async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await openSettings(page);
    const field = page.locator('#focus-shortcut');
    const toggle = page.locator('#toggle-shortcut');
    const saved = async (): Promise<string | undefined> =>
      (await page.evaluate(SETTINGS_MOCK))?.behavior.focusShortcut;
    const savedToggle = async (): Promise<string | undefined> =>
      (await page.evaluate(SETTINGS_MOCK))?.behavior.toggleShortcut;
    await expect(field).toHaveValue('Ctrl+Alt+Shift+D');
    await expect(field).toHaveAccessibleName('Focus the dock');
    await expect(field).toHaveAccessibleDescription(/Ctrl\+Alt\+Shift\+D/);
    // The next row after the toggle's, in the same column.
    const below = await page.evaluate(() => {
      const rows = Array.from(document.querySelectorAll('input[type="text"], input:not([type])'));
      const ids = rows.map((r) => r.id);
      return ids.indexOf('focus-shortcut') - ids.indexOf('toggle-shortcut');
    });
    expect(below).toBe(1);
    const [fieldBox, toggleBox] = [(await field.boundingBox())!, (await toggle.boundingBox())!];
    expect(fieldBox.y).toBeGreaterThan(toggleBox.y);
    expect(fieldBox.x).toBeCloseTo(toggleBox.x, 0);
    await field.scrollIntoViewIfNeeded();
    await page.screenshot({ path: `${OUT}/settings-focus-shortcut.png` });

    // Typing alone saves nothing; Escape puts the saved value back.
    await field.fill('Ctrl+Shift+F8');
    await expect(page.locator('.status')).toHaveText('Saved');
    expect(await saved()).toBe('Ctrl+Alt+Shift+D');
    await field.press('Escape');
    await expect(field).toHaveValue('Ctrl+Alt+Shift+D');

    // Enter applies the trimmed value; the toggle is left alone.
    await field.fill('  Ctrl+Shift+F8 ');
    await field.press('Enter');
    await expect.poll(saved).toBe('Ctrl+Shift+F8');
    await expect(field).toHaveValue('Ctrl+Shift+F8');
    expect(await savedToggle()).toBe('Ctrl+Alt+D');

    // The toggle's own keys are kept as written, on blur (Rust registers
    // nothing for them and logs why).
    await field.fill('alt+ctrl+d');
    await page.locator('#tooltips').focus();
    await expect.poll(saved).toBe('alt+ctrl+d');
    expect(await savedToggle()).toBe('Ctrl+Alt+D');

    // Empty means no shortcut.
    await field.fill('');
    await field.press('Enter');
    await expect.poll(saved).toBe('');
    await expect(field).toHaveAttribute('placeholder', 'None');
    await expect(page.locator('.status')).toHaveText('Saved');
  });

  test("pinned apps show the dock's names and icons, and can be moved and unpinned", async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await openSettings(page);
    const names = () => page.locator('.pin-name').allTextContents();
    const before = await names();
    // The titles the dock resolved (the mock derives them from the exe
    // name), never the raw ids; the user's label still wins (the last one).
    expect(before).toEqual(['explorer', 'firefox', 'WindowsTerminal', 'notepad', 'Settings']);
    // Each resolved pin has the dock's icon for it: a data URL from
    // `get_icon` (the mock draws SVG; Rust returns PNG), drawn at 24 CSS px.
    const icons = page.locator('.pin img');
    await expect(icons).toHaveCount(5);
    for (const src of await icons.evaluateAll((imgs) =>
      imgs.map((img) => (img as HTMLImageElement).src),
    )) {
      expect(src).toMatch(/^data:image\//);
    }
    expect((await icons.first().boundingBox())!.width).toBe(24);
    expect((await icons.first().boundingBox())!.height).toBe(24);
    await expect(page.locator('.pin .placeholder')).toHaveCount(0);
    // The launch target stays as the second line.
    await expect(page.locator('.pin-launch').first()).toHaveText('C:\\Windows\\explorer.exe');
    await expect(page.locator('section[aria-labelledby="h-pinned"] .note')).toHaveText(
      'Drag apps onto the dock to pin them.',
    );

    await page.getByRole('button', { name: `Move ${before[0]} down` }).click();
    await expect.poll(names).toEqual([before[1], before[0], before[2], before[3], before[4]]);
    await page.getByRole('button', { name: 'Move Settings up' }).click();
    await expect.poll(names).toEqual([before[1], before[0], before[2], before[4], before[3]]);
    await page.getByRole('button', { name: `Unpin ${before[2]}` }).click();
    await expect.poll(names).toEqual([before[1], before[0], before[4], before[3]]);
    await expect(page.locator('.status')).toHaveText('Saved');
    expect((await page.evaluate(SETTINGS_MOCK))?.pinned.map((p) => p.id)).toHaveLength(4);

    await page.locator('#h-pinned').scrollIntoViewIfNeeded();
    await page.screenshot({ path: `${OUT}/settings-pinned.png` });
  });

  test('a pin the dock has not resolved yet shows its id and a placeholder, then its name and icon', async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await openSettings(page);
    // A settings file naming an app the dock has never seen. The mock
    // resolves the identity ~300 ms after the change, as the service does
    // lazily; the row's first facts are read in the page, in the same frame
    // it appears, so no round trip can land after the resolution.
    const PAINT = 'exe:c:/windows/system32/mspaint.exe';
    const unresolved = await page.evaluate(async (id) => {
      const mock = (window as unknown as DemoWindow).__dockMock!;
      const settings = await mock.getSettings();
      settings.pinned.push({
        id,
        label: null,
        launch: {
          kind: 'exe',
          path: 'C:\\Windows\\System32\\mspaint.exe',
          args: [],
          workingDir: null,
        },
      });
      await mock.updateSettings(settings);
      let row: HTMLElement | null = null;
      for (let i = 0; i < 10 && row === null; i += 1) {
        await new Promise(requestAnimationFrame);
        row = document.querySelector<HTMLElement>(`.pin[data-id="${id}"]`);
      }
      return row === null
        ? null
        : {
            icon: row.dataset.icon,
            name: row.querySelector('.pin-name')?.textContent,
            images: row.querySelectorAll('img').length,
            placeholders: row.querySelectorAll('.placeholder').length,
          };
    }, PAINT);
    expect(unresolved).toEqual({ icon: 'none', name: PAINT, images: 0, placeholders: 1 });

    // Resolved: the name and the icon replace them, in the same row (the
    // list is keyed by the pin id); the other rows are untouched.
    const row = page.locator(`.pin[data-id="${PAINT}"]`);
    await expect(row.locator('.pin-name')).toHaveText('mspaint');
    await expect(row).toHaveAttribute('data-icon', 'loaded');
    await expect(row.locator('img')).toHaveAttribute('src', /^data:image\//);
    await expect(row.locator('.placeholder')).toHaveCount(0);
    await expect(page.locator('.pin')).toHaveCount(6);
    await expect(page.locator('.pin:not([data-icon="loaded"])')).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'Unpin mspaint' })).toBeVisible();
  });

  test('reset asks first, then restores the defaults', async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await openSettings(page);
    await page.locator('#icon-size').focus();
    await page.keyboard.press('ArrowRight');
    await expect(page.locator('output[for="icon-size"]')).toHaveText('49 px');

    await page.getByRole('button', { name: 'Reset to defaults' }).click();
    await expect(page.getByRole('alertdialog')).toBeVisible();
    await page.screenshot({ path: `${OUT}/settings-reset.png` });
    await page.getByRole('button', { name: 'Cancel' }).click();
    await expect(page.locator('output[for="icon-size"]')).toHaveText('49 px');

    await page.getByRole('button', { name: 'Reset to defaults' }).click();
    await page.getByRole('button', { name: 'Reset', exact: true }).click();
    await expect(page.locator('output[for="icon-size"]')).toHaveText('48 px');
    await expect(page.locator('.empty')).toHaveText('Nothing is pinned yet.');
    await expect(page.locator('.status')).toHaveText('Saved');
  });

  test('About shows the version and the files, and each Open folder asks Rust for its folder', async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await openSettings(page);
    const about = page.locator('section[aria-labelledby="h-about"]');
    const row = (name: string): Locator => about.locator(`.about-row[data-row="${name}"]`);
    await expect(about.locator('h2')).toHaveText('About');
    await expect(row('version').locator('.about-label')).toHaveText('Stem 0.1.0-mock');
    await expect(row('version').locator('.about-detail')).toHaveText('Platform: mock (demo data)');
    await expect(row('settings').locator('.about-label')).toHaveText('Settings file');
    await expect(row('settings').locator('code')).toHaveText(
      'C:\\Users\\me\\AppData\\Roaming\\dev.davidcristian.stem\\settings.json',
    );
    await expect(row('log').locator('.about-label')).toHaveText('Log file');
    await expect(row('log').locator('code')).toHaveText(
      'C:\\Users\\me\\AppData\\Local\\dev.davidcristian.stem\\logs\\stem.log',
    );
    // The About section replaces the footer's old line about the file.
    await expect(page.locator('footer')).not.toContainText('settings.json');
    // The section is the last one on the page.
    await expect(page.locator('main section').last()).toHaveAttribute('aria-labelledby', 'h-about');

    const openSettingsFolder = page.getByRole('button', {
      name: 'Open folder of the settings file',
    });
    const openLogFolder = page.getByRole('button', { name: 'Open folder of the log file' });
    await expect(openSettingsFolder).toHaveText('Open folder');
    await expect(openLogFolder).toBeEnabled();
    await openSettingsFolder.click();
    await expect.poll(() => folderCalls(page)).toEqual(['settings']);
    await openLogFolder.click();
    await expect.poll(() => folderCalls(page)).toEqual(['settings', 'logs']);
    await expect(about.locator('[role="alert"]')).toHaveCount(0);
    // Opening a folder saves nothing.
    await expect(page.locator('.status')).toHaveText('Saved');

    // A refusal from Rust is the section's own notice, not the save status.
    await page.evaluate((message) => {
      (window as unknown as DemoWindow).__dockMock?.failNextCommand(message);
    }, 'the log folder is unknown on this host');
    await openLogFolder.click();
    await expect(about.locator('[role="alert"]')).toHaveText(
      "Couldn't open the folder: the log folder is unknown on this host",
    );
    await expect(page.locator('.status')).toHaveText('Saved');
    // A refused command never reached the mock's job, so nothing was recorded.
    expect(await folderCalls(page)).toEqual(['settings', 'logs']);
    await about.scrollIntoViewIfNeeded();
    await page.screenshot({ path: `${OUT}/settings-about.png` });

    // The next attempt clears the notice.
    await openLogFolder.click();
    await expect.poll(() => folderCalls(page)).toEqual(['settings', 'logs', 'logs']);
    await expect(about.locator('[role="alert"]')).toHaveCount(0);
  });

  test('About says when file logging is off and keeps that button disabled', async ({ page }) => {
    // The page asks `get_app_info` once, as its store starts, so the mock's
    // answer is changed the moment the page exposes the mock (`exposeMock`,
    // before the store starts): a run started with *Write a log file* off.
    await page.addInitScript(() => {
      let backend: { setAppInfo: (info: { logPath: string }) => void } | undefined;
      Object.defineProperty(window, '__dockMock', {
        configurable: true,
        get: () => backend,
        set: (value: typeof backend) => {
          backend = value;
          backend?.setAppInfo({ logPath: '' });
        },
      });
    });
    await openSettings(page);
    const row = page.locator('.about-row[data-row="log"]');
    await expect(row.locator('.about-detail')).toHaveText('File logging is off');
    await expect(row.locator('code')).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'Open folder of the log file' })).toBeDisabled();
    await expect(
      page.getByRole('button', { name: 'Open folder of the settings file' }),
    ).toBeEnabled();
    await expect(page.locator('.about-row[data-row="version"] .about-label')).toHaveText(
      'Stem 0.1.0-mock',
    );
  });

  test('each fixed item has a switch that applies at once', async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await openSettings(page);
    const saved = async (): Promise<Settings['items'] | undefined> =>
      (await page.evaluate(SETTINGS_MOCK))?.items;
    const launcher = page.locator('#item-launcher');
    const taskView = page.locator('#item-task-view');
    const showDesktop = page.locator('#item-show-desktop');
    await expect(launcher).toBeChecked();
    await expect(taskView).not.toBeChecked();
    await expect(showDesktop).not.toBeChecked();
    await expect(launcher).toHaveAccessibleName('Start');
    await expect(taskView).toHaveAccessibleName('Task View');
    await expect(showDesktop).toHaveAccessibleName('Show Desktop');
    await expect(page.locator('#item-task-view-description')).toHaveText(/open windows/);
    await expect(page.locator('#item-show-desktop-description')).toHaveText(/Minimises every/);

    await taskView.click();
    await expect.poll(async () => (await saved())?.taskView).toBe(true);
    await showDesktop.click();
    await expect.poll(async () => (await saved())?.showDesktop).toBe(true);
    await launcher.click();
    await expect.poll(async () => (await saved())?.launcher).toBe(false);
    await expect(page.locator('.status')).toHaveText('Saved');
    await expect(launcher).not.toBeChecked();
    await expect(taskView).toBeChecked();
    await expect(showDesktop).toBeChecked();
    await page.locator('#h-items').scrollIntoViewIfNeeded();
    await page.screenshot({ path: `${OUT}/settings-items.png` });
  });

  test('Quick Settings and Notification Center have switches, off by default, that apply at once', async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await openSettings(page);
    const saved = async (): Promise<Settings['items'] | undefined> =>
      (await page.evaluate(SETTINGS_MOCK))?.items;
    const quickSettings = page.locator('#item-quick-settings');
    const notificationCenter = page.locator('#item-notification-center');
    await expect(quickSettings).not.toBeChecked();
    await expect(notificationCenter).not.toBeChecked();
    await expect(quickSettings).toHaveAccessibleName('Quick Settings');
    await expect(notificationCenter).toHaveAccessibleName('Notification Center');
    await expect(page.locator('#item-quick-settings-description')).toHaveText(/volume/);
    await expect(page.locator('#item-notification-center-description')).toHaveText(/notifications/);
    // In the dock's order: after Show Desktop, before the Recycle Bin.
    const rows = await page
      .locator('section[aria-labelledby="h-items"] [role="switch"]')
      .evaluateAll((els) => els.map((el) => el.id));
    expect(rows.indexOf('item-quick-settings')).toBe(rows.indexOf('item-show-desktop') + 1);
    expect(rows.indexOf('item-notification-center')).toBe(rows.indexOf('item-quick-settings') + 1);
    // The battery and clock switches come next, then the Recycle Bin.
    expect(rows.indexOf('item-recycle-bin')).toBe(rows.indexOf('item-notification-center') + 3);

    await quickSettings.click();
    await expect.poll(async () => (await saved())?.quickSettings).toBe(true);
    await notificationCenter.click();
    await expect.poll(async () => (await saved())?.notificationCenter).toBe(true);
    await expect(page.locator('.status')).toHaveText('Saved');
    await expect(quickSettings).toBeChecked();
    await expect(notificationCenter).toBeChecked();
    await notificationCenter.scrollIntoViewIfNeeded();
    await page.screenshot({ path: `${OUT}/settings-items-quick-settings.png` });
    await quickSettings.click();
    await expect.poll(async () => (await saved())?.quickSettings).toBe(false);
  });

  test('the battery and the clock have switches, off by default, that apply at once', async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await openSettings(page);
    const saved = async (): Promise<Settings['items'] | undefined> =>
      (await page.evaluate(SETTINGS_MOCK))?.items;
    const battery = page.locator('#item-battery');
    const clock = page.locator('#item-clock');
    await expect(battery).not.toBeChecked();
    await expect(clock).not.toBeChecked();
    await expect(battery).toHaveAccessibleName('Battery');
    await expect(clock).toHaveAccessibleName('Clock');
    await expect(page.locator('#item-battery-description')).toHaveText(
      /only on a PC with a battery/,
    );
    await expect(page.locator('#item-clock-description')).toHaveText(/time and the date/);
    // In the dock's order: after Notification Center, battery before clock,
    // then the Recycle Bin.
    const rows = await page
      .locator('section[aria-labelledby="h-items"] [role="switch"]')
      .evaluateAll((els) => els.map((el) => el.id));
    expect(rows.indexOf('item-battery')).toBe(rows.indexOf('item-notification-center') + 1);
    expect(rows.indexOf('item-clock')).toBe(rows.indexOf('item-battery') + 1);
    expect(rows.indexOf('item-recycle-bin')).toBe(rows.indexOf('item-clock') + 1);

    await battery.click();
    await expect.poll(async () => (await saved())?.battery).toBe(true);
    await clock.click();
    await expect.poll(async () => (await saved())?.clock).toBe(true);
    await expect(page.locator('.status')).toHaveText('Saved');
    await expect(battery).toBeChecked();
    await expect(clock).toBeChecked();
    await clock.scrollIntoViewIfNeeded();
    await page.screenshot({ path: `${OUT}/settings-items-clock-battery.png` });
    await battery.click();
    await expect.poll(async () => (await saved())?.battery).toBe(false);
    await expect.poll(async () => (await saved())?.clock).toBe(true);
  });

  test('follows a contrast theme and the accent of Windows; forced colours keep switches and sliders readable', async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await openSettings(page);
    const html = page.locator('html');
    const on = page.locator('#magnification');
    const off = page.locator('#autostart');
    await setOsTheme(page, { highContrast: true, accent: '#FF8C00' });
    await expect(html).toHaveAttribute('data-high-contrast', '');
    await expect(html).toHaveAttribute('data-theme', 'dark');
    // Solid black page and cards, every line in the text colour, the Windows accent.
    await expect(page.locator('body')).toHaveCSS('background-color', 'rgb(0, 0, 0)');
    await expect(page.locator('.card').first()).toHaveCSS('background-color', 'rgb(0, 0, 0)');
    await expect(page.locator('.card').first()).toHaveCSS('border-top-color', 'rgb(255, 255, 255)');
    await expect(off).toHaveCSS('border-top-color', 'rgb(255, 255, 255)');
    await expect(on).toHaveCSS('background-color', 'rgb(255, 140, 0)');
    await page.screenshot({ path: `${OUT}/settings-high-contrast.png` });

    // Forced colours, as WebView2 renders under a contrast theme: the
    // switches' knobs and the sliders' tracks would be painted away.
    await page.emulateMedia({ colorScheme: 'dark', forcedColors: 'active' });
    const knob = (toggle: Locator): Promise<string> =>
      computed(toggle, 'background-color', '::before');
    expect(await knob(on)).not.toBe(await computed(on, 'background-color'));
    expect(await knob(off)).not.toBe(await computed(off, 'background-color'));
    expect(await computed(on, 'background-color')).not.toBe(
      await computed(off, 'background-color'),
    );
    await expect(page.locator('#icon-size')).toHaveCSS('forced-color-adjust', 'none');
    await page.screenshot({ path: `${OUT}/settings-forced-colors.png` });

    // The contrast theme is turned off again.
    await page.emulateMedia({ colorScheme: 'dark', forcedColors: 'none' });
    await setOsTheme(page, {});
    await expect(html).not.toHaveAttribute('data-high-contrast');
    await expect(page.locator('body')).toHaveCSS('background-color', 'rgb(32, 32, 32)');
  });

  test('every control has a name and the keyboard reaches the first one', async ({ page }) => {
    await openSettings(page);
    const controls = page.locator('input, select, button');
    const count = await controls.count();
    expect(count).toBeGreaterThan(30);
    for (let i = 0; i < count; i += 1) {
      await expect(controls.nth(i)).toHaveAccessibleName(/\S/);
    }
    await page.keyboard.press('Tab');
    await expect(page.locator('#icon-size')).toBeFocused();
  });
});

test.describe('one dock per monitor', () => {
  /** The second monitor of the mock's `twoMonitors` scenario (`SECOND_MONITOR_ID` in src/lib/ipc/mock.ts). */
  const DISPLAY3 = '\\\\.\\DISPLAY3';
  const DISPLAY3_WORK_AREA: Rect = { x: 2560, y: 0, width: 1920, height: 1032 };
  const DISPLAY3_SCALE = 1.5;
  const SECOND_PAGE = `mock=1&scenario=twoMonitors&monitor=${encodeURIComponent(DISPLAY3)}`;
  const PRIMARY_PAGE = 'mock=1&scenario=twoMonitors';
  const PANEL = '.dock-panel';
  const DELAY = 600;
  const SLIDE_MS = 200;
  const NOTEPAD_SLOT = `${APP_SLOT}[data-key="app:exe:c:/windows/notepad.exe"]`;
  const CODE_SLOT = `${APP_SLOT}[data-key="${CODE_KEY}"]`;
  /** Paint's window in the scenario, on the second monitor. */
  const PAINT_WINDOW_ID = 8;
  const PAINT_FRAME: Rect = { x: 3100, y: 60, width: 1100, height: 700 };

  const appKeys = (page: Page): Promise<string[]> =>
    page.$$eval(APP_SLOT, (els) => els.map((el) => (el as HTMLElement).dataset.key ?? ''));
  const dots = (page: Page, selector: string): Locator => page.locator(selector).locator('.dots i');
  /** The setting, with a dock on every monitor (`monitors = all`), the only case it applies to. */
  const showOnlyCurrentMonitor = (page: Page, on: boolean): Promise<void> =>
    patchSettings(page, { monitors: 'all', behavior: { showOnlyCurrentMonitor: on } });
  const SETTINGS = (): Promise<Settings> | undefined =>
    (window as unknown as DemoWindow).__dockMock?.getSettings();
  const pageLabel = (page: Page): Promise<string | undefined> =>
    page.evaluate(() => (window as unknown as DemoWindow).__dockMock?.pageLabel);
  /** Paint's window reported again on `monitor` (a `WindowUpdated` after a move between monitors). */
  const movePaintTo = (page: Page, monitor: string, rect: Rect): Promise<void> =>
    page.evaluate(
      (raw) => {
        (window as unknown as DemoWindow).__dockMock?.pushWindow(raw as RawWindow);
      },
      { ...PAINT_WINDOW, id: PAINT_WINDOW_ID, monitor, rect },
    );

  test('the secondary page lists the pinned apps and only the windows on its monitor', async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await openDock(page, SECOND_PAGE);
    expect(await pageLabel(page)).toBe('dock-DISPLAY3');
    // The setting off: every window, like the primary.
    await expect(page.locator(APP_SLOT)).toHaveCount(8);
    await expect(dots(page, NOTEPAD_SLOT)).toHaveCount(1);
    await expect(dots(page, PAINT_SLOT)).toHaveCount(1);
    await expect(page.locator(CODE_SLOT)).toHaveCount(1);

    // On: the five pinned apps stay (Notepad with its one window here),
    // Paint is the only running app on this monitor; Code and Spotify, on
    // the primary, are gone.
    await showOnlyCurrentMonitor(page, true);
    await expect(page.locator(APP_SLOT)).toHaveCount(6);
    expect(await appKeys(page)).toEqual([
      'app:exe:c:/windows/explorer.exe',
      'app:exe:c:/program files/mozilla firefox/firefox.exe',
      'app:exe:c:/program files/windowsapps/terminal/windowsterminal.exe',
      'app:exe:c:/windows/notepad.exe',
      'app:uri:ms-settings:',
      'app:exe:c:/windows/system32/mspaint.exe',
    ]);
    await expect(dots(page, NOTEPAD_SLOT)).toHaveCount(1);
    await expect(dots(page, PAINT_SLOT)).toHaveCount(1);
    await expect(page.locator(`${APP_SLOT}[data-pinned="true"]`)).toHaveCount(5);
    await expect(page.locator('[role="separator"]')).toHaveCount(3);
    // Firefox's window is in the foreground, on the primary: nothing is
    // active here.
    await expect(page.locator(`${APP_SLOT}[data-active="true"]`)).toHaveCount(0);
    await expect(page.locator(`${APP_SLOT}:not([data-icon="loaded"])`)).toHaveCount(0);
    await page.screenshot({ path: `${OUT}/dock-second-monitor.png` });

    // Off again: everything is back.
    await showOnlyCurrentMonitor(page, false);
    await expect(page.locator(APP_SLOT)).toHaveCount(8);
    await expect(page.locator(CODE_SLOT)).toHaveCount(1);
  });

  test('the primary page shows everything with the setting off and only its own windows with it on', async ({
    page,
  }) => {
    await openDock(page, PRIMARY_PAGE);
    expect(await pageLabel(page)).toBe('dock');
    await expect(page.locator(APP_SLOT)).toHaveCount(8);
    await expect(dots(page, NOTEPAD_SLOT)).toHaveCount(1);
    await expect(page.locator(PAINT_SLOT)).toHaveCount(1);

    await showOnlyCurrentMonitor(page, true);
    // Notepad stays, pinned, with no window on this monitor; Paint is gone.
    await expect(page.locator(APP_SLOT)).toHaveCount(7);
    await expect(dots(page, NOTEPAD_SLOT)).toHaveCount(0);
    await expect(page.locator(PAINT_SLOT)).toHaveCount(0);
    await expect(page.locator(CODE_SLOT)).toHaveCount(1);
    await expect(page.locator(`${APP_SLOT}[data-active="true"]`)).toHaveCount(1);
    await expect(page.locator('[role="separator"]')).toHaveCount(3);

    // Paint dragged onto this monitor appears at once; dragged back, it goes.
    await movePaintTo(page, 'mock-primary', { x: 100, y: 100, width: 1100, height: 700 });
    await expect(page.locator(PAINT_SLOT)).toHaveCount(1);
    await expect(page.locator(APP_SLOT)).toHaveCount(8);
    await movePaintTo(page, DISPLAY3, PAINT_FRAME);
    await expect(page.locator(PAINT_SLOT)).toHaveCount(0);
    await expect(page.locator(APP_SLOT)).toHaveCount(7);
  });

  test('a click on the secondary page acts on the window on its monitor, not on the app\u2019s first', async ({
    page,
  }) => {
    await openDock(page, SECOND_PAGE);
    await patchSettings(page, { monitors: 'all', behavior: { showOnlyCurrentMonitor: true } });
    // The page has reported its panel: the engine knows this dock's monitor.
    await expect.poll(() => dockPanel(page)).not.toBeNull();
    // Notepad gets a window on the primary, seen before its one here (7).
    const notepad = (id: number, monitor: string): RawWindow => ({
      ...PAINT_WINDOW,
      id,
      title: `Notepad ${id}`,
      className: 'Notepad',
      processId: 1000 + id,
      exePath: 'C:\\Windows\\notepad.exe',
      monitor,
    });
    await page.evaluate(
      ([onPrimary, here]) => {
        const mock = (window as unknown as DemoWindow).__dockMock!;
        mock.pushWindowRemoved(here.id);
        mock.pushWindow(onPrimary);
        mock.pushWindow(here);
      },
      [notepad(90, 'mock-primary'), notepad(7, DISPLAY3)] as const,
    );
    await expect(dots(page, NOTEPAD_SLOT)).toHaveCount(1);
    const windowActions = (): Promise<unknown[]> =>
      page.evaluate(() =>
        ((window as unknown as DemoWindow).__dockMock?.calls ?? [])
          .filter((c) => c.type === 'windowAction')
          .map((c) => ({ windowId: c.windowId, action: c.action })),
      );

    // Nothing of Notepad's is active: the click activates the window here.
    const slot = page.locator(NOTEPAD_SLOT);
    await slot.click();
    await expect.poll(windowActions).toEqual([{ windowId: 7, action: 'activate' }]);
    await expect(slot).toHaveAttribute('data-active', 'true');
    // Active and alone on this monitor: minimised, not cycled to window 90.
    await slot.click();
    await expect.poll(windowActions).toEqual([
      { windowId: 7, action: 'activate' },
      { windowId: 7, action: 'minimize' },
    ]);
    await expect(slot).not.toHaveAttribute('data-active', 'true');
  });

  test('with the dock on the primary monitor only the setting narrows nothing: every window has its app on the dock', async ({
    page,
  }) => {
    await openDock(page, PRIMARY_PAGE);
    await patchSettings(page, { monitors: 'primary', behavior: { showOnlyCurrentMonitor: true } });
    await expect
      .poll(async () => (await page.evaluate(SETTINGS))?.behavior.showOnlyCurrentMonitor)
      .toBe(true);
    await page.waitForTimeout(SLIDE_MS);
    // Paint and Notepad's window live on the second monitor, which has no dock.
    const everyApp = await page.evaluate(async () =>
      ((await (window as unknown as DemoWindow).__dockMock?.getDockState())?.items ?? [])
        .filter((i) => i.kind === 'app')
        .map((i) => i.key),
    );
    expect(everyApp).toContain('app:exe:c:/windows/system32/mspaint.exe');
    expect(await appKeys(page)).toEqual(everyApp);
    await expect(page.locator(PAINT_SLOT)).toHaveCount(1);
    await expect(dots(page, NOTEPAD_SLOT)).toHaveCount(1);

    // A dock per monitor: now it narrows.
    await patchSettings(page, { monitors: 'all' });
    await expect(page.locator(PAINT_SLOT)).toHaveCount(0);
    await expect(dots(page, NOTEPAD_SLOT)).toHaveCount(0);
  });

  test('a fullscreen app hides only the dock of its monitor', async ({ page }) => {
    await openDock(page, SECOND_PAGE);
    const panel = page.locator(PANEL);
    await expect(panel).not.toHaveAttribute('data-autohide');

    // Fullscreen on the primary, or on an unknown monitor (which the engine
    // gives to the primary's dock): this dock stays.
    await fullscreen(page, true, 'mock-primary');
    await page.waitForTimeout(SLIDE_MS * 2);
    await expect(panel).not.toHaveAttribute('data-autohide');
    await expect(panel).toBeVisible();
    await fullscreen(page, false);
    await fullscreen(page, true, null);
    await page.waitForTimeout(SLIDE_MS * 2);
    await expect(panel).not.toHaveAttribute('data-autohide');
    await fullscreen(page, false);

    // On this monitor: out at once, and back when it ends.
    await fullscreen(page, true, DISPLAY3);
    await expect(panel).toHaveAttribute('data-autohide', 'hidden');
    await expect(panel).toBeHidden();
    await fullscreen(page, false);
    await expect(panel).not.toHaveAttribute('data-autohide');
    await expect(panel).toBeVisible();
  });

  test('the primary page takes a fullscreen verdict with no monitor, not one about the other monitor', async ({
    page,
  }) => {
    await openDock(page, PRIMARY_PAGE);
    const panel = page.locator(PANEL);
    await fullscreen(page, true, DISPLAY3);
    await page.waitForTimeout(SLIDE_MS * 2);
    await expect(panel).not.toHaveAttribute('data-autohide');
    await expect(panel).toBeVisible();
    await fullscreen(page, false);
    await fullscreen(page, true, null);
    await expect(panel).toHaveAttribute('data-autohide', 'hidden');
    await fullscreen(page, false);
    await expect(panel).not.toHaveAttribute('data-autohide');
    await fullscreen(page, true, 'mock-primary');
    await expect(panel).toHaveAttribute('data-autohide', 'hidden');
  });

  test('"when a window overlaps" hides only the dock whose panel the window covers', async ({
    page,
  }) => {
    await openDock(page, SECOND_PAGE);
    const viewport = page.viewportSize()!;
    await expect.poll(() => hitRegions(page)).toHaveLength(1);
    const atRest = (await hitRegions(page))[0]!;
    const panel = page.locator(PANEL);

    // This page's panel was placed on the second monitor's work area, at
    // its scale: 1280 × 240 CSS px are 1920 × 360 physical px there.
    const onScreen = await dockPanel(page);
    expect(onScreen).not.toBeNull();
    expect(onScreen!.x).toBe(DISPLAY3_WORK_AREA.x + Math.floor(atRest.panel.x * DISPLAY3_SCALE));
    expect(onScreen!.y).toBe(
      DISPLAY3_WORK_AREA.y +
        DISPLAY3_WORK_AREA.height -
        Math.round(viewport.height * DISPLAY3_SCALE) +
        Math.floor(atRest.panel.y * DISPLAY3_SCALE),
    );
    expect(onScreen!.x + onScreen!.width).toBeLessThanOrEqual(
      DISPLAY3_WORK_AREA.x + DISPLAY3_WORK_AREA.width,
    );
    expect(onScreen!.y + onScreen!.height).toBeLessThanOrEqual(
      DISPLAY3_WORK_AREA.y + DISPLAY3_WORK_AREA.height,
    );

    await patchSettings(page, { autoHide: { mode: 'whenOverlapped', delayMs: DELAY } });
    await page.waitForTimeout(DELAY + SLIDE_MS);
    await expect(panel).not.toHaveAttribute('data-autohide');

    // Paint (on this monitor) is dragged down over the panel: the countdown
    // runs, then the dock slides out; dragged away, it is back at once.
    const over = { x: onScreen!.x - 100, y: onScreen!.y - 300, width: 900, height: 400 };
    await moveWindow(page, PAINT_WINDOW_ID, over);
    await expect(panel).toHaveAttribute('data-autohide', 'pending');
    await expect(panel).toHaveAttribute('data-autohide', 'hidden');
    await expect(panel).toBeHidden();
    await page.screenshot({ path: `${OUT}/dock-second-monitor-overlapped.png` });
    await moveWindow(page, PAINT_WINDOW_ID, PAINT_FRAME);
    await expect(panel).not.toHaveAttribute('data-autohide');
    await expect(panel).toBeVisible();

    // Code's window, dragged over the *primary's* panel area: nothing here.
    await moveWindow(page, CODE_WINDOW_ID, { x: 900, y: 1100, width: 900, height: 400 });
    await page.waitForTimeout(DELAY + SLIDE_MS);
    await expect(panel).not.toHaveAttribute('data-autohide');
    await expect(panel).toBeVisible();
  });

  test('the primary page is not hidden by a window over the secondary dock', async ({ page }) => {
    await openDock(page, PRIMARY_PAGE);
    const panel = page.locator(PANEL);
    const onScreen = (await dockPanel(page))!;
    expect(onScreen.x + onScreen.width).toBeLessThanOrEqual(WORK_AREA.width);
    await patchSettings(page, { autoHide: { mode: 'whenOverlapped', delayMs: DELAY } });
    // The frame that covers the second monitor's panel area.
    await moveWindow(page, PAINT_WINDOW_ID, { x: 3000, y: 800, width: 900, height: 300 });
    await page.waitForTimeout(DELAY + SLIDE_MS);
    await expect(panel).not.toHaveAttribute('data-autohide');
    await expect(panel).toBeVisible();
    // One over its own panel does hide it.
    await moveWindow(page, CODE_WINDOW_ID, {
      x: onScreen.x - 100,
      y: onScreen.y - 300,
      width: 900,
      height: 400,
    });
    await expect(panel).toHaveAttribute('data-autohide', 'hidden');
  });
});

test.describe('keyboard navigation', () => {
  const PANEL = '.dock-panel';
  const ITEMS = `${PANEL} > [role="button"]`;
  const FIREFOX = 'app:exe:c:/program files/mozilla firefox/firefox.exe';
  const SPOTIFY = 'app:exe:c:/program files/spotify/spotify.exe';
  const EXPLORER = 'app:exe:c:/windows/explorer.exe';
  const EXPLORER_WINDOW = 1;
  /** Long enough to tell "counting down" from "hidden" under Playwright's own latency. */
  const DELAY = 600;
  const SLIDE_MS = 200;

  /** What `spyPrimaryClick` adds to the page. */
  interface PrimarySpyWindow {
    __dockMock: { primaryClick: (itemKey: string) => Promise<void> };
    __primaryClicks: string[];
  }

  /** Record every `primaryClick` the dock makes (the mock itself records only its effect). */
  function spyPrimaryClick(page: Page): Promise<void> {
    return page.evaluate(() => {
      const w = window as unknown as PrimarySpyWindow;
      const mock = w.__dockMock;
      const original = mock.primaryClick.bind(mock);
      w.__primaryClicks = [];
      mock.primaryClick = (itemKey: string): Promise<void> => {
        w.__primaryClicks.push(itemKey);
        return original(itemKey);
      };
    });
  }

  function primaryClicks(page: Page): Promise<string[]> {
    return page.evaluate(() => [...(window as unknown as PrimarySpyWindow).__primaryClicks]);
  }

  /** Every `show_item_menu` request so far, oldest first. */
  function itemMenus(page: Page): Promise<Array<{ itemKey: string; x: number; y: number }>> {
    return page.evaluate(() =>
      ((window as unknown as DemoWindow).__dockMock?.calls ?? [])
        .filter((c) => c.type === 'showItemMenu')
        .map((c) => ({ itemKey: c.itemKey as string, x: c.x as number, y: c.y as number })),
    );
  }

  /** The focused item's key; `'panel'` for the panel itself; `null` when the focus is not in the dock. */
  function focused(page: Page): Promise<string | null> {
    return page.evaluate(() => {
      const active = document.activeElement;
      if (!(active instanceof HTMLElement) || active.closest('.dock-panel') === null) {
        return null;
      }
      return active.dataset.key ?? 'panel';
    });
  }

  /** Every item's key in toolbar order (no separators). */
  function itemKeys(page: Page): Promise<string[]> {
    return page
      .locator(ITEMS)
      .evaluateAll((els) => els.map((el) => (el as HTMLElement).dataset.key ?? ''));
  }

  /** The keys of the elements in the panel that Tab stops at. */
  function tabStops(page: Page): Promise<string[]> {
    return page
      .locator(`${PANEL} [tabindex="0"]`)
      .evaluateAll((els) => els.map((el) => (el as HTMLElement).dataset.key ?? 'panel'));
  }

  /** The slot of the item `key`. */
  function slot(page: Page, key: string): Locator {
    return page.locator(`${PANEL} > [data-key="${key}"]`);
  }

  /** Tab into the toolbar and arrow along to the item `key`. */
  async function focusItem(page: Page, key: string): Promise<void> {
    await page.keyboard.press('Tab');
    const keys = await itemKeys(page);
    expect(keys).toContain(key);
    await page.keyboard.press('Home');
    for (let i = 0; i < keys.length && (await focused(page)) !== key; i += 1) {
      await page.keyboard.press('ArrowRight');
    }
    expect(await focused(page)).toBe(key);
  }

  test('Tab reaches the toolbar once; arrows, Home and End move the focus past separators; it comes back where it was', async ({
    page,
  }) => {
    await openDock(page);
    const toolbar = page.getByRole('toolbar', { name: 'Stem' });
    await expect(toolbar).toHaveAttribute('aria-orientation', 'horizontal');
    const separators = page.locator(`${PANEL} > [role="separator"]`);
    await expect(separators).toHaveCount(3);
    for (let i = 0; i < 3; i += 1) {
      await expect(separators.nth(i)).toHaveAttribute('aria-orientation', 'vertical');
      await expect(separators.nth(i)).not.toHaveAttribute('tabindex');
    }
    const keys = await itemKeys(page);
    expect(keys).toHaveLength(9); // Start + 7 apps + Recycle Bin
    const first = keys[0]!;
    const last = keys.at(-1)!;
    expect(first).toBe('fixed:launcher');

    // One tab stop, the first item; Tab reaches it, and the next Tab leaves the dock.
    expect(await tabStops(page)).toEqual([first]);
    await page.keyboard.press('Tab');
    expect(await focused(page)).toBe(first);
    await page.keyboard.press('Tab');
    expect(await focused(page)).toBeNull();
    await page.keyboard.press('Shift+Tab');
    expect(await focused(page)).toBe(first);

    // ArrowRight visits every item once, never a separator, and stops at the end.
    const right: Array<string | null> = [];
    for (let i = 0; i < keys.length; i += 1) {
      await page.keyboard.press('ArrowRight');
      right.push(await focused(page));
    }
    expect(right).toEqual([...keys.slice(1), last]);
    // ArrowLeft goes back the same way and stops at the start.
    const left: Array<string | null> = [];
    for (let i = 0; i < keys.length; i += 1) {
      await page.keyboard.press('ArrowLeft');
      left.push(await focused(page));
    }
    expect(left).toEqual([...keys.slice(0, -1).reverse(), first]);
    // ArrowDown and ArrowUp move nothing on a horizontal dock: only the
    // arrows of its axis do (a dock on a side edge uses them).
    await page.keyboard.press('ArrowDown');
    expect(await focused(page)).toBe(first);
    await page.keyboard.press('ArrowUp');
    expect(await focused(page)).toBe(first);
    // End and Home.
    await page.keyboard.press('End');
    expect(await focused(page)).toBe(last);
    await page.keyboard.press('Home');
    expect(await focused(page)).toBe(first);

    // The tab stop follows the focus, and Tab out and back returns to it.
    await page.keyboard.press('ArrowRight');
    await page.keyboard.press('ArrowRight');
    expect(await focused(page)).toBe(keys[2]);
    expect(await tabStops(page)).toEqual([keys[2]]);
    await page.keyboard.press('Tab');
    expect(await focused(page)).toBeNull();
    await page.keyboard.press('Shift+Tab');
    expect(await focused(page)).toBe(keys[2]);
    // Nothing was magnified on the way.
    expect(await page.locator(`${PANEL} .slot[data-magnified]`).count()).toBe(0);
  });

  test('Enter and Space ask for the primary action; Shift+F10 and the menu key open the menu once, at the centre and top edge', async ({
    page,
  }) => {
    await openDock(page);
    await spyPrimaryClick(page);
    await focusItem(page, FIREFOX);
    await page.keyboard.press('Enter');
    await expect.poll(() => primaryClicks(page)).toEqual([FIREFOX]);
    await page.keyboard.press(' ');
    await expect.poll(() => primaryClicks(page)).toEqual([FIREFOX, FIREFOX]);
    expect(await focused(page)).toBe(FIREFOX);

    // Shift+F10: one native menu request for the item, at its slot's
    // horizontal centre and top edge (CSS px of the viewport).
    const box = (await slot(page, FIREFOX).boundingBox())!;
    await page.keyboard.press('Shift+F10');
    await expect.poll(() => callsOfType(page, 'showItemMenu')).toBe(1);
    const [request] = await itemMenus(page);
    expect(request!.itemKey).toBe(FIREFOX);
    expect(request!.x).toBeCloseTo(box.x + box.width / 2, 1);
    expect(request!.y).toBeCloseTo(box.y, 1);
    expect(request!.x).toBeGreaterThan(box.x);
    expect(request!.x).toBeLessThan(box.x + box.width);
    expect(request!.y).toBeGreaterThanOrEqual(box.y - 0.5);
    expect(request!.y).toBeLessThan(box.y + box.height);
    // The browser's own keyboard `contextmenu` did not ask a second time.
    await page.waitForTimeout(200);
    expect(await callsOfType(page, 'showItemMenu')).toBe(1);

    // The context menu key: the same (on Windows Chromium sends its own
    // `contextmenu` on the key's release, which the item swallows).
    await page.keyboard.press('ContextMenu');
    await expect.poll(() => callsOfType(page, 'showItemMenu')).toBe(2);
    await page.waitForTimeout(200);
    const menus = await itemMenus(page);
    expect(menus).toHaveLength(2);
    expect(menus[1]).toEqual(request);
    expect(await callsOfType(page, 'showDockMenu')).toBe(0);
    expect(await focused(page)).toBe(FIREFOX);
  });

  test('the focused item shows its title pill and focus ring without magnifying; the pointer pill wins; Escape lets go', async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await openDock(page);
    await page.mouse.move(640, 10);
    await focusItem(page, FIREFOX);
    const firefox = slot(page, FIREFOX);
    const pill = page.locator('.tooltip');
    await expect(pill).toHaveCount(1);
    await expect(firefox.locator('.tooltip')).toBeVisible();
    const title = (await firefox.locator('.tooltip').textContent()) ?? '';
    expect(title.length).toBeGreaterThan(0);
    expect((await firefox.getAttribute('aria-label'))?.startsWith(`${title},`)).toBe(true);
    // The focus ring is on the icon; nothing is magnified.
    expect(await computed(firefox.locator('.icon'), 'outline-style')).toBe('solid');
    expect(await firefox.evaluate((el) => el.style.getPropertyValue('--scale') || '1')).toBe('1');
    await expect(firefox).not.toHaveAttribute('data-magnified');
    await page.screenshot({ path: `${OUT}/dock-keyboard-focus.png` });

    // The pill follows the focus.
    await page.keyboard.press('ArrowRight');
    const next = (await focused(page))!;
    expect(next).not.toBe(FIREFOX);
    await expect(slot(page, next).locator('.tooltip')).toBeVisible();
    await expect(pill).toHaveCount(1);

    // The pointer over another item: its pill replaces the focused one's;
    // the pointer gone, the focused item's pill is back.
    const keys = await itemKeys(page);
    const hovered = keys[keys.indexOf(next) + 2]!;
    await slot(page, hovered).hover();
    await expect(slot(page, hovered).locator('.tooltip')).toBeVisible();
    await expect(pill).toHaveCount(1);
    await page.mouse.move(640, 10);
    await expect(slot(page, next).locator('.tooltip')).toBeVisible();
    await expect(pill).toHaveCount(1);

    // Escape lets go of the item: no focus, no pill; the tab stop stays.
    await page.keyboard.press('Escape');
    expect(await focused(page)).toBeNull();
    await expect(pill).toHaveCount(0);
    expect(await tabStops(page)).toEqual([next]);

    // With the title pills turned off, focus shows none either.
    // (After the blur Chromium resumes from the item: Tab goes past it and
    // out of the page, Shift+Tab comes back to the tab stop.)
    await patchSettings(page, { appearance: { tooltips: false } });
    await page.keyboard.press('Tab');
    expect(await focused(page)).toBeNull();
    await page.keyboard.press('Shift+Tab');
    expect(await focused(page)).toBe(next);
    await page.waitForTimeout(150);
    await expect(pill).toHaveCount(0);
  });

  test('when the focused item leaves the dock the focus moves on to its neighbour, and lets go as usual', async ({
    page,
  }) => {
    // With motion the leaving slot lingers for its animation; without, it
    // goes at once (and Chromium may blur it on the way out).
    for (const reducedMotion of ['no-preference', 'reduce'] as const) {
      await page.emulateMedia({ reducedMotion });
      await openDock(page);
      await page.mouse.move(640, 10);
      const keys = await itemKeys(page);
      const neighbour = keys[keys.indexOf(CODE_KEY) + 1]!;
      await focusItem(page, CODE_KEY);
      // Code's only window closes: the (unpinned) item goes.
      await page.evaluate((id) => {
        (window as unknown as DemoWindow).__dockMock?.pushWindowRemoved(id);
      }, CODE_WINDOW_ID);
      await expect(
        page.locator(`${PANEL} > [data-key="${CODE_KEY}"]:not([data-leaving])`),
      ).toHaveCount(0);
      await expect.poll(() => focused(page)).toBe(neighbour);
      await expect(slot(page, CODE_KEY)).toHaveCount(0);
      expect(await tabStops(page)).toEqual([neighbour]);
      await expect(slot(page, neighbour).locator('.tooltip')).toBeVisible();
      // The hold moved with the focus and goes with it.
      await patchSettings(page, { autoHide: { mode: 'always', delayMs: 0 } });
      await page.waitForTimeout(SLIDE_MS);
      await expect(page.locator(PANEL)).toHaveAttribute('data-autohide', 'shown');
      await page.keyboard.press('Escape');
      await expect(page.locator(PANEL)).toHaveAttribute('data-autohide', 'hidden');
    }
  });

  test("labels say when all of an app's windows are minimised and when it needs attention", async ({
    page,
  }) => {
    await openDock(page);
    // Spotify's only window is minimised in the demo.
    await expect(slot(page, SPOTIFY)).toHaveAttribute('aria-label', /, 1 window, minimised$/);
    // Firefox has two windows, neither minimised, and is the active app.
    await expect(slot(page, FIREFOX)).toHaveAttribute('aria-label', /, 2 windows, active$/);
    await flash(page, EXPLORER_WINDOW);
    await expect(slot(page, EXPLORER)).toHaveAttribute('aria-label', /, needs attention$/);
  });

  test('with auto-hide always the dock stays up while an item has keyboard focus and hides after the delay once it leaves', async ({
    page,
  }) => {
    await openDock(page);
    const panel = page.locator(PANEL);
    await page.mouse.move(640, 10);
    await page.keyboard.press('Tab');
    expect(await focused(page)).toBe('fixed:launcher');
    await patchSettings(page, { autoHide: { mode: 'always', delayMs: DELAY } });
    await expect(panel).toHaveAttribute('data-autohide', 'shown');
    await page.waitForTimeout(DELAY + SLIDE_MS + 200);
    await expect(panel).toHaveAttribute('data-autohide', 'shown');
    // Moving between items keeps it up.
    await page.keyboard.press('ArrowRight');
    await page.waitForTimeout(DELAY + SLIDE_MS);
    await expect(panel).toHaveAttribute('data-autohide', 'shown');
    await expect(panel).toBeVisible();

    // Escape: the countdown starts, and the dock slides away after it.
    await page.keyboard.press('Escape');
    await expect(panel).toHaveAttribute('data-autohide', 'pending');
    await expect(panel).toHaveAttribute('data-autohide', 'hidden');
    await expect(panel).toBeHidden();
    // Hidden, the panel takes no focus, and Tab does not bring it back.
    await page.keyboard.press('Tab');
    expect(await focused(page)).toBeNull();
    await page.waitForTimeout(SLIDE_MS);
    await expect(panel).toHaveAttribute('data-autohide', 'hidden');

    // Revealed from the bottom edge and focused again, it stays when the
    // pointer leaves; Tab out of the dock starts the countdown.
    const strip = (await page.locator('.reveal-strip').boundingBox())!;
    await page.mouse.move(strip.x + strip.width / 2, strip.y + 1);
    await expect(panel).toHaveAttribute('data-autohide', 'shown');
    await expect(panel).toBeVisible();
    await page.keyboard.press('Shift+Tab');
    expect(await focused(page)).not.toBeNull();
    await page.mouse.move(640, 10);
    await page.waitForTimeout(DELAY + SLIDE_MS + 200);
    await expect(panel).toHaveAttribute('data-autohide', 'shown');
    await page.keyboard.press('Tab');
    expect(await focused(page)).toBeNull();
    await expect(panel).toHaveAttribute('data-autohide', 'pending');
    await expect(panel).toHaveAttribute('data-autohide', 'hidden');

    // A right-click is no keyboard focus, whatever it does to the focus:
    // its menu closed and the pointer gone, the dock hides as before, with
    // no pill left behind.
    await page.mouse.move(strip.x + strip.width / 2, strip.y + 1);
    await expect(panel).toHaveAttribute('data-autohide', 'shown');
    await slot(page, FIREFOX).click({ button: 'right' });
    await expect.poll(() => callsOfType(page, 'showItemMenu')).toBe(1);
    await page.mouse.move(640, 10);
    await expect(page.locator('.tooltip')).toHaveCount(0);
    await expect(panel).toHaveAttribute('data-autohide', 'hidden');
  });

  // --- The focus shortcut (docs/ipc/keyboard-entry.md) -----------------------------

  /** How often the page asked Rust to give the keyboard focus back. */
  const releases = (page: Page): Promise<number> => callsOfType(page, 'releaseDockFocus');

  /**
   * A press of the focus shortcut, as Rust tells the page about it: `dock://focus`, or
   * `dock://release-focus` while the dock still has the keyboard from the last press.
   */
  function pressFocusShortcut(page: Page): Promise<void> {
    return page.evaluate(() => {
      (window as unknown as DemoWindow).__dockMock?.requestFocus();
    });
  }

  /** The item `key` shows keyboard focus: the ring on its icon and its title pill. */
  async function expectKeyboardFocusOn(page: Page, key: string): Promise<void> {
    await expect.poll(() => focused(page)).toBe(key);
    const item = slot(page, key);
    expect(await item.evaluate((el) => el.matches(':focus-visible'))).toBe(true);
    expect(await computed(item.locator('.icon'), 'outline-style')).toBe('solid');
    await expect(item.locator('.tooltip')).toBeVisible();
    await expect(page.locator('.tooltip')).toHaveCount(1);
  }

  test("the focus shortcut puts the keyboard on the active app's item; arrows and Enter work from there; Escape gives the focus back", async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await openDock(page);
    await spyPrimaryClick(page);
    await page.mouse.move(640, 10);
    expect(await focused(page)).toBeNull();

    // Firefox is the active app in the demo: the keyboard lands on it, as
    // Tab would bring it there (ring, pill, nothing magnified).
    await pressFocusShortcut(page);
    await expectKeyboardFocusOn(page, FIREFOX);
    await expect(slot(page, FIREFOX)).not.toHaveAttribute('data-magnified');
    expect(await tabStops(page)).toEqual([FIREFOX]);
    await page.screenshot({ path: `${OUT}/dock-focus-shortcut.png` });

    // From there the toolbar's keys work as usual: an arrow to the next
    // item, Enter clicks it (and only it).
    const keys = await itemKeys(page);
    const next = keys[keys.indexOf(FIREFOX) + 1]!;
    await page.keyboard.press('ArrowRight');
    await expectKeyboardFocusOn(page, next);
    await page.keyboard.press('Enter');
    await expect.poll(() => primaryClicks(page)).toEqual([next]);
    expect(await releases(page)).toBe(0);

    // Escape lets go of the item and asks Rust, once, to give the focus
    // back to the window that was in front.
    await page.keyboard.press('Escape');
    expect(await focused(page)).toBeNull();
    await expect(page.locator('.tooltip')).toHaveCount(0);
    await expect.poll(() => releases(page)).toBe(1);
    await page.waitForTimeout(150);
    expect(await releases(page)).toBe(1);
    expect(await primaryClicks(page)).toEqual([next]);

    // Nothing in front (the desktop): the first item, Start, takes it.
    await foreground(page, null);
    await expect(slot(page, FIREFOX)).not.toHaveAttribute('aria-label', /, active$/);
    await pressFocusShortcut(page);
    await expectKeyboardFocusOn(page, keys[0]!);
    expect(keys[0]).toBe('fixed:launcher');
    await page.keyboard.press('Escape');
    await expect.poll(() => releases(page)).toBe(2);
    expect(await focused(page)).toBeNull();

    // Another app in front: its item, wherever the tab stop was.
    await foreground(page, CODE_WINDOW_ID);
    await expect(slot(page, CODE_KEY)).toHaveAttribute('aria-label', /, active$/);
    await pressFocusShortcut(page);
    await expectKeyboardFocusOn(page, CODE_KEY);
    await page.keyboard.press('Escape');
    await expect.poll(() => releases(page)).toBe(3);
  });

  test('after the pointer, the focus shortcut still shows the ring and the pill', async ({
    page,
  }) => {
    await openDock(page);
    // The mouse was used on the dock: a click (which never focuses an
    // item; here it brings the minimised Spotify to the front, so Spotify
    // is the active app now) and a right-click (which may focus one,
    // without :focus-visible).
    await spyPrimaryClick(page);
    await slot(page, SPOTIFY).click();
    await expect.poll(() => primaryClicks(page)).toEqual([SPOTIFY]);
    await expect(slot(page, SPOTIFY)).toHaveAttribute('aria-label', /, active$/);
    await slot(page, FIREFOX).click({ button: 'right' });
    await expect.poll(() => callsOfType(page, 'showItemMenu')).toBe(1);
    await page.mouse.move(640, 10);
    await expect(page.locator('.tooltip')).toHaveCount(0);

    await pressFocusShortcut(page);
    await expectKeyboardFocusOn(page, SPOTIFY);
    // A repeated `dock://focus` (a press that crossed a release on its way)
    // only puts the keyboard on the entry item again.
    await page.keyboard.press('ArrowRight');
    expect(await focused(page)).not.toBe(SPOTIFY);
    await page.evaluate(() => {
      const mock = (window as unknown as DemoWindow).__dockMock as unknown as {
        emit: (event: string, payload: null) => void;
      };
      mock.emit('dock://focus', null);
    });
    await expectKeyboardFocusOn(page, SPOTIFY);
    await page.keyboard.press('Escape');
    await expect.poll(() => releases(page)).toBe(1);
    expect(await primaryClicks(page)).toEqual([SPOTIFY]);
  });

  test('a second press of the focus shortcut lets go like Escape, also with nothing to go back to; the next press enters again', async ({
    page,
  }) => {
    await openDock(page);
    const panel = page.locator(PANEL);
    await spyPrimaryClick(page);
    await page.mouse.move(640, 10);
    await patchSettings(page, { autoHide: { mode: 'always', delayMs: DELAY } });
    await expect(panel).toHaveAttribute('data-autohide', 'hidden');
    const keys = await itemKeys(page);

    // From the desktop (nothing in front): the first item takes it.
    await foreground(page, null);
    await pressFocusShortcut(page);
    await expect(panel).toHaveAttribute('data-autohide', 'shown');
    await expectKeyboardFocusOn(page, keys[0]!);

    // The second press: the ring and the pill go and Rust is asked, once,
    // to give the focus back, as for Escape.
    await pressFocusShortcut(page);
    await expect.poll(() => focused(page)).toBeNull();
    await expect(page.locator('.tooltip')).toHaveCount(0);
    await expect.poll(() => releases(page)).toBe(1);
    await page.waitForTimeout(150);
    expect(await releases(page)).toBe(1);
    // The `focus` hold is gone: as after Escape, the dock stays until the
    // pointer has been over it and left, then slides away.
    const box = (await panel.boundingBox())!;
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await page.mouse.move(640, 10);
    await expect(panel).toHaveAttribute('data-autohide', 'pending');
    await expect(panel).toHaveAttribute('data-autohide', 'hidden');

    // The third press enters again, sliding the dock back in.
    await pressFocusShortcut(page);
    await expect(panel).toHaveAttribute('data-autohide', 'shown');
    await expectKeyboardFocusOn(page, keys[0]!);
    await page.keyboard.press('Escape');
    await expect.poll(() => releases(page)).toBe(2);

    // Enter on the active app's item minimises it, which leaves the dock
    // in front with the keyboard: the next press still lets go.
    await foreground(page, CODE_WINDOW_ID);
    await pressFocusShortcut(page);
    await expectKeyboardFocusOn(page, CODE_KEY);
    await page.keyboard.press('Enter');
    await expect.poll(() => primaryClicks(page)).toEqual([CODE_KEY]);
    await expect(slot(page, CODE_KEY)).not.toHaveAttribute('aria-label', /, active$/);
    expect(await focused(page)).toBe(CODE_KEY);
    await pressFocusShortcut(page);
    await expect.poll(() => focused(page)).toBeNull();
    await expect(page.locator('.tooltip')).toHaveCount(0);
    await expect.poll(() => releases(page)).toBe(3);
    expect(await primaryClicks(page)).toEqual([CODE_KEY]);
  });

  test('with auto-hide always, the focus shortcut brings a slid-out dock back and keeps it up until Escape', async ({
    page,
  }) => {
    await openDock(page);
    const panel = page.locator(PANEL);
    await page.mouse.move(640, 10);
    await patchSettings(page, { autoHide: { mode: 'always', delayMs: DELAY } });
    await expect(panel).toHaveAttribute('data-autohide', 'hidden');
    await expect(panel).toBeHidden();
    // The page told Rust its panel is slid out, so the press sends
    // `dock://show` before `dock://focus`.
    await expect
      .poll(() =>
        page.evaluate(
          () =>
            ((window as unknown as DemoWindow).__dockMock?.calls ?? [])
              .filter((c) => c.type === 'setDockAutoHidden')
              .at(-1)?.hidden,
        ),
      )
      .toBe(true);

    await pressFocusShortcut(page);
    await expect(panel).toHaveAttribute('data-autohide', 'shown');
    await expect(panel).toBeVisible();
    await expectKeyboardFocusOn(page, FIREFOX);
    // The `focus` hold keeps it up, pointer away, past the delay.
    await page.waitForTimeout(DELAY + SLIDE_MS + 200);
    await expect(panel).toHaveAttribute('data-autohide', 'shown');
    await page.keyboard.press('ArrowRight');
    await page.waitForTimeout(DELAY + SLIDE_MS);
    await expect(panel).toHaveAttribute('data-autohide', 'shown');

    // Escape gives the focus back; like any reveal on request, the dock
    // then stays until the pointer has been over it and left.
    await page.keyboard.press('Escape');
    await expect.poll(() => releases(page)).toBe(1);
    await expect(page.locator('.tooltip')).toHaveCount(0);
    const box = (await panel.boundingBox())!;
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
    await page.mouse.move(640, 10);
    await expect(panel).toHaveAttribute('data-autohide', 'pending');
    await expect(panel).toHaveAttribute('data-autohide', 'hidden');

    // A `dock://focus` alone, as when the press crossed the slide and Rust
    // had not heard of it yet, brings the panel back too.
    await page.evaluate(() => {
      const mock = (window as unknown as DemoWindow).__dockMock as unknown as {
        emit: (event: string, payload: null) => void;
      };
      mock.emit('dock://focus', null);
    });
    await expect(panel).toHaveAttribute('data-autohide', 'shown');
    await expectKeyboardFocusOn(page, FIREFOX);
    await page.keyboard.press('Escape');
    await expect.poll(() => releases(page)).toBe(2);
  });
});

test.describe('the dock on the left, right and top edges', () => {
  type Edge = 'bottom' | 'top' | 'left' | 'right';
  const PANEL = '.dock-panel';
  const STRIP = '.reveal-strip';
  const PINNED = '.slot[data-pinned="true"]';
  const GAP = '.drop-gap';
  /** A window on a side edge: as thick as the page asks for (341 CSS px), shown a little wider. */
  const SIDE_VIEWPORT = { width: 360, height: 800 };
  const TOP_VIEWPORT = { width: 1280, height: 240 };
  /** `EDGE_MARGIN`, `SIDE_HEADROOM` and `HEADROOM` of Dock.svelte; the demo's magnification. */
  const EDGE_MARGIN = 8;
  const SIDE_HEADROOM = 240;
  const HEADROOM = 48;
  const MAX_SCALE = 1.6;
  const RANGE_ICONS = 3;
  const SLOT = ICON + SLOT_GAP;
  const DELAY = 600;
  const SLIDE_MS = 200;

  const isVertical = (edge: Edge): boolean => edge === 'left' || edge === 'right';
  const viewportFor = (edge: Edge): { width: number; height: number } =>
    isVertical(edge) ? SIDE_VIEWPORT : TOP_VIEWPORT;

  async function openOn(page: Page, edge: Edge): Promise<void> {
    await page.setViewportSize(viewportFor(edge));
    await openDock(page, `mock=1&position=${edge}`);
  }

  interface Box {
    left: number;
    top: number;
    right: number;
    bottom: number;
    width: number;
    height: number;
  }

  const box = (locator: Locator): Promise<Box> =>
    locator.first().evaluate((el) => {
      const r = el.getBoundingClientRect();
      return {
        left: r.left,
        top: r.top,
        right: r.right,
        bottom: r.bottom,
        width: r.width,
        height: r.height,
      };
    });
  const centreOf = (b: Box): { x: number; y: number } => ({
    x: b.left + b.width / 2,
    y: b.top + b.height / 2,
  });
  const lastRegion = async (page: Page): Promise<HitRegionCall | undefined> =>
    (await hitRegions(page)).at(-1);
  const lastSize = async (page: Page): Promise<{ width: number; height: number } | undefined> => {
    const last = (await contentSizes(page)).at(-1);
    return last === undefined ? undefined : { width: last.width, height: last.height };
  };
  const lastPanelRect = (page: Page): Promise<Rect | undefined> =>
    page.evaluate(() =>
      ((window as unknown as DemoWindow).__dockMock?.calls ?? [])
        .filter((c) => c.type === 'setDockPanelRect')
        .map((c) => c.rect as Rect)
        .at(-1),
    );
  const pinnedKeys = (page: Page): Promise<string[]> =>
    page.$$eval(PINNED, (els) => els.map((el) => (el as HTMLElement).dataset.key ?? ''));
  const focusedKey = (page: Page): Promise<string | null> =>
    page.evaluate(() => (document.activeElement as HTMLElement | null)?.dataset.key ?? null);
  const fileDrag = (page: Page, event: FileDrag): Promise<void> =>
    page.evaluate((e) => {
      (window as unknown as DemoWindow).__dockMock?.simulateFileDrag(e);
    }, event);

  /** Somewhere in the window well away from the panel (the transparent part). */
  function awayFrom(edge: Edge): { x: number; y: number } {
    switch (edge) {
      case 'bottom':
        return { x: 640, y: 10 };
      case 'top':
        return { x: 640, y: 230 };
      case 'left':
        return { x: 350, y: 10 };
      case 'right':
        return { x: 10, y: 10 };
    }
  }

  /** The reveal strip for a panel resting at `panel`: 2 px of the window's edge side, as long as the panel. */
  function stripFor(edge: Edge, panel: Rect): Rect {
    const { width, height } = viewportFor(edge);
    switch (edge) {
      case 'bottom':
        return { x: panel.x, y: height - 2, width: panel.width, height: 2 };
      case 'top':
        return { x: panel.x, y: 0, width: panel.width, height: 2 };
      case 'left':
        return { x: 0, y: panel.y, width: 2, height: panel.height };
      case 'right':
        return { x: width - 2, y: panel.y, width: 2, height: panel.height };
    }
  }

  /**
   * The stock taskbar (or another app bar) on `side` of the mock monitor:
   * `get_monitors` then reports a work area 48 px short of the bounds there
   * (and none at the bottom), the way Windows does. Installed before the
   * page loads, like `withTaskbarAutoHidden`.
   */
  async function withTaskbarOn(page: Page, side: Edge): Promise<void> {
    await page.addInitScript((s) => {
      interface Monitors {
        getMonitors: () => Promise<MonitorInfo[]>;
      }
      let mock: Monitors | undefined;
      Object.defineProperty(window, '__dockMock', {
        configurable: true,
        get: () => mock,
        set: (value: Monitors) => {
          const original = value.getMonitors.bind(value);
          value.getMonitors = async () =>
            (await original()).map((m) => {
              const b = m.bounds;
              const bar = 48;
              const workArea =
                s === 'left'
                  ? { x: b.x + bar, y: b.y, width: b.width - bar, height: b.height }
                  : s === 'right'
                    ? { x: b.x, y: b.y, width: b.width - bar, height: b.height }
                    : s === 'top'
                      ? { x: b.x, y: b.y + bar, width: b.width, height: b.height - bar }
                      : { x: b.x, y: b.y, width: b.width, height: b.height - bar };
              return { ...m, workArea };
            });
          mock = value;
        },
      });
    }, side);
  }

  for (const edge of ['left', 'right', 'top'] as const) {
    const vertical = isVertical(edge);
    /** The arrow keys of the toolbar's axis: to the next item and back. */
    const [nextKey, previousKey] = vertical
      ? ['ArrowDown', 'ArrowUp']
      : ['ArrowRight', 'ArrowLeft'];
    const [crossKey] = vertical ? ['ArrowRight'] : ['ArrowDown'];
    /** The coordinate along the dock. */
    const along = (p: { x: number; y: number }): number => (vertical ? p.y : p.x);

    test.describe(`on ${edge}`, () => {
      test('at rest: on its edge side, centred along it, items in order with the dots towards the edge; the reports for that edge', async ({
        page,
      }) => {
        await page.emulateMedia({ colorScheme: 'dark' });
        await openOn(page, edge);
        const viewport = viewportFor(edge);
        const panel = page.locator(PANEL);
        await expect(panel).toHaveAttribute(
          'aria-orientation',
          vertical ? 'vertical' : 'horizontal',
        );
        await expect(page.locator('.separator').first()).toHaveAttribute(
          'aria-orientation',
          vertical ? 'horizontal' : 'vertical',
        );
        await expect.poll(() => lastPanelRect(page)).toBeDefined();
        const p = await box(panel);

        // Anchored to the window's edge side, EDGE_MARGIN px from it, and
        // centred along it (both within the half CSS px a snap may move it).
        const gapToEdge = {
          left: p.left,
          right: viewport.width - p.right,
          top: p.top,
          bottom: viewport.height - p.bottom,
        }[edge];
        expect(Math.abs(gapToEdge - EDGE_MARGIN)).toBeLessThanOrEqual(0.5);
        const length = vertical ? viewport.height : viewport.width;
        expect(Math.abs(along(centreOf(p)) - length / 2)).toBeLessThanOrEqual(1);
        // A row on top, a column on a side edge: one icon plus padding thick.
        expect(vertical ? p.width : p.height).toBe(ICON + 2 * PANEL_PADDING);

        // Items keep their order along the main axis: Start first, the bin last.
        const order = await page.$$eval(
          `${PANEL} > .slot`,
          (els, v) =>
            els.map((el) => {
              const r = el.getBoundingClientRect();
              return {
                kind: (el as HTMLElement).dataset.kind ?? '',
                at: v ? r.top + r.height / 2 : r.left + r.width / 2,
              };
            }),
          vertical,
        );
        expect(order[0]!.kind).toBe('launcher');
        expect(order.at(-1)!.kind).toBe('recycleBin');
        for (let i = 1; i < order.length; i += 1) {
          expect(order[i]!.at).toBeGreaterThan(order[i - 1]!.at);
        }

        // The running dots sit between the icon and the edge, inside the panel.
        const running = page
          .locator(APP_SLOT)
          .filter({ has: page.locator('.dots i') })
          .first();
        const icon = await box(running.locator('.icon'));
        const dot = await box(running.locator('.dots i'));
        switch (edge) {
          case 'left':
            expect(dot.right).toBeLessThanOrEqual(icon.left);
            expect(dot.left).toBeGreaterThanOrEqual(p.left);
            break;
          case 'right':
            expect(dot.left).toBeGreaterThanOrEqual(icon.right);
            expect(dot.right).toBeLessThanOrEqual(p.right);
            break;
          case 'top':
            expect(dot.bottom).toBeLessThanOrEqual(icon.top);
            expect(dot.top).toBeGreaterThanOrEqual(p.top);
            break;
        }
        expect(Math.abs(along(centreOf(dot)) - along(centreOf(icon)))).toBeLessThanOrEqual(1);

        // set_dock_content_size: the thickness across the edge (the height on
        // top, the width on a side edge), the length along it informational.
        const growth = (MAX_SCALE - 1) * SLOT * (RANGE_ICONS + 0.5);
        const thickness = Math.ceil(
          ICON +
            2 * PANEL_PADDING +
            EDGE_MARGIN +
            ICON * (MAX_SCALE - 1) +
            (vertical ? SIDE_HEADROOM : HEADROOM),
        );
        const size = (await lastSize(page))!;
        if (vertical) {
          expect(size).toEqual({ width: thickness, height: Math.ceil(p.height + 48 + growth) });
        } else {
          expect(size).toEqual({ width: Math.ceil(p.width + 48 + growth), height: thickness });
        }

        // The hit region: the panel, and an envelope lengthened along the
        // dock and grown away from the edge only.
        const region = (await lastRegion(page))!;
        const rest = {
          x: Math.floor(p.left),
          y: Math.floor(p.top),
          width: Math.ceil(p.right) - Math.floor(p.left),
          height: Math.ceil(p.bottom) - Math.floor(p.top),
        };
        expect(region.panel).toEqual(rest);
        expect(contains(region.envelope, region.panel)).toBe(true);
        const e = region.envelope;
        const r = region.panel;
        switch (edge) {
          case 'left':
            expect(e.x).toBe(r.x);
            expect(e.x + e.width).toBeGreaterThan(r.x + r.width);
            break;
          case 'right':
            expect(e.x + e.width).toBe(r.x + r.width);
            expect(e.x).toBeLessThan(r.x);
            break;
          case 'top':
            expect(e.y).toBe(r.y);
            expect(e.y + e.height).toBeGreaterThan(r.y + r.height);
            break;
        }
        if (vertical) {
          expect(e.y).toBeLessThan(r.y);
          expect(e.y + e.height).toBeGreaterThan(r.y + r.height);
        } else {
          expect(e.x).toBeLessThan(r.x);
          expect(e.x + e.width).toBeGreaterThan(r.x + r.width);
        }

        // The resting panel, and where the app layer (the mock's conversion)
        // puts it on the monitor: the window flush with its edge of the work
        // area, which on the right starts 360 px from the work area's right.
        expect(await lastPanelRect(page)).toEqual(rest);
        const onScreen =
          edge === 'right' ? { ...rest, x: WORK_AREA.width - viewport.width + rest.x } : rest;
        await expect.poll(() => dockPanel(page)).toEqual(onScreen);

        await page.screenshot({ path: `${OUT}/dock-edge-${edge}-rest.png` });
      });

      test('magnifies along the dock, the icons growing away from the edge, with the title pill on the far side', async ({
        page,
      }) => {
        await page.emulateMedia({ colorScheme: 'dark' });
        await openOn(page, edge);
        const viewport = viewportFor(edge);
        const panel = page.locator(PANEL);
        await expect.poll(() => lastPanelRect(page)).toBeDefined();
        const rest = await box(panel);
        const slot = page.locator(APP_SLOT).nth(3);
        const at = centreOf(await box(slot));
        // Off the icon's centre across the dock: only the position along it counts.
        await page.mouse.move(vertical ? at.x + 10 : at.x, vertical ? at.y : at.y + 10);
        await expect.poll(() => scaleOf(page, 3)).toBeGreaterThan(1.55);
        // Its neighbours are scaled the same on either side.
        expect(Math.abs((await scaleOf(page, 2)) - (await scaleOf(page, 4)))).toBeLessThan(0.1);
        expect(await scaleOf(page, 2)).toBeGreaterThan(1);

        // The icon keeps its side towards the edge and grows away from it.
        const icon = await box(slot.locator('.icon'));
        expect(icon.width).toBeGreaterThan(ICON * 1.5);
        switch (edge) {
          case 'left':
            expect(Math.abs(icon.left - (rest.left + PANEL_PADDING))).toBeLessThanOrEqual(0.5);
            expect(icon.right).toBeGreaterThan(rest.right);
            break;
          case 'right':
            expect(Math.abs(icon.right - (rest.right - PANEL_PADDING))).toBeLessThanOrEqual(0.5);
            expect(icon.left).toBeLessThan(rest.left);
            break;
          case 'top':
            expect(Math.abs(icon.top - (rest.top + PANEL_PADDING))).toBeLessThanOrEqual(0.5);
            expect(icon.bottom).toBeGreaterThan(rest.bottom);
            break;
        }
        // The panel lengthens along the dock only.
        const grown = await box(panel);
        if (vertical) {
          expect(grown.height).toBeGreaterThan(rest.height + SLOT);
          expect(grown.width).toBeCloseTo(rest.width, 1);
        } else {
          expect(grown.width).toBeGreaterThan(rest.width + SLOT);
          expect(grown.height).toBeCloseTo(rest.height, 1);
        }

        // The title pill: beyond the magnified icon on the side away from
        // the edge, centred on it, and inside the window.
        const tooltip = page.locator('.tooltip');
        await expect(tooltip).toBeVisible();
        const t = await box(tooltip);
        switch (edge) {
          case 'left':
            expect(t.left).toBeGreaterThan(icon.right);
            break;
          case 'right':
            expect(t.right).toBeLessThan(icon.left);
            break;
          case 'top':
            expect(t.top).toBeGreaterThan(icon.bottom);
            break;
        }
        expect(Math.abs(along(centreOf(t)) - along(centreOf(icon)))).toBeLessThanOrEqual(1);
        expect(t.left).toBeGreaterThanOrEqual(0);
        expect(t.top).toBeGreaterThanOrEqual(0);
        expect(t.right).toBeLessThanOrEqual(viewport.width);
        expect(t.bottom).toBeLessThanOrEqual(viewport.height);
        // Nothing was measured or reported mid-magnification.
        const reported = (await contentSizes(page)).length;
        await page.screenshot({ path: `${OUT}/dock-edge-${edge}-magnified.png` });

        // Away: back to rest, and the reports are the resting ones.
        await page.mouse.move(awayFrom(edge).x, awayFrom(edge).y);
        await expect.poll(() => scaleOf(page, 3)).toBe(1);
        await expect(tooltip).toHaveCount(0);
        expect((await contentSizes(page)).length).toBe(reported);
      });

      test('the keyboard moves along the dock with its own arrows; the focused item shows its pill on the far side', async ({
        page,
      }) => {
        await page.emulateMedia({ colorScheme: 'dark' });
        await openOn(page, edge);
        const keys = await page.$$eval(`${PANEL} > .slot:not([data-kind="separator"])`, (els) =>
          els.map((el) => (el as HTMLElement).dataset.key ?? ''),
        );
        await page.keyboard.press('Tab');
        expect(await focusedKey(page)).toBe(keys[0]);
        await page.keyboard.press(nextKey);
        expect(await focusedKey(page)).toBe(keys[1]);
        await page.keyboard.press(nextKey);
        await page.keyboard.press(nextKey);
        expect(await focusedKey(page)).toBe(keys[3]);
        // The arrows across the dock move nothing.
        await page.keyboard.press(crossKey);
        expect(await focusedKey(page)).toBe(keys[3]);
        await page.keyboard.press(previousKey);
        expect(await focusedKey(page)).toBe(keys[2]);
        await page.keyboard.press('End');
        expect(await focusedKey(page)).toBe(keys.at(-1));
        await page.keyboard.press('Home');
        await page.keyboard.press(nextKey);
        await page.keyboard.press(nextKey);

        // The focused item's pill, without magnification, away from the edge.
        const slot = page.locator(`${PANEL} > .slot[data-key="${keys[2]}"]`);
        const tooltip = page.locator('.tooltip');
        await expect(tooltip).toBeVisible();
        expect(await slot.evaluate((el) => el.style.getPropertyValue('--scale') || '1')).toBe('1');
        const icon = await box(slot.locator('.icon'));
        const t = await box(tooltip);
        switch (edge) {
          case 'left':
            expect(t.left).toBeGreaterThan(icon.right);
            break;
          case 'right':
            expect(t.right).toBeLessThan(icon.left);
            break;
          case 'top':
            expect(t.top).toBeGreaterThan(icon.bottom);
            break;
        }
        await page.screenshot({ path: `${OUT}/dock-edge-${edge}-pill.png` });
      });

      test('auto-hide slides it out towards its edge; the strip along the edge side brings it back; a toast waits at the edge', async ({
        page,
      }) => {
        await page.emulateMedia({ colorScheme: 'dark' });
        await openOn(page, edge);
        const viewport = viewportFor(edge);
        const panel = page.locator(PANEL);
        await expect.poll(() => lastRegion(page)).toBeDefined();
        const atRest = (await lastRegion(page))!;
        const away = awayFrom(edge);
        await page.mouse.move(away.x, away.y);
        await patchSettings(page, { autoHide: { mode: 'always', delayMs: DELAY } });
        await expect(panel).toHaveAttribute('data-autohide', 'pending');
        await expect(panel).toHaveAttribute('data-autohide', 'hidden');
        await expect(panel).toBeHidden();
        await page.waitForTimeout(SLIDE_MS + 100);

        // Out past the window's edge side.
        const out = await box(panel);
        switch (edge) {
          case 'left':
            expect(out.right).toBeLessThanOrEqual(0);
            break;
          case 'right':
            expect(out.left).toBeGreaterThanOrEqual(viewport.width);
            break;
          case 'top':
            expect(out.bottom).toBeLessThanOrEqual(0);
            break;
        }
        // Only the 2 px strip along the edge side, as long as the resting panel, takes the pointer.
        const strip = stripFor(edge, atRest.panel);
        await expect.poll(() => lastRegion(page)).toEqual({ panel: strip, envelope: strip });
        expect(await page.locator(STRIP).boundingBox()).toEqual(strip);
        // The resting panel is still what the app layer knows.
        expect(await lastPanelRect(page)).toEqual(atRest.panel);

        // A command that fails meanwhile: its toast sits just inside the edge side.
        await page.evaluate(
          ([selector, message]) => {
            const mock = (window as unknown as DemoWindow).__dockMock!;
            mock.failNextCommand(message);
            document
              .querySelector(selector)!
              .dispatchEvent(new PointerEvent('pointerup', { button: 0, bubbles: true }));
          },
          [`${APP_SLOT}[data-key="${CODE_KEY}"]`, 'window not found: 4'] as const,
        );
        const toast = page.getByRole('status');
        await expect(toast).toHaveText('Code failed: window not found: 4');
        const toastGap = async (): Promise<number> => {
          const t = await box(toast);
          return { left: t.left, right: viewport.width - t.right, top: t.top }[edge];
        };
        await expect.poll(toastGap).toBeLessThanOrEqual(12);
        expect(await toastGap()).toBeGreaterThan(0);
        await page.screenshot({ path: `${OUT}/dock-edge-${edge}-hidden.png` });

        // The pointer on the strip: back in, the envelope out to the edge side.
        const c = { x: strip.x + strip.width / 2, y: strip.y + strip.height / 2 };
        await page.mouse.move(c.x, c.y);
        await expect(panel).toHaveAttribute('data-autohide', 'shown');
        await expect(panel).toBeVisible();
        await expect
          .poll(async () => await panel.evaluate((el) => getComputedStyle(el).transform))
          .toBe('none');
        const shown = (await lastRegion(page))!;
        expect(shown.panel).toEqual(atRest.panel);
        expect(contains(shown.envelope, atRest.envelope)).toBe(true);
        expect(contains(shown.envelope, strip)).toBe(true);
        // The toast rode back out with the panel, beyond its far side.
        const p = await box(panel);
        await expect
          .poll(async () => {
            const t = await box(toast);
            return { left: t.left >= p.right, right: t.right <= p.left, top: t.top >= p.bottom }[
              edge
            ];
          })
          .toBe(true);
        await page.mouse.move(away.x, away.y);
        await expect(panel).toHaveAttribute('data-autohide', 'hidden');
      });

      test('a fullscreen app leaves the strip on this edge unless a taskbar holds that side', async ({
        page,
      }) => {
        // The mock's monitor has its taskbar at the bottom: this edge reaches the screen.
        await openOn(page, edge);
        const panel = page.locator(PANEL);
        await expect.poll(() => lastRegion(page)).toBeDefined();
        const atRest = (await lastRegion(page))!;
        const away = awayFrom(edge);
        await page.mouse.move(away.x, away.y);
        await fullscreen(page, true);
        await expect(panel).toHaveAttribute('data-autohide', 'hidden');
        const strip = stripFor(edge, atRest.panel);
        await expect.poll(() => lastRegion(page)).toEqual({ panel: strip, envelope: strip });
        await expect(page.locator(STRIP)).toHaveCount(1);
        await fullscreen(page, false);
        await expect(panel).not.toHaveAttribute('data-autohide');
        await expect.poll(() => lastRegion(page)).toEqual(atRest);

        // The taskbar on this edge: in fullscreen alone, no strip and no region.
        await withTaskbarOn(page, edge);
        await openOn(page, edge);
        await expect.poll(() => lastRegion(page)).toBeDefined();
        await page.mouse.move(away.x, away.y);
        await fullscreen(page, true);
        await expect(page.locator(PANEL)).toHaveAttribute('data-autohide', 'hidden');
        const none = { x: 0, y: 0, width: 0, height: 0 };
        await expect.poll(() => lastRegion(page)).toEqual({ panel: none, envelope: none });
        await expect(page.locator(STRIP)).toHaveCount(0);
        // The tray or the shortcut still brings it back.
        await page.evaluate(() => {
          (window as unknown as DemoWindow).__dockMock?.requestShow();
        });
        await expect(page.locator(PANEL)).toHaveAttribute('data-autohide', 'shown');
      });

      test('drag to reorder works along the dock', async ({ page }) => {
        await page.emulateMedia({ colorScheme: 'dark' });
        await openOn(page, edge);
        await expect.poll(() => lastSize(page)).toBeDefined();
        const restSize = (await lastSize(page))!;
        const before = await pinnedKeys(page);
        expect(before).toHaveLength(5);
        const a = centreOf(await box(page.locator(PINNED).nth(0)));
        const b = centreOf(await box(page.locator(PINNED).nth(2)));
        await page.mouse.move(a.x, a.y);
        await page.mouse.down();
        // Past the third pinned item along the dock, a little off the row across it.
        const away = { left: 12, right: -12, top: 12 }[edge];
        const to = vertical ? { x: a.x + away, y: b.y + 6 } : { x: b.x + 6, y: a.y + away };
        await page.mouse.move(to.x, to.y, { steps: 12 });
        const transformOf = (index: number): Promise<string> =>
          page
            .locator(PINNED)
            .nth(index)
            .evaluate((el) => (el as HTMLElement).style.transform);
        await expect(page.locator(PINNED).nth(0)).toHaveAttribute('data-drag', 'lifted');
        // The two items between the origin and the gap shifted one slot back along the dock.
        const shift = vertical ? 'translateY(-56px)' : 'translateX(-56px)';
        await expect.poll(() => transformOf(1)).toBe(shift);
        await expect.poll(() => transformOf(2)).toBe(shift);
        expect(await transformOf(3)).toBe('');
        // The window is asked to grow across the edge for the drag; the
        // length along it is unchanged, and the whole window takes clicks.
        await expect
          .poll(() => lastSize(page))
          .toEqual(
            vertical
              ? { width: 4096, height: restSize.height }
              : { width: restSize.width, height: 4096 },
          );
        const viewport = viewportFor(edge);
        await expect
          .poll(async () => (await lastRegion(page))?.envelope)
          .toEqual({ x: 0, y: 0, ...viewport });
        // The neighbours' 160 ms shift has ended: the gap is open in the picture.
        await page.waitForTimeout(250);
        await page.screenshot({ path: `${OUT}/dock-edge-${edge}-drag.png` });

        await page.mouse.up();
        await expect
          .poll(() => pinnedKeys(page))
          .toEqual([before[1], before[2], before[0], before[3], before[4]]);
        await expect.poll(() => transformOf(2)).toBe('');
        await expect(page.locator('[data-drag]')).toHaveCount(0);
        expect(await callsOfType(page, 'launch')).toBe(0);
        await expect.poll(() => lastSize(page)).toEqual(restSize);
      });

      test('a program dragged from Explorer opens the gap along the dock and is pinned there', async ({
        page,
      }) => {
        await openOn(page, edge);
        await expect.poll(() => lastRegion(page)).toBeDefined();
        const atRest = (await lastRegion(page))!;
        const keys = await pinnedKeys(page);
        const a = await box(page.locator(PINNED).nth(1));
        const b = await box(page.locator(PINNED).nth(2));
        const between = vertical
          ? { x: centreOf(a).x, y: (centreOf(a).y + centreOf(b).y) / 2 }
          : { x: (centreOf(a).x + centreOf(b).x) / 2, y: centreOf(a).y };
        const EXE_PATH = 'C:\\Tools\\PuTTY\\putty.exe';
        await fileDrag(page, { type: 'enter', paths: [EXE_PATH], position: between });
        const gap = page.locator(GAP);
        await expect(gap).toHaveCount(1);
        // One slot long along the dock, one icon thick, after the second pinned item.
        await expect
          .poll(async () => {
            const g = await box(gap);
            return Math.round(vertical ? g.height : g.width);
          })
          .toBe(SLOT);
        const g = await box(gap);
        expect(Math.round(vertical ? g.width : g.height)).toBe(ICON);
        expect(
          await gap.evaluate(
            (el) => (el.previousElementSibling as HTMLElement | null)?.dataset.key,
          ),
        ).toBe(keys[1]);
        // The envelope is one slot longer along the dock, half a slot at each end.
        const during = (await lastRegion(page))!;
        const longer = vertical
          ? during.envelope.height - atRest.envelope.height
          : during.envelope.width - atRest.envelope.width;
        expect(Math.abs(longer - SLOT)).toBeLessThanOrEqual(1);
        expect(during.panel).toEqual(atRest.panel);

        await fileDrag(page, { type: 'drop', paths: [EXE_PATH], position: between });
        const id = `exe:${EXE_PATH.replace(/\\/g, '/').toLowerCase()}`;
        await expect
          .poll(() => pinnedKeys(page))
          .toEqual([keys[0], keys[1], `app:${id}`, keys[2], keys[3], keys[4]]);
        await expect(gap).toHaveCount(0);
      });
    });
  }

  // The item's own motion and its keyboard menu follow the edge (DockItem's
  // `edge`): bottom is the reference the other three are measured against.
  for (const edge of ['bottom', 'top', 'left', 'right'] as const) {
    const vertical = isVertical(edge);
    const nextKey = vertical ? 'ArrowDown' : 'ArrowRight';
    const NOTEPAD = 'app:exe:c:/windows/notepad.exe';
    const EXPLORER_WINDOW = 1;
    /** The bounce's first and highest keyframe (DockItem.svelte): 42 % of the icon at offset 0.17. */
    const PEAK = 0.42 * ICON;

    test(`on ${edge}: a launch bounces away from the edge, attention wiggles along the dock, the keyboard menu opens on the side away from the edge`, async ({
      page,
    }) => {
      await openOn(page, edge);
      await expect.poll(() => lastPanelRect(page)).toBeDefined();
      const slot = page.locator(`${PANEL} > .slot[data-key="${NOTEPAD}"]`);
      await page.keyboard.press('Tab');
      for (let i = 0; i < 12 && (await focusedKey(page)) !== NOTEPAD; i += 1) {
        await page.keyboard.press(nextKey);
      }
      expect(await focusedKey(page)).toBe(NOTEPAD);

      // Shift+F10: the middle of the slot's side away from the edge.
      const s = await box(slot);
      await page.keyboard.press('Shift+F10');
      await expect.poll(() => callsOfType(page, 'showItemMenu')).toBe(1);
      const menu = await page.evaluate(() =>
        ((window as unknown as DemoWindow).__dockMock?.calls ?? [])
          .filter((c) => c.type === 'showItemMenu')
          .map((c) => ({ itemKey: c.itemKey as string, x: c.x as number, y: c.y as number }))
          .at(-1),
      );
      expect(menu!.itemKey).toBe(NOTEPAD);
      const anchor = {
        bottom: { x: s.left + s.width / 2, y: s.top },
        top: { x: s.left + s.width / 2, y: s.bottom },
        left: { x: s.right, y: s.top + s.height / 2 },
        right: { x: s.left, y: s.top + s.height / 2 },
      }[edge];
      expect(menu!.x).toBeCloseTo(anchor.x, 1);
      expect(menu!.y).toBeCloseTo(anchor.y, 1);

      // Enter launches it (pinned, not running): the bounce, paused at rest
      // and at its first peak, moves the icon away from the edge only.
      await page.keyboard.press('Enter');
      await expect.poll(() => callsOfType(page, 'launch')).toBe(1);
      const bouncer = slot.locator('.bouncer');
      // Asserted, not annotated: TypeScript does not see the poll assign it.
      let bounce = null as { dx: number; dy: number } | null;
      await expect
        .poll(async () => {
          bounce = await bouncer.evaluate((el) => {
            // The Web Animations bounce: neither a CSS transition (the lift) nor a CSS animation.
            const a = el
              .getAnimations()
              .find((x) => !(x instanceof CSSTransition) && !(x instanceof CSSAnimation));
            if (a === undefined) {
              return null;
            }
            a.pause();
            const duration = Number(a.effect?.getComputedTiming().duration ?? 0);
            a.currentTime = 0;
            const rest = el.getBoundingClientRect();
            a.currentTime = 0.17 * duration;
            const peak = el.getBoundingClientRect();
            a.finish();
            return { dx: peak.left - rest.left, dy: peak.top - rest.top };
          });
          return bounce !== null;
        })
        .toBe(true);
      const { dx, dy } = bounce!;
      const expected = {
        bottom: { dx: 0, dy: -PEAK },
        top: { dx: 0, dy: PEAK },
        left: { dx: PEAK, dy: 0 },
        right: { dx: -PEAK, dy: 0 },
      }[edge];
      expect(dx).toBeCloseTo(expected.dx, 1);
      expect(dy).toBeCloseTo(expected.dy, 1);

      // Attention: the wiggle's first swing (4 px back at 5 % of its 2 s)
      // runs along the dock, never towards or away from the edge.
      await flash(page, EXPLORER_WINDOW);
      const wiggler = page.locator(
        `${APP_SLOT}[data-key="app:exe:c:/windows/explorer.exe"] .wiggler`,
      );
      await expect(wiggler).toHaveClass(/attention/);
      let swing = null as { dx: number; dy: number } | null;
      await expect
        .poll(async () => {
          swing = await wiggler.evaluate((el) => {
            const a = el.getAnimations().find((x) => x instanceof CSSAnimation);
            if (a === undefined) {
              return null;
            }
            a.pause();
            a.currentTime = 0;
            const rest = el.getBoundingClientRect();
            a.currentTime = 100;
            const first = el.getBoundingClientRect();
            a.play();
            return { dx: first.left - rest.left, dy: first.top - rest.top };
          });
          return swing !== null;
        })
        .toBe(true);
      const sw = swing!;
      expect(sw.dx).toBeCloseTo(vertical ? 0 : -4, 1);
      expect(sw.dy).toBeCloseTo(vertical ? -4 : 0, 1);
    });
  }

  // Report provenance (docs/ipc/report-provenance.md): Rust drops a
  // report naming another edge than its window's and re-anchors the panel
  // rect from the viewport it carries, so both must be the page's own.
  for (const edge of ['bottom', 'top', 'left', 'right'] as const) {
    test(`on ${edge}: every geometry report names the edge the page laid out for, the panel rect its viewport`, async ({
      page,
    }) => {
      await openOn(page, edge);
      await expect.poll(() => lastPanelRect(page)).toBeDefined();
      await expect(page.locator('[data-edge]').first()).toHaveAttribute('data-edge', edge);
      // A setting that changes the thickness only: another round of reports.
      const mark = await callCount(page);
      await patchSettings(page, { magnification: { enabled: true, maxScale: 2, rangeIcons: 3 } });
      await expect.poll(async () => (await geometryReports(page, mark)).length).toBeGreaterThan(0);

      const reports = await geometryReports(page);
      expect(new Set(reports.map((r) => r.type))).toEqual(
        new Set(['setDockContentSize', 'setDockHitRegion', 'setDockPanelRect']),
      );
      for (const report of reports) {
        expect(report.edge, report.type).toBe(edge);
      }
      const viewport = viewportFor(edge);
      const panels = await panelRects(page);
      expect(panels.length).toBeGreaterThan(0);
      for (const panel of panels) {
        expect(panel.viewport).toEqual(viewport);
        expect(contains({ x: 0, y: 0, ...viewport }, panel.rect)).toBe(true);
      }
      // The mock converts each report from its viewport, so its panel on
      // screen is the one of the latest report.
      const rest = panels.at(-1)!.rect;
      const onScreen =
        edge === 'right'
          ? { ...rest, x: WORK_AREA.width - viewport.width + rest.x }
          : edge === 'bottom'
            ? { ...rest, y: WORK_AREA.height - viewport.height + rest.y }
            : rest;
      expect(await dockPanel(page)).toEqual(onScreen);
    });
  }

  test('a position change: the reports name the old edge before it and the new one after it', async ({
    page,
  }) => {
    await openDock(page);
    await expect.poll(() => lastPanelRect(page)).toBeDefined();
    const before = await geometryReports(page);
    expect(before.length).toBeGreaterThan(0);
    expect(before.every((r) => r.edge === 'bottom')).toBe(true);

    for (const [edge, size] of [
      ['left', SIDE_VIEWPORT],
      ['top', TOP_VIEWPORT],
      ['right', SIDE_VIEWPORT],
      ['bottom', TOP_VIEWPORT],
    ] as const) {
      const mark = await callCount(page);
      await patchSettings(page, { position: edge });
      await page.setViewportSize(size);
      await expect(page.locator('[data-edge]').first()).toHaveAttribute('data-edge', edge);
      await expect
        .poll(async () => (await panelRects(page)).at(-1)?.viewport)
        .toEqual({ width: size.width, height: size.height });
      const since = await geometryReports(page, mark);
      expect(new Set(since.map((r) => r.type))).toEqual(
        new Set(['setDockContentSize', 'setDockHitRegion', 'setDockPanelRect']),
      );
      for (const report of since) {
        expect(report.edge, `${report.type} after the move to ${edge}`).toBe(edge);
      }
      expect((await panelRects(page)).at(-1)?.edge).toBe(edge);
    }
  });

  test('a position change at runtime lays the page out again without a reload', async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await openDock(page);
    await expect.poll(() => lastPanelRect(page)).toBeDefined();
    const bottom = {
      size: await lastSize(page),
      region: await lastRegion(page),
      panel: await lastPanelRect(page),
    };
    await page.evaluate(() => {
      (window as unknown as { __marker?: number }).__marker = 402;
    });
    const panel = page.locator(PANEL);

    // To the left: Rust moves the window to the left edge (here: the viewport
    // turns into a side window); the page lays out vertically for it.
    await patchSettings(page, { position: 'left' });
    await page.setViewportSize(SIDE_VIEWPORT);
    await expect(panel).toHaveAttribute('aria-orientation', 'vertical');
    await expect
      .poll(async () => (await lastSize(page))?.width)
      .toBe(
        Math.ceil(ICON + 2 * PANEL_PADDING + EDGE_MARGIN + ICON * (MAX_SCALE - 1) + SIDE_HEADROOM),
      );
    await expect
      .poll(async () => Math.abs((await box(panel)).left - EDGE_MARGIN))
      .toBeLessThanOrEqual(0.5);
    await expect.poll(async () => (await lastRegion(page))?.panel.x).toBe(EDGE_MARGIN);
    // The bottom edge's snap margin is gone; only margins of the left edge remain.
    expect(await panel.evaluate((el) => el.style.marginBottom)).toBe('');
    await page.screenshot({ path: `${OUT}/dock-edge-switched-left.png` });

    // To the top, then back to the bottom: the same reports as at the start.
    await patchSettings(page, { position: 'top' });
    await page.setViewportSize(TOP_VIEWPORT);
    await expect(panel).toHaveAttribute('aria-orientation', 'horizontal');
    await expect.poll(async () => (await lastRegion(page))?.panel.y).toBe(EDGE_MARGIN);
    await patchSettings(page, { position: 'bottom' });
    await expect.poll(() => lastRegion(page)).toEqual(bottom.region);
    await expect.poll(() => lastSize(page)).toEqual(bottom.size);
    await expect.poll(() => lastPanelRect(page)).toEqual(bottom.panel);
    expect(await panel.evaluate((el) => el.style.marginTop)).toBe('');
    // The same page all along.
    expect(await page.evaluate(() => (window as unknown as { __marker?: number }).__marker)).toBe(
      402,
    );
  });

  // The settings change reaches several effects at once (the magnification's
  // and the work area's report too, and ending a file drag reports the hit
  // region): none of them may send the bottom row's box, or a thickness
  // made from it, under the new edge.
  for (const during of ['at rest', 'during a file drag'] as const) {
    test(`a position change ${during} reports nothing measured on the old edge`, async ({
      page,
    }) => {
      await openDock(page);
      await expect.poll(() => lastPanelRect(page)).toBeDefined();
      if (during === 'during a file drag') {
        await expect.poll(() => lastRegion(page)).toBeDefined();
        const atRest = (await lastRegion(page))!;
        const a = await box(page.locator(PINNED).nth(1));
        const b = await box(page.locator(PINNED).nth(2));
        await fileDrag(page, {
          type: 'enter',
          paths: ['C:\\Tools\\PuTTY\\putty.exe'],
          position: { x: (centreOf(a).x + centreOf(b).x) / 2, y: centreOf(a).y },
        });
        await expect(page.locator(GAP)).toHaveCount(1);
        // The drag's own region (one slot longer) has gone out on the bottom edge.
        await expect
          .poll(async () => (await lastRegion(page))!.envelope.width - atRest.envelope.width)
          .toBeGreaterThanOrEqual(SLOT - 1);
      }
      const mark = await page.evaluate(
        () => ((window as unknown as DemoWindow).__dockMock?.calls ?? []).length,
      );

      await patchSettings(page, { position: 'left' });
      await page.setViewportSize(SIDE_VIEWPORT);
      await expect(page.locator(PANEL)).toHaveAttribute('aria-orientation', 'vertical');
      const thickness = Math.ceil(
        ICON + 2 * PANEL_PADDING + EDGE_MARGIN + ICON * (MAX_SCALE - 1) + SIDE_HEADROOM,
      );
      await expect.poll(async () => (await lastSize(page))?.width).toBe(thickness);
      await expect.poll(async () => (await lastRegion(page))?.panel.x).toBe(EDGE_MARGIN);
      await expect.poll(async () => (await lastPanelRect(page))?.x).toBe(EDGE_MARGIN);
      type Report = { type: string; edge: unknown } & ({ rect: Rect } | { width: number });
      const since = await page.evaluate(
        (m) =>
          ((window as unknown as DemoWindow).__dockMock?.calls ?? [])
            .slice(m)
            .flatMap((c): Report[] => {
              switch (c.type) {
                case 'setDockHitRegion':
                  return [{ type: c.type, edge: c.edge, rect: c.panel as Rect }];
                case 'setDockPanelRect':
                  return [{ type: c.type, edge: c.edge, rect: c.rect as Rect }];
                case 'setDockContentSize':
                  return [{ type: c.type, edge: c.edge, width: c.width as number }];
                default:
                  return [];
              }
            }),
        mark,
      );
      expect(since.map((c) => c.type)).toEqual(
        expect.arrayContaining(['setDockContentSize', 'setDockHitRegion', 'setDockPanelRect']),
      );
      for (const call of since) {
        // Named for the edge it was laid out for.
        expect(call.edge, call.type).toBe('left');
        if ('rect' in call) {
          // A column 8 px from the left side, not the row centred along the bottom.
          expect(call.rect.x, call.type).toBe(EDGE_MARGIN);
          expect(call.rect.height, call.type).toBeGreaterThan(call.rect.width);
        } else {
          expect(call.width, call.type).toBe(thickness);
        }
      }
    });
  }
});

test.describe('theme presets and the user dock.css', () => {
  const PANEL = '.dock-panel';
  const STAGE = '.dock-stage';
  const SEPARATOR = '[role="separator"]';
  const DOT = `${APP_SLOT} .dots i`;
  const PRESETS: readonly ThemePreset[] = ['standard', 'solid', 'minimal'];
  const DOCK_CSS_PATH = 'C:\\Users\\me\\AppData\\Roaming\\dev.davidcristian.stem\\dock.css';
  /** A light-mode accent that is not Windows blue, to show the dots follow it in every preset. */
  const LIGHT_ACCENT = '#C239B3';
  const LIGHT_ACCENT_RGB = 'rgb(194, 57, 179)';
  const DARK_ACCENT_RGB = 'rgb(0, 120, 212)';

  interface Look {
    fill: string;
    blur: string;
    hairline: string;
    separator: string;
    pill: string;
  }

  /** What each preset draws, per colour mode: the computed colours of Dock.svelte's preset rules. */
  const LOOK: Record<'dark' | 'light', Record<ThemePreset, Look>> = {
    dark: {
      standard: {
        fill: 'rgba(28, 28, 30, 0.72)',
        blur: 'blur(24px) saturate(1.4)',
        hairline: 'rgba(255, 255, 255, 0.12)',
        separator: 'rgba(255, 255, 255, 0.22)',
        pill: 'rgba(40, 40, 42, 0.94)',
      },
      solid: {
        fill: 'rgb(44, 44, 44)',
        blur: 'none',
        hairline: 'rgba(117, 117, 117, 0.4)',
        separator: 'rgba(255, 255, 255, 0.28)',
        pill: 'rgb(44, 44, 44)',
      },
      minimal: {
        fill: 'rgba(0, 0, 0, 0)',
        blur: 'none',
        hairline: 'rgba(0, 0, 0, 0)',
        separator: 'rgba(255, 255, 255, 0.5)',
        pill: 'rgba(40, 40, 42, 0.94)',
      },
    },
    light: {
      standard: {
        fill: 'rgba(244, 244, 246, 0.72)',
        blur: 'blur(24px) saturate(1.4)',
        hairline: 'rgba(0, 0, 0, 0.1)',
        separator: 'rgba(0, 0, 0, 0.18)',
        pill: 'rgba(250, 250, 252, 0.96)',
      },
      solid: {
        fill: 'rgb(249, 249, 249)',
        blur: 'none',
        hairline: 'rgba(117, 117, 117, 0.4)',
        separator: 'rgba(0, 0, 0, 0.22)',
        pill: 'rgb(249, 249, 249)',
      },
      minimal: {
        fill: 'rgba(0, 0, 0, 0)',
        blur: 'none',
        hairline: 'rgba(0, 0, 0, 0)',
        separator: 'rgba(0, 0, 0, 0.4)',
        pill: 'rgba(250, 250, 252, 0.96)',
      },
    },
  };

  /** The user's dock.css of the screenshot: a variable of every kind changed but the blur. */
  const USER_CSS = [
    '.dock-stage {',
    '  --stem-panel-fill: linear-gradient(135deg, rgb(0 120 212 / 0.55), rgb(136 23 152 / 0.55));',
    '  --stem-panel-hairline: rgb(255 255 255 / 0.4);',
    '  --stem-panel-shadow: 0 6px 18px rgb(0 0 0 / 0.5);',
    '  --stem-panel-radius: 10px;',
    '  --stem-separator: rgb(255 255 255 / 0.6);',
    '  --stem-dot: #ffb900;',
    '  --stem-pill-fill: #ffb900;',
    '  --stem-pill-text: #1b1b1f;',
    '}',
    '',
  ].join('\n');

  function setPreset(page: Page, theme: ThemePreset): Promise<void> {
    return patchSettings(page, { appearance: { theme } });
  }

  /** The dock.css "on disk" changes and Settings → Reload is pressed: `dock://user-css` reaches the page. */
  function reloadWith(page: Page, css: string | null): Promise<UserCss | undefined> {
    return page.evaluate(async (c) => {
      const mock = (window as unknown as DemoWindow).__dockMock!;
      mock.writeUserCss(c);
      return mock.reloadUserCss();
    }, css);
  }

  /**
   * A dock.css that is there when Stem starts: the mock's file is written
   * and read the moment the page exposes the mock (before mounting, so
   * before the page's `get_user_css`), which leaves the mock where
   * `MockOptions.userCss` would. Installed before the page loads, like
   * `withTaskbarAutoHidden`.
   */
  async function withUserCssAtStart(page: Page, css: string): Promise<void> {
    await page.addInitScript((c) => {
      interface UserCssMock {
        writeUserCss: (css: string | null) => void;
        reloadUserCss: () => Promise<unknown>;
      }
      let mock: UserCssMock | undefined;
      Object.defineProperty(window, '__dockMock', {
        configurable: true,
        get: () => mock,
        set: (value: UserCssMock) => {
          value.writeUserCss(c);
          void value.reloadUserCss();
          mock = value;
        },
      });
    }, css);
  }

  /** The panel's box (CSS px). */
  function panelBox(page: Page): Promise<Rect> {
    return page.locator(PANEL).evaluate((el) => {
      const r = el.getBoundingClientRect();
      return { x: r.x, y: r.y, width: r.width, height: r.height };
    });
  }

  async function expectLook(
    page: Page,
    scheme: 'dark' | 'light',
    preset: ThemePreset,
    accent: string,
  ): Promise<void> {
    const look = LOOK[scheme][preset];
    const panel = page.locator(PANEL);
    await expect(page.locator(STAGE)).toHaveAttribute('data-theme', preset);
    await expect(panel).toHaveCSS('background-color', look.fill);
    await expect(panel).toHaveCSS('backdrop-filter', look.blur);
    expect(await computed(panel, 'border-top-color', '::before')).toBe(look.hairline);
    await expect(page.locator(SEPARATOR).first()).toHaveCSS('background-color', look.separator);
    await expect(page.locator(DOT).first()).toHaveCSS('background-color', accent);
    if (preset === 'minimal') {
      await expect(panel).toHaveCSS('box-shadow', 'none');
    } else {
      await expect(panel).not.toHaveCSS('box-shadow', 'none');
    }
  }

  for (const scheme of ['dark', 'light'] as const) {
    const accent = scheme === 'dark' ? DARK_ACCENT_RGB : LIGHT_ACCENT_RGB;

    test(`at the bottom in ${scheme} mode: each preset draws its panel, the dots keep the accent, nothing moves`, async ({
      page,
    }) => {
      await page.emulateMedia({ colorScheme: scheme });
      await openDock(page);
      if (scheme === 'light') {
        await setOsTheme(page, { dark: false, accent: LIGHT_ACCENT });
      }
      await expect(page.locator('html')).toHaveAttribute('data-theme', scheme);
      const atRest = await panelBox(page);
      for (const preset of PRESETS) {
        // Back at rest after the last hover's magnification...
        await page.mouse.move(640, 10);
        await expect.poll(() => panelBox(page)).toEqual(atRest);
        await setPreset(page, preset);
        await expectLook(page, scheme, preset, accent);
        // ...the panel keeps its box: a preset change moves no icon.
        expect(await panelBox(page)).toEqual(atRest);
        await page.locator(APP_SLOT).nth(3).hover();
        const pill = page.locator('.tooltip');
        await expect(pill).toHaveText('notepad');
        await expect(pill).toHaveCSS('background-color', LOOK[scheme][preset].pill);
        await page.screenshot({ path: `${OUT}/dock-theme-${preset}-${scheme}.png` });
      }
    });

    test(`on the left in ${scheme} mode: each preset draws its panel along the edge`, async ({
      page,
    }) => {
      await page.emulateMedia({ colorScheme: scheme });
      await page.setViewportSize({ width: 360, height: 800 });
      await openDock(page, 'mock=1&position=left');
      if (scheme === 'light') {
        await setOsTheme(page, { dark: false, accent: LIGHT_ACCENT });
      }
      await expect(page.locator(STAGE)).toHaveAttribute('data-edge', 'left');
      const atRest = await panelBox(page);
      for (const preset of PRESETS) {
        await page.mouse.move(350, 10);
        await expect.poll(() => panelBox(page)).toEqual(atRest);
        await setPreset(page, preset);
        await expectLook(page, scheme, preset, accent);
        expect(await panelBox(page)).toEqual(atRest);
        await page.locator(APP_SLOT).nth(3).hover();
        await expect(page.locator('.tooltip')).toHaveText('notepad');
        await page.screenshot({ path: `${OUT}/dock-theme-${preset}-left-${scheme}.png` });
      }
    });
  }

  test('a contrast theme draws the contrast panel whatever the preset; Transparency effects off leaves solid opaque', async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await openDock(page);
    const panel = page.locator(PANEL);
    await setPreset(page, 'minimal');
    await setOsTheme(page, { highContrast: true });
    await expect(page.locator('html')).toHaveAttribute('data-high-contrast', '');
    await expect(panel).toHaveCSS('background-color', 'rgb(28, 28, 30)');
    await expect(panel).toHaveCSS('backdrop-filter', 'none');
    await expect(panel).toHaveCSS('box-shadow', 'none');
    expect(await computed(panel, 'border-top-width', '::before')).toBe('2px');
    expect(await computed(panel, 'border-top-color', '::before')).toBe('rgb(255, 255, 255)');
    await expect(page.locator(SEPARATOR).first()).toHaveCSS(
      'background-color',
      'rgb(255, 255, 255)',
    );
    await setPreset(page, 'solid');
    await expect(page.locator(STAGE)).toHaveAttribute('data-theme', 'solid');
    await expect(panel).toHaveCSS('background-color', 'rgb(28, 28, 30)');

    // Out of the contrast theme, with Transparency effects off: solid is
    // opaque anyway; standard is nearly opaque; minimal still has
    // no fill.
    await setOsTheme(page, { transparency: false });
    await expectLook(page, 'dark', 'solid', DARK_ACCENT_RGB);
    await setPreset(page, 'standard');
    await expect(panel).toHaveCSS('background-color', 'rgba(28, 28, 30, 0.95)');
    await setPreset(page, 'minimal');
    await expectLook(page, 'dark', 'minimal', DARK_ACCENT_RGB);
  });

  test('under a contrast theme the user\u2019s blur and hairline still apply, on the 2 px contrast line', async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await openDock(page);
    const panel = page.locator(PANEL);
    await setOsTheme(page, { highContrast: true });
    await expect(page.locator('html')).toHaveAttribute('data-high-contrast', '');
    await expect(panel).toHaveCSS('backdrop-filter', 'none');

    await reloadWith(
      page,
      '.dock-stage { --stem-panel-blur: blur(12px); --stem-panel-hairline: rgb(255, 185, 0); }',
    );
    await expect(panel).toHaveCSS('backdrop-filter', 'blur(12px)');
    expect(await computed(panel, 'border-top-color', '::before')).toBe('rgb(255, 185, 0)');
    expect(await computed(panel, 'border-top-width', '::before')).toBe('2px');
    // The variables it does not set keep the contrast values.
    await expect(panel).toHaveCSS('background-color', 'rgb(28, 28, 30)');
    await expect(panel).toHaveCSS('box-shadow', 'none');

    // Without the file: the contrast panel, unblurred, in the text colour.
    await reloadWith(page, null);
    await expect(panel).toHaveCSS('backdrop-filter', 'none');
    expect(await computed(panel, 'border-top-color', '::before')).toBe('rgb(255, 255, 255)');
    expect(await computed(panel, 'border-top-width', '::before')).toBe('2px');
  });

  test('a dock.css rule keyed on the preset or the colour mode is measured and reported again when they change', async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await openDock(page);
    // A padding for a preset: the content box (the slots) stays, the border
    // box and the panel's place change. An offset for a colour mode: the
    // panel moves and keeps its size, so no ResizeObserver sees it.
    await reloadWith(
      page,
      [
        '.dock-stage[data-theme="minimal"] .dock-panel { padding: 2px; }',
        'html[data-theme="light"] .dock-stage[data-theme="standard"] .dock-panel { top: -12px; }',
      ].join('\n'),
    );
    const viewport = { width: 1280, height: 240 };

    /** The latest three reports agree with the panel's box on screen now. */
    async function expectReportsOf(box: Rect, size: { width: number; height: number }) {
      const near = (a: Rect | undefined): boolean =>
        a !== undefined &&
        Math.abs(a.x - box.x) <= 1 &&
        Math.abs(a.y - box.y) <= 1 &&
        Math.abs(a.width - box.width) <= 1 &&
        Math.abs(a.height - box.height) <= 1;
      await expect.poll(async () => near((await panelRects(page)).at(-1)?.rect)).toBe(true);
      const panelRect = (await panelRects(page)).at(-1);
      expect(panelRect?.edge).toBe('bottom');
      expect(panelRect?.viewport).toEqual(viewport);
      await expect.poll(async () => near((await hitRegions(page)).at(-1)?.panel)).toBe(true);
      expect((await contentSizes(page)).at(-1)).toEqual(size);
    }

    const standard = await panelBox(page);
    const standardSize = (await contentSizes(page)).at(-1)!;

    // The preset: smaller by the padding it takes off on every side.
    await setPreset(page, 'minimal');
    await expect.poll(async () => (await panelBox(page)).width).toBeLessThan(standard.width);
    const minimal = await panelBox(page);
    expect(minimal.height).toBeLessThan(standard.height);
    await expect
      .poll(async () => (await contentSizes(page)).at(-1)?.width)
      .toBeLessThan(standardSize.width);
    const minimalSize = (await contentSizes(page)).at(-1)!;
    expect(minimalSize.height).toBeLessThan(standardSize.height);
    await expectReportsOf(minimal, minimalSize);

    // Back to standard: the panel's own padding again.
    await setPreset(page, 'standard');
    await expect.poll(() => panelBox(page)).toEqual(standard);
    await expectReportsOf(standard, standardSize);

    // The colour mode: 12 px further from the edge in light mode, the same
    // size, so the same content size.
    await setOsTheme(page, { dark: false });
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'light');
    await expect.poll(() => panelBox(page)).toEqual({ ...standard, y: standard.y - 12 });
    await expectReportsOf({ ...standard, y: standard.y - 12 }, standardSize);
  });

  test('a dock.css there at start styles every stable variable, after the page\u2019s own styles', async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await withUserCssAtStart(page, USER_CSS);
    await openDock(page);
    const panel = page.locator(PANEL);
    const style = page.locator('style#stem-user-css');
    await expect(style).toHaveCount(1);
    expect(await style.textContent()).toBe(USER_CSS);
    // The last element of <head>: after Vite's own stylesheets.
    expect(await page.evaluate(() => document.head.lastElementChild?.id)).toBe('stem-user-css');

    await expect(panel).toHaveCSS('background-image', /^linear-gradient\(/);
    await expect(panel).toHaveCSS('border-top-left-radius', '10px');
    await expect(panel).toHaveCSS('box-shadow', 'rgba(0, 0, 0, 0.5) 0px 6px 18px 0px');
    expect(await computed(panel, 'border-top-color', '::before')).toBe('rgba(255, 255, 255, 0.4)');
    expect(await computed(panel, 'border-top-left-radius', '::before')).toBe('10px');
    await expect(page.locator(SEPARATOR).first()).toHaveCSS(
      'background-color',
      'rgba(255, 255, 255, 0.6)',
    );
    await expect(page.locator(DOT).first()).toHaveCSS('background-color', 'rgb(255, 185, 0)');
    // The blur is the preset's: not one of the variables this file sets.
    await expect(panel).toHaveCSS('backdrop-filter', 'blur(24px) saturate(1.4)');
    // The accent stays the accent outside the dots.
    expect(await computed(page.locator('.slot[data-kind="launcher"] svg rect'), 'fill')).toBe(
      DARK_ACCENT_RGB,
    );
    await page.locator(APP_SLOT).nth(3).hover();
    const pill = page.locator('.tooltip');
    await expect(pill).toHaveCSS('background-color', 'rgb(255, 185, 0)');
    await expect(pill).toHaveCSS('color', 'rgb(27, 27, 31)');
    await page.screenshot({ path: `${OUT}/dock-user-css.png` });

    // The variables override every preset alike.
    await page.mouse.move(640, 10);
    await setPreset(page, 'minimal');
    await expect(panel).toHaveCSS('background-image', /^linear-gradient\(/);
    await expect(page.locator(DOT).first()).toHaveCSS('background-color', 'rgb(255, 185, 0)');
  });

  test('Reload replaces the one style element; a variable set on :root applies; no file, a bad rule, markup, a file too large', async ({
    page,
  }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await openDock(page);
    const panel = page.locator(PANEL);
    const dot = page.locator(DOT).first();
    const style = page.locator('style#stem-user-css');
    await expect(style).toHaveCount(0);
    await expect(dot).toHaveCSS('background-color', DARK_ACCENT_RGB);

    // Set on :root, levels above the dots: still applies, since the page
    // sets no --stem-* variable of its own.
    const root = ':root { --stem-dot: rgb(16, 124, 16); --stem-panel-radius: 4px; }';
    expect(await reloadWith(page, root)).toEqual({ path: DOCK_CSS_PATH, css: root });
    await expect(dot).toHaveCSS('background-color', 'rgb(16, 124, 16)');
    await expect(panel).toHaveCSS('border-top-left-radius', '4px');
    await expect(style).toHaveCount(1);

    // A new text replaces the old one in the same element.
    await reloadWith(page, '.dock-stage[data-theme="standard"] { --stem-dot: rgb(232, 17, 35); }');
    await expect(style).toHaveCount(1);
    await expect(dot).toHaveCSS('background-color', 'rgb(232, 17, 35)');
    await expect(panel).toHaveCSS('border-top-left-radius', '22px');
    // That rule names the standard preset only.
    await setPreset(page, 'solid');
    await expect(dot).toHaveCSS('background-color', DARK_ACCENT_RGB);
    await setPreset(page, 'standard');
    await expect(dot).toHaveCSS('background-color', 'rgb(232, 17, 35)');

    // A broken rule is skipped by the browser; the next one still applies.
    await reloadWith(
      page,
      '%%% { --stem-dot: red; } .dock-stage { color: ; --stem-dot: rgb(255, 185, 0); }',
    );
    await expect(dot).toHaveCSS('background-color', 'rgb(255, 185, 0)');
    await expect(page.locator('.dock-panel[data-ready="true"]')).toBeVisible();

    // Markup is text: nothing is parsed out of it, no script runs.
    await reloadWith(page, '</style><script>window.__ran = true</script><style>');
    expect(await page.evaluate(() => (window as unknown as { __ran?: boolean }).__ran)).toBe(
      undefined,
    );
    expect(await page.locator('head script:not([src])').count()).toBe(0);
    await expect(style).toHaveCount(1);

    // No file: the element goes and the preset's look is back.
    await reloadWith(page, null);
    await expect(style).toHaveCount(0);
    await expect(dot).toHaveCSS('background-color', DARK_ACCENT_RGB);

    // Larger than 64 KiB: ignored whole, like Rust does.
    const big = `.dock-stage { --stem-dot: rgb(255, 185, 0); }\n/*${'x'.repeat(64 * 1024)}*/`;
    expect((await reloadWith(page, big))?.css).toBeNull();
    await expect(style).toHaveCount(0);
    await expect(dot).toHaveCSS('background-color', DARK_ACCENT_RGB);
  });

  test.describe('in the settings window', () => {
    test.use({ viewport: { width: 760, height: 600 } });

    async function openSettings(page: Page): Promise<void> {
      await page.goto('/settings.html?mock=1');
      await expect(page.locator('.page[data-ready="true"]')).toBeVisible();
      await expect(page.locator('.status')).toHaveText('Saved');
      await expect(page.locator('.pin:not([data-icon="loaded"])')).toHaveCount(0);
    }

    function userCssCalls(page: Page): Promise<string[]> {
      return page.evaluate(() =>
        ((window as unknown as DemoWindow).__dockMock?.calls ?? [])
          .filter((c) => c.type === 'openUserCss' || c.type === 'reloadUserCss')
          .map((c) => c.type),
      );
    }

    test('Theme offers the three presets and saves; opacity is for Standard only; the dock.css row edits and reloads', async ({
      page,
    }) => {
      await page.emulateMedia({ colorScheme: 'dark' });
      await openSettings(page);
      const preset = page.locator('#appearance-theme');
      await expect(page.locator('label[for="appearance-theme"]')).toContainText('Theme');
      await expect(page.locator('label[for="theme"]')).toContainText('Colour mode');
      await expect(preset).toHaveValue('standard');
      await expect(preset.locator('option')).toHaveText([
        'Standard (translucent)',
        'Solid (opaque)',
        'Minimal (icons only)',
      ]);
      const opacity = page.locator('#opacity');
      await expect(opacity).toBeEnabled();

      await preset.selectOption('solid');
      await expect(page.locator('.status')).toHaveText('Saved');
      expect(
        (await page.evaluate(() => (window as unknown as DemoWindow).__dockMock?.getSettings()))
          ?.appearance.theme,
      ).toBe('solid');
      await expect(opacity).toBeDisabled();
      await expect(page.locator('#opacity-description')).toHaveText(
        'Only the Standard theme is translucent',
      );

      const row = page.locator('[data-row="user-css"]');
      await expect(row.locator('code')).toHaveText(DOCK_CSS_PATH);
      const status = row.locator('[data-user-css-status]');
      await expect(status).toContainText('Not in use');

      // Edit creates the file from the template; nothing is read yet.
      await page.getByRole('button', { name: 'Edit dock.css' }).click();
      await expect.poll(() => userCssCalls(page)).toEqual(['openUserCss']);
      expect(
        await page.evaluate(() => (window as unknown as DemoWindow).__dockMock?.userCssOnDisk()),
      ).toContain('dock.css');
      await expect(status).toContainText('Not in use');

      // The user saves the file; Reload reads it.
      await page.evaluate(() => {
        (window as unknown as DemoWindow).__dockMock?.writeUserCss(
          '.dock-stage { --stem-dot: #ffb900; }',
        );
      });
      await page.getByRole('button', { name: 'Reload dock.css' }).click();
      await expect(status).toHaveText('In use since the last start or Reload');
      await expect.poll(() => userCssCalls(page)).toEqual(['openUserCss', 'reloadUserCss']);
      await expect(page.locator('.file-error')).toHaveCount(0);
      // Neither is a settings change.
      await expect(page.locator('.status')).toHaveText('Saved');
      await page.locator('#appearance-theme').scrollIntoViewIfNeeded();
      await page.screenshot({ path: `${OUT}/settings-themes.png` });
      await row.scrollIntoViewIfNeeded();
      await page.screenshot({ path: `${OUT}/settings-user-css.png` });

      // A refusal from Rust is the row's own notice: here the one it gives
      // when neither the default app for `.css` nor Notepad opens the file.
      const refusal =
        'could not open dock.css with its default app (no application is associated) or with Notepad (the system cannot find the file specified)';
      await page.evaluate((message) => {
        (window as unknown as DemoWindow).__dockMock?.failNextCommand(message);
      }, refusal);
      await page.getByRole('button', { name: 'Edit dock.css' }).click();
      await expect(page.locator('.file-error')).toHaveText(`Couldn't open dock.css: ${refusal}`);
      await expect(page.locator('.status')).toHaveText('Saved');
      // The next attempt clears it.
      await page.getByRole('button', { name: 'Reload dock.css' }).click();
      await expect(page.locator('.file-error')).toHaveCount(0);
    });
  });
});
