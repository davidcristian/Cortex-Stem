import { mkdirSync } from 'node:fs';

import { expect, test, type Locator, type Page } from '@playwright/test';

import type { RawWindow } from '../src/lib/ipc/generated/RawWindow';
import type { Rect } from '../src/lib/ipc/generated/Rect';
import type { Settings } from '../src/lib/ipc/generated/Settings';
import type { ThemeInfo } from '../src/lib/ipc/generated/ThemeInfo';

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
    updateSettings: (settings: Settings) => Promise<Settings>;
    pushWindow: (raw: RawWindow) => void;
    pushWindowRemoved: (id: number) => void;
    pushForeground: (id: number | null) => void;
    /** `HSHELL_FLASH` as the platform reports it (`WindowAttention`). */
    pushAttention: (id: number, flashing: boolean) => void;
    /** `dock://theme`: what Windows reports for accent, mode, contrast and transparency. */
    setTheme: (theme: ThemeInfo) => void;
  };
}

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

/** Every `set_dock_content_size` the mock recorded so far, oldest first. */
function contentSizes(page: Page): Promise<Array<{ width: number; height: number }>> {
  return page.evaluate(
    () =>
      ((window as unknown as DemoWindow).__dockMock?.calls ?? []).filter(
        (c) => c.type === 'setDockContentSize',
      ) as unknown as Array<{ width: number; height: number }>,
  );
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
      await expect(button).toHaveAttribute('tabindex', '0');
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
      await expect(slot).toHaveAttribute('tabindex', '0');
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

  test('reduced motion fades instead of sliding; "when overlapped" acts as off for now', async ({
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

    // "When a window overlaps" needs window geometry the platform does not
    // report yet (ADR-0008): it reveals the dock and never hides it.
    await setAutoHide(page, 'whenOverlapped', 0);
    await expect(panel).not.toHaveAttribute('data-autohide');
    await expect(panel).toBeVisible();
    await expect.poll(() => lastRegion(page)).toEqual(atRest);
    await page.waitForTimeout(SLIDE_MS * 2);
    await expect(panel).toBeVisible();
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

    // Disabled, not-yet-available options stay disabled.
    await expect(page.locator('#current-monitor')).toBeDisabled();
    await expect(page.locator('#position option[value="left"]')).toBeDisabled();
    await expect(page.locator('#auto-hide option[value="whenOverlapped"]')).toBeDisabled();
  });

  test('auto-hide can be turned on; "when overlapped" is not there yet and the notes say so', async ({
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
    await expect(overlapped).toBeDisabled();
    await expect(overlapped).toHaveText(/acts as Off/);
    await expect(page.locator('#auto-hide-description')).toContainText('bottom edge below it');
    await expect(page.locator('#auto-hide-description')).toContainText('click-through on');

    await mode.selectOption('always');
    await expect.poll(async () => (await saved())?.mode).toBe('always');
    await expect(page.locator('.status')).toHaveText('Saved');

    // The delay is in milliseconds, 50 at a time from the keyboard.
    const delay = page.locator('#auto-hide-delay');
    await expect(page.locator('output[for="auto-hide-delay"]')).toHaveText('250 ms');
    await delay.focus();
    await page.keyboard.press('ArrowRight');
    await expect(page.locator('output[for="auto-hide-delay"]')).toHaveText('300 ms');
    await expect.poll(async () => (await saved())?.delayMs).toBe(300);
    await expect(page.locator('.status')).toHaveText('Saved');

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

  test('pinned apps can be moved and unpinned', async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await openSettings(page);
    const names = () => page.locator('.pin-name').allTextContents();
    const before = await names();
    expect(before).toHaveLength(5);
    expect(before[4]).toBe('Settings'); // the labelled entry shows its label

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
