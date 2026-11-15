import { describe, expect, it } from 'vitest';

import type { DockItemKind } from '$lib/ipc/generated/DockItemKind';
import type { MonitorInfo } from '$lib/ipc/generated/MonitorInfo';

import {
  awaitingMove,
  FIT_END_MARGIN,
  fitIconSize,
  fitRoom,
  fittedPanelLength,
  MIN_FITTED_ICON,
  slotCounts,
  workAreaCss,
  type FitInput,
  type RoomInput,
} from './fit';
import { maxGrowth } from './magnify';

/** Dock.svelte's constants: an 8 px slot gap and padding, a 1 px separator with 6 px margins. */
const LAYOUT = { slotGap: 8, panelPadding: 8, separatorLength: 13 };
const MAGNIFY = { maxScale: 1.6, rangeIcons: 3 };
const FLAT = { maxScale: 1, rangeIcons: 3 };
/** The hands-on case: every fixed item on, the demo's apps (12 icons, 3 separators). */
const EVERY_ITEM = { icons: 12, separators: 3 };

function input(patch: Partial<FitInput>): FitInput {
  return {
    iconSize: 40,
    counts: EVERY_ITEM,
    layout: LAYOUT,
    magnify: MAGNIFY,
    viewportLength: 600,
    ...patch,
  };
}

describe('slotCounts', () => {
  it('counts separators apart from every other kind', () => {
    const kinds: DockItemKind[] = [
      'launcher',
      'separator',
      'app',
      'app',
      'separator',
      'clock',
      'recycleBin',
    ];
    expect(slotCounts(kinds.map((kind) => ({ kind })))).toEqual({ icons: 5, separators: 2 });
    expect(slotCounts([])).toEqual({ icons: 0, separators: 0 });
  });
});

describe('fittedPanelLength', () => {
  it('is the padding, the slots and the separators at rest, plus the magnification bound', () => {
    // 16 + 12 × 48 + 3 × 13 = 631: the panel the hands-on check logged at 40.
    expect(fittedPanelLength(40, EVERY_ITEM, LAYOUT, FLAT)).toBe(631);
    expect(fittedPanelLength(40, EVERY_ITEM, LAYOUT, MAGNIFY)).toBeCloseTo(
      631 + maxGrowth(48, MAGNIFY),
    );
  });

  it('grows with the icon size', () => {
    let previous = fittedPanelLength(MIN_FITTED_ICON, EVERY_ITEM, LAYOUT, MAGNIFY);
    for (let size = MIN_FITTED_ICON + 1; size <= 128; size += 1) {
      const length = fittedPanelLength(size, EVERY_ITEM, LAYOUT, MAGNIFY);
      expect(length).toBeGreaterThan(previous);
      previous = length;
    }
  });
});

describe('fitIconSize', () => {
  it('keeps the setting when the dock fits', () => {
    expect(fitIconSize(input({ viewportLength: 1080 }))).toBe(40);
    expect(fitIconSize(input({ iconSize: 48, viewportLength: 2560 }))).toBe(48);
  });

  it('gives the largest whole size that fits with the end margins to spare', () => {
    const size = fitIconSize(input({}));
    expect(Number.isInteger(size)).toBe(true);
    expect(size).toBeLessThan(40);
    const room = 600 - 2 * FIT_END_MARGIN;
    expect(fittedPanelLength(size, EVERY_ITEM, LAYOUT, MAGNIFY)).toBeLessThanOrEqual(room);
    expect(fittedPanelLength(size + 1, EVERY_ITEM, LAYOUT, MAGNIFY)).toBeGreaterThan(room);
    // 55 + 14.1 × (s + 8) ≤ 584 → s ≤ 29.5.
    expect(size).toBe(29);
  });

  it('keeps a size that fits to the pixel', () => {
    // At 30 with magnification off the panel is 16 + 12 × 38 + 39 = 511 px.
    const exact = 511 + 2 * FIT_END_MARGIN;
    expect(fitIconSize(input({ iconSize: 30, magnify: FLAT, viewportLength: exact }))).toBe(30);
    expect(fitIconSize(input({ iconSize: 30, magnify: FLAT, viewportLength: exact - 1 }))).toBe(29);
  });

  it('counts the magnification growth, so turning magnification off gives more room', () => {
    const magnified = fitIconSize(input({}));
    const flat = fitIconSize(input({ magnify: FLAT }));
    expect(flat).toBeGreaterThan(magnified);
    // 55 + 12 × (s + 8) ≤ 584 → s ≤ 36.08.
    expect(flat).toBe(36);
    // Reduced motion is magnification off too (the page passes a scale of 1).
    expect(fitIconSize(input({ magnify: { maxScale: 1, rangeIcons: 6 } }))).toBe(36);
  });

  it('shrinks as items come and grows back as they go', () => {
    const sizes = [8, 12, 16, 24, 32].map((icons) =>
      fitIconSize(input({ iconSize: 64, counts: { icons, separators: 3 }, viewportLength: 1032 })),
    );
    for (let i = 1; i < sizes.length; i += 1) {
      expect(sizes[i]!).toBeLessThanOrEqual(sizes[i - 1]!);
    }
    expect(sizes[0]).toBe(64);
    expect(sizes.at(-1)!).toBeLessThan(64);
  });

  it('never goes below the minimum, even when the panel still overflows there', () => {
    const many = { icons: 60, separators: 3 };
    expect(fitIconSize(input({ counts: many }))).toBe(MIN_FITTED_ICON);
    expect(fittedPanelLength(MIN_FITTED_ICON, many, LAYOUT, MAGNIFY)).toBeGreaterThan(600);
    expect(fitIconSize(input({ counts: many, viewportLength: 40 }))).toBe(MIN_FITTED_ICON);
  });

  it('never gives more than the setting, and leaves a setting below the minimum alone', () => {
    expect(fitIconSize(input({ iconSize: 12, viewportLength: 4000 }))).toBe(12);
    expect(fitIconSize(input({ iconSize: 12, counts: { icons: 60, separators: 3 } }))).toBe(12);
    expect(fitIconSize(input({ iconSize: 17, counts: { icons: 60, separators: 3 } }))).toBe(16);
  });

  it('fits anything in a viewport of unknown length', () => {
    for (const viewportLength of [0, -1, Number.NaN, Number.POSITIVE_INFINITY]) {
      expect(fitIconSize(input({ viewportLength }))).toBe(40);
    }
  });

  it('an empty dock is its padding long and fits at its size', () => {
    const none = { icons: 0, separators: 0 };
    expect(fittedPanelLength(40, none, LAYOUT, FLAT)).toBe(16);
    expect(fitIconSize(input({ counts: none, magnify: FLAT, viewportLength: 100 }))).toBe(40);
  });

  it('depends on nothing but its inputs: the same inputs give the same size', () => {
    const a = input({ iconSize: 64, counts: { icons: 20, separators: 3 }, viewportLength: 1032 });
    expect(fitIconSize(a)).toBe(fitIconSize({ ...a }));
  });
});

