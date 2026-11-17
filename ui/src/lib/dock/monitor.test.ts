import { describe, expect, it } from 'vitest';

import type { DockFacts } from '$lib/ipc/generated/DockFacts';
import type { DockItem } from '$lib/ipc/generated/DockItem';
import type { DockItemKind } from '$lib/ipc/generated/DockItemKind';
import type { MonitorInfo } from '$lib/ipc/generated/MonitorInfo';
import type { WindowInfo } from '$lib/ipc/generated/WindowInfo';

import {
  dockFactsFor,
  dockMonitor,
  itemsForMonitor,
  monitorFromSearch,
  NO_FACTS,
  primaryMonitorId,
} from './monitor';

const PRIMARY = 'mock-primary';
const SECOND = '\\\\.\\DISPLAY3';

function monitor(id: string, isPrimary: boolean): MonitorInfo {
  return {
    id,
    name: id,
    bounds: { x: 0, y: 0, width: 2560, height: 1440 },
    workArea: { x: 0, y: 0, width: 2560, height: 1392 },
    scaleFactor: 1,
    isPrimary,
  };
}

function win(
  id: number,
  monitor: string | null,
  isActive = false,
  needsAttention = false,
): WindowInfo {
  return {
    id,
    title: `W${id}`,
    isMinimized: false,
    isActive,
    monitor,
    iconKey: null,
    needsAttention,
  };
}

/** An app item whose flags follow its windows, as the engine builds it. */
function app(key: string, pinned: boolean, windows: WindowInfo[]): DockItem {
  return {
    key: `app:${key}`,
    kind: 'app',
    appId: key,
    title: key,
    iconKey: null,
    pinned,
    windows,
    isActive: windows.some((w) => w.isActive),
    needsAttention: windows.some((w) => w.needsAttention),
    badge: null,
  };
}

function fixed(kind: Exclude<DockItemKind, 'app'>, key: string): DockItem {
  return {
    key,
    kind,
    appId: null,
    title: key,
    iconKey: null,
    pinned: false,
    windows: [],
    isActive: false,
    needsAttention: false,
    badge: null,
  };
}

const sep = (n: number): DockItem => fixed('separator', `sep:${n}`);
const keys = (items: DockItem[]): string[] => items.map((i) => i.key);

describe('the dock page’s monitor', () => {
  it('reads and decodes ?monitor= from the URL', () => {
    expect(monitorFromSearch('?mock=1&monitor=%5C%5C.%5CDISPLAY3')).toBe(SECOND);
    expect(monitorFromSearch('?monitor=mock-primary')).toBe(PRIMARY);
    expect(monitorFromSearch('')).toBeNull();
    expect(monitorFromSearch('?mock=1')).toBeNull();
    expect(monitorFromSearch('?monitor=')).toBeNull();
  });

  it('picks the primary monitor like the app layer: the flagged one, else the first', () => {
    expect(primaryMonitorId([monitor(SECOND, false), monitor(PRIMARY, true)])).toBe(PRIMARY);
    expect(primaryMonitorId([monitor(SECOND, false), monitor(PRIMARY, false)])).toBe(SECOND);
    expect(primaryMonitorId([])).toBeNull();
  });

  it('is the URL’s monitor, else the primary, else unknown', () => {
    const monitors = [monitor(PRIMARY, true), monitor(SECOND, false)];
    expect(dockMonitor(SECOND, monitors)).toBe(SECOND);
    expect(dockMonitor(null, monitors)).toBe(PRIMARY);
    expect(dockMonitor(SECOND, [])).toBe(SECOND);
    expect(dockMonitor(null, [])).toBeNull();
  });
});

describe('the dock’s own facts', () => {
  const docks: DockFacts[] = [
    { monitor: SECOND, fullscreen: true, overlapped: false },
    { monitor: PRIMARY, fullscreen: false, overlapped: true },
  ];

  it('finds the entry of the dock’s monitor', () => {
    expect(dockFactsFor(docks, PRIMARY)).toEqual({ fullscreen: false, overlapped: true });
    expect(dockFactsFor(docks, SECOND)).toEqual({ fullscreen: true, overlapped: false });
  });

  it('is all false without an entry', () => {
    expect(dockFactsFor(docks, '\\\\.\\DISPLAY9')).toEqual(NO_FACTS);
    expect(dockFactsFor([], PRIMARY)).toEqual(NO_FACTS);
    expect(dockFactsFor([], null)).toEqual(NO_FACTS);
  });

  it('takes the only entry when the page cannot tell its monitor yet', () => {
    expect(dockFactsFor([docks[0]!], null)).toEqual({ fullscreen: true, overlapped: false });
    // Two entries and no monitor: nothing to choose from.
    expect(dockFactsFor(docks, null)).toEqual(NO_FACTS);
  });
});

