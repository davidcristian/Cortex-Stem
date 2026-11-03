import { mkdirSync } from 'node:fs';

import { expect, test, type Page } from '@playwright/test';

import type { RawWindow } from '../src/lib/ipc/generated/RawWindow';
import type { Rect } from '../src/lib/ipc/generated/Rect';

const OUT = 'e2e/output';
const APP_SLOT = '.slot[data-kind="app"]';

// The dock's layout constants (src/dock/Dock.svelte) the assertions below rely on.
const ICON = 48;
const SLOT_GAP = 8;
const PANEL_PADDING = 8;

/** What the pages expose in mock mode (`src/lib/ipc/demo.ts`; structural, for `page.evaluate`). */
interface DemoWindow {
  __dockMock?: {
    calls: Array<{ type: string } & Record<string, unknown>>;
    failNextCommand: (message: string) => void;
    getSettings: () => Promise<{ iconSize: number; theme: string; pinned: Array<{ id: string }> }>;
    pushWindow: (raw: RawWindow) => void;
    pushWindowRemoved: (id: number) => void;
  };
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
    expect((await iconPairs(page))[3]).toMatchObject({ shown: [192], magnifiedAttr: true });
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

    // Disabled, not-yet-available options stay disabled.
    await expect(page.locator('#current-monitor')).toBeDisabled();
    await expect(page.locator('#autostart')).toBeDisabled();
    await expect(page.locator('#position option[value="left"]')).toBeDisabled();
    await expect(page.locator('#auto-hide option[value="always"]')).toBeDisabled();
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