describe('fitRoom: the length along the edge the window has, or is about to have', () => {
  /** A 1920×1080 monitor at 100 %, the stock taskbar at the bottom (CSS px). */
  const WORK_AREA = { width: 1920, height: 1032 };

  function room(patch: Partial<RoomInput>): RoomInput {
    return {
      axis: 'vertical',
      shapedAxis: 'vertical',
      viewport: { width: 341, height: 1032 },
      workArea: WORK_AREA,
      ...patch,
    };
  }

  it("is the viewport's length along the edge while the window is shaped for it", () => {
    expect(fitRoom(room({}))).toBe(1032);
    expect(
      fitRoom(
        room({
          axis: 'horizontal',
          shapedAxis: 'horizontal',
          viewport: { width: 1920, height: 150 },
        }),
      ),
    ).toBe(1920);
    // A side dock in a window wider than tall (a browser at 800×600, the
    // hands-on case): shaped for its axis all along, so its viewport.
    expect(fitRoom(room({ viewport: { width: 800, height: 600 } }))).toBe(600);
    // Before the first settings nothing has switched.
    expect(fitRoom(room({ shapedAxis: null, viewport: { width: 1920, height: 150 } }))).toBe(150);
  });

  it("bottom to left before the move: the work area's height, not the bottom window's thickness or width", () => {
    const r = room({ shapedAxis: 'horizontal', viewport: { width: 1920, height: 150 } });
    expect(awaitingMove(r)).toBe(true);
    expect(fitRoom(r)).toBe(1032);
  });

  it("left to bottom before the move: the work area's width, not the side window's height", () => {
    const r = room({ axis: 'horizontal', viewport: { width: 341, height: 1032 } });
    expect(awaitingMove(r)).toBe(true);
    expect(fitRoom(r)).toBe(1920);
  });

  it('after the move the viewport again, even before the page saw the resize', () => {
    const r = room({ shapedAxis: 'horizontal', viewport: { width: 150, height: 1032 } });
    expect(awaitingMove(r)).toBe(false);
    expect(fitRoom(r)).toBe(1032);
  });

  it('a change along the same axis (left to right) waits for nothing', () => {
    expect(awaitingMove(room({ shapedAxis: 'vertical' }))).toBe(false);
  });

  it('without a monitor list, or with an empty work area, nothing better than the viewport is known', () => {
    const r = room({
      shapedAxis: 'horizontal',
      viewport: { width: 1920, height: 150 },
      workArea: null,
    });
    expect(awaitingMove(r)).toBe(true);
    expect(fitRoom(r)).toBe(150);
    expect(fitRoom({ ...r, workArea: { width: 0, height: 0 } })).toBe(150);
  });

  it('a bottom-to-left switch on 1920×1080 fits the column the window will have', () => {
    const r = room({ shapedAxis: 'horizontal', viewport: { width: 1920, height: 150 } });
    const size = fitIconSize(
      input({ iconSize: 64, counts: { icons: 24, separators: 3 }, viewportLength: fitRoom(r) }),
    );
    expect(size).toBeLessThan(64);
    expect(
      fittedPanelLength(size, { icons: 24, separators: 3 }, LAYOUT, MAGNIFY),
    ).toBeLessThanOrEqual(1032 - 2 * FIT_END_MARGIN);
  });
});

describe('workAreaCss', () => {
  const monitor = (scaleFactor: number): MonitorInfo => ({
    id: 'm',
    name: 'm',
    bounds: { x: 0, y: 0, width: 2880, height: 1620 },
    workArea: { x: 0, y: 0, width: 2880, height: 1548 },
    scaleFactor,
    isPrimary: true,
  });

  it("turns the physical work area into CSS px at the monitor's scale", () => {
    expect(workAreaCss(monitor(1.5))).toEqual({ width: 1920, height: 1032 });
    expect(workAreaCss(monitor(1))).toEqual({ width: 2880, height: 1548 });
  });

  it('has nothing without a monitor, and takes a scale it cannot use as 1', () => {
    expect(workAreaCss(null)).toBeNull();
    expect(workAreaCss(undefined)).toBeNull();
    expect(workAreaCss(monitor(0))).toEqual({ width: 2880, height: 1548 });
  });
});