describe('the per-monitor item filter', () => {
  // The default demo (mirror of the Rust engine's order): fixed, pinned,
  // running, Recycle Bin, with separators between the non-empty groups.
  const explorer = app('explorer', true, [win(1, PRIMARY)]);
  const firefox = app('firefox', true, [win(2, PRIMARY, true), win(3, SECOND)]);
  const notepad = app('notepad', true, [win(7, SECOND)]);
  const settings = app('settings', true, []);
  const code = app('code', false, [win(4, PRIMARY, false, true)]);
  const paint = app('paint', false, [win(8, SECOND, false, true)]);
  const items: DockItem[] = [
    fixed('launcher', 'fixed:launcher'),
    sep(1),
    explorer,
    firefox,
    notepad,
    settings,
    sep(2),
    code,
    paint,
    sep(3),
    fixed('recycleBin', 'fixed:recycle-bin'),
  ];

  it('returns the list as is while off, or while the monitor is unknown', () => {
    expect(itemsForMonitor(items, PRIMARY, false)).toEqual(items);
    expect(itemsForMonitor(items, null, true)).toEqual(items);
    // A copy, never the same array (the caller may freeze it).
    expect(itemsForMonitor(items, PRIMARY, false)).not.toBe(items);
  });

  it('keeps the windows on the dock’s monitor, pinned and fixed items always', () => {
    const primary = itemsForMonitor(items, PRIMARY, true);
    expect(keys(primary)).toEqual([
      'fixed:launcher',
      'sep:1',
      'app:explorer',
      'app:firefox',
      'app:notepad',
      'app:settings',
      'sep:2',
      'app:code',
      'sep:3',
      'fixed:recycle-bin',
    ]);
    // Firefox keeps only its primary window, still active; Notepad is a
    // pinned app with no window here; Paint (unpinned, elsewhere) is gone.
    expect(primary.find((i) => i.key === 'app:firefox')?.windows.map((w) => w.id)).toEqual([2]);
    expect(primary.find((i) => i.key === 'app:firefox')?.isActive).toBe(true);
    expect(primary.find((i) => i.key === 'app:notepad')?.windows).toEqual([]);
    expect(primary.find((i) => i.key === 'app:notepad')?.pinned).toBe(true);
    // Items that lost nothing are the same objects.
    expect(primary.find((i) => i.key === 'app:explorer')).toBe(explorer);
    expect(primary.find((i) => i.key === 'app:code')).toBe(code);

    const second = itemsForMonitor(items, SECOND, true);
    expect(keys(second)).toEqual([
      'fixed:launcher',
      'sep:1',
      'app:explorer',
      'app:firefox',
      'app:notepad',
      'app:settings',
      'sep:2',
      'app:paint',
      'sep:3',
      'fixed:recycle-bin',
    ]);
    // Firefox's remaining window is not the active one.
    const ff = second.find((i) => i.key === 'app:firefox')!;
    expect(ff.windows.map((w) => w.id)).toEqual([3]);
    expect(ff.isActive).toBe(false);
    expect(second.find((i) => i.key === 'app:notepad')?.windows.map((w) => w.id)).toEqual([7]);
    expect(second.find((i) => i.key === 'app:paint')).toBe(paint);
  });

  it('drops the separators that no longer separate two groups', () => {
    // No running app left on a third monitor: the running group goes and
    // one of the two separators around it with it, leaving fixed | pinned |
    // bin (the first of the pair stays; keys are not renumbered).
    const third = itemsForMonitor(items, '\\\\.\\DISPLAY9', true);
    expect(keys(third)).toEqual([
      'fixed:launcher',
      'sep:1',
      'app:explorer',
      'app:firefox',
      'app:notepad',
      'app:settings',
      'sep:2',
      'fixed:recycle-bin',
    ]);
    // Nothing but running apps and separators around them: no separator
    // leads or trails.
    const bare: DockItem[] = [sep(1), code, sep(2), paint, sep(3)];
    expect(keys(itemsForMonitor(bare, PRIMARY, true))).toEqual(['app:code']);
    expect(keys(itemsForMonitor(bare, '\\\\.\\DISPLAY9', true))).toEqual([]);
  });

  it('keeps the attention flag only while a window remains', () => {
    const second = itemsForMonitor(items, SECOND, true);
    expect(second.find((i) => i.key === 'app:paint')?.needsAttention).toBe(true);
    // Code (flashing) has no window on the second monitor and is not pinned: gone.
    expect(second.find((i) => i.key === 'app:code')).toBeUndefined();
    // A pinned, flashing app whose windows are all elsewhere keeps its slot, not its flag.
    const pinnedFlashing = app('mail', true, [win(9, PRIMARY, false, true)]);
    const [kept] = itemsForMonitor([pinnedFlashing], SECOND, true);
    expect(kept?.needsAttention).toBe(false);
    expect(kept?.windows).toEqual([]);
    // Windows without a monitor belong to no dock while the filter is on.
    const unknown = app('x', false, [win(10, null)]);
    expect(itemsForMonitor([unknown], PRIMARY, true)).toEqual([]);
  });

  it('needs attention only on the monitor of the window that flashes', () => {
    // Teams has a window on each monitor; only the one on the second flashes.
    const teams = app('teams', false, [win(11, PRIMARY), win(12, SECOND, false, true)]);
    expect(teams.needsAttention).toBe(true);
    const [onPrimary] = itemsForMonitor([teams], PRIMARY, true);
    expect(onPrimary?.windows.map((w) => w.id)).toEqual([11]);
    expect(onPrimary?.needsAttention).toBe(false);
    const [onSecond] = itemsForMonitor([teams], SECOND, true);
    expect(onSecond?.windows.map((w) => w.id)).toEqual([12]);
    expect(onSecond?.needsAttention).toBe(true);

    // The other way round, and with both flashing: each dock follows its own window.
    const back = app('teams', false, [win(11, PRIMARY, false, true), win(12, SECOND)]);
    expect(itemsForMonitor([back], PRIMARY, true)[0]?.needsAttention).toBe(true);
    expect(itemsForMonitor([back], SECOND, true)[0]?.needsAttention).toBe(false);
    const both = app('teams', false, [win(11, PRIMARY, false, true), win(12, SECOND, false, true)]);
    expect(itemsForMonitor([both], PRIMARY, true)[0]?.needsAttention).toBe(true);
    expect(itemsForMonitor([both], SECOND, true)[0]?.needsAttention).toBe(true);

    // With the filter off every dock shows the engine's flag: any window.
    expect(itemsForMonitor([teams], PRIMARY, false)[0]?.needsAttention).toBe(true);
  });
});
