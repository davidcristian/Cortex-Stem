import { mkdirSync } from 'node:fs';

import { expect, test, type Page } from '@playwright/test';

const OUT = 'e2e/output';
const APP_SLOT = '.slot[data-kind="app"]';

/** What the pages expose in mock mode (`src/lib/ipc/demo.ts`; structural, for `page.evaluate`). */
interface DemoWindow {
  __dockMock?: {
    calls: Array<{ type: string }>;
    failNextCommand: (message: string) => void;
    getSettings: () => Promise<{ iconSize: number; theme: string; pinned: Array<{ id: string }> }>;
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

test.describe('dock at 1× device pixel ratio', () => {
  test.use({ deviceScaleFactor: 1 });

  test('dark theme at rest requests icons at the base size', async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await openDock(page);
    // Icons are requested at iconSize × devicePixelRatio = 48 × 1.
    const widths = await page.$$eval('.icon img', (imgs) =>
      imgs.map((img) => (img as HTMLImageElement).naturalWidth),
    );
    expect(widths.length).toBeGreaterThan(0);
    expect(new Set(widths)).toEqual(new Set([48]));
    await page.screenshot({ path: `${OUT}/dock-dark-1x.png` });
  });
});

test.describe('dock in mock mode', () => {
  test('dark theme at rest', async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await openDock(page);
    await expect(page.locator('html')).toHaveAttribute('data-theme', 'dark');
    await page.screenshot({ path: `${OUT}/dock-dark.png` });
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
    await page.screenshot({ path: `${OUT}/dock-hover.png` });

    // Leaving resets everything.
    await page.mouse.move(10, 10);
    await expect.poll(() => scaleOf(page, 3)).toBe(1);
    await expect(page.locator('.tooltip')).toHaveCount(0);
  });

  test('twenty apps', async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'dark' });
    await openDock(page, 'mock=many');
    await expect(page.locator(APP_SLOT)).toHaveCount(20);
    await page.screenshot({ path: `${OUT}/dock-many.png` });
  });

  test('reduced motion disables magnification', async ({ page }) => {
    await page.emulateMedia({ colorScheme: 'dark', reducedMotion: 'reduce' });
    await openDock(page);
    await page.locator(APP_SLOT).nth(3).hover();
    await expect(page.locator('.tooltip')).toHaveText('notepad');
    expect(await scaleOf(page, 3)).toBe(1);
    await page.screenshot({ path: `${OUT}/dock-reduced-motion.png` });
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

    /** Press on pinned slot `from` and drag to just past the centre of pinned slot `over`. */
    async function dragPinned(page: Page, from: number, over: number): Promise<void> {
      const a = (await page.locator(PINNED).nth(from).boundingBox())!;
      const b = (await page.locator(PINNED).nth(over).boundingBox())!;
      const past = over > from ? 6 : -6;
      await page.mouse.move(a.x + a.width / 2, a.y + a.height / 2);
      await page.mouse.down();
      await page.mouse.move(b.x + b.width / 2 + past, a.y + a.height / 2 - 12, { steps: 12 });
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
    });

    test('Escape puts everything back', async ({ page }) => {
      await openDock(page);
      const before = await pinnedKeys(page);
      await dragPinned(page, 3, 1);
      await expect.poll(() => transformOf(page, 1)).toBe('translateX(56px)');
      await page.keyboard.press('Escape');
      await expect(page.locator('[data-drag]')).toHaveCount(0);
      await expect.poll(() => transformOf(page, 1)).toBe('');
      await page.mouse.up();
      expect(await pinnedKeys(page)).toEqual(before);
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
