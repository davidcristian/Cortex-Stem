import { afterEach, describe, expect, it, vi } from 'vitest';

import type { DockState } from '$lib/ipc/generated/DockState';
import type { Settings } from '$lib/ipc/generated/Settings';

import type { BouncePayload, FileDragEvent } from './backend';
import type { RawWindow } from '$lib/ipc/generated/RawWindow';
import type { Rect } from '$lib/ipc/generated/Rect';

import {
  appIdForExe,
  colorFor,
  defaultSettings,
  fileStem,
  MockBackend,
  nothingToRecycle,
  overlapped,
  panelOnScreen,
  rectsIntersect,
  recyclePlan,
  validateSettings,
  type MockOptions,
} from './mock';

const EXPLORER = appIdForExe('C:\\Windows\\explorer.exe');
const FIREFOX = appIdForExe('C:\\Program Files\\Mozilla Firefox\\firefox.exe');
const TERMINAL = appIdForExe('C:\\Program Files\\WindowsApps\\Terminal\\WindowsTerminal.exe');
const NOTEPAD = appIdForExe('C:\\Windows\\notepad.exe');
const SETTINGS = 'uri:ms-settings:';
const CODE = appIdForExe('C:\\Users\\me\\AppData\\Local\\Programs\\Microsoft VS Code\\Code.exe');
const SPOTIFY = appIdForExe('C:\\Program Files\\Spotify\\Spotify.exe');

const app = (id: string): string => `app:${id}`;
const keys = (state: DockState): string[] => state.items.map((i) => i.key);
const item = (state: DockState, key: string) => {
  const found = state.items.find((i) => i.key === key);
  if (!found) {
    throw new Error(`no item ${key} in ${keys(state).join(', ')}`);
  }
  return found;
};

/** A backend with no latency and identities resolved up front. */
function fast(options: MockOptions = {}): MockBackend {
  return new MockBackend({ latencyMs: [0, 0], identityDelayMs: 0, log: false, ...options });
}

/** Collects `dock://state` events. */
function recordStates(backend: MockBackend): DockState[] {
  const states: DockState[] = [];
  backend.on('dock://state', (s) => {
    states.push(s);
  });
  return states;
}

const backends: MockBackend[] = [];
afterEach(() => {
  for (const b of backends.splice(0)) {
    b.dispose();
  }
  vi.useRealTimers();
});

describe('demo scenario (mirror of crates/stem-platform/src/mock.rs)', () => {
  it('orders items like the Rust engine: fixed, pinned, running, recycle bin', async () => {
    const b = fast();
    const state = await b.getDockState();
    expect(keys(state)).toEqual([
      'fixed:launcher',
      'sep:1',
      app(EXPLORER),
      app(FIREFOX),
      app(TERMINAL),
      app(NOTEPAD),
      app(SETTINGS),
      'sep:2',
      app(CODE),
      app(SPOTIFY),
      'sep:3',
      'fixed:recycle-bin',
    ]);
    const firefox = item(state, app(FIREFOX));
    expect(firefox.windows.map((w) => w.id)).toEqual([2, 3]);
    expect(firefox.isActive).toBe(true);
    expect(firefox.windows[1]?.isMinimized).toBe(true);
    expect(firefox.pinned).toBe(true);
    expect(item(state, app(CODE)).pinned).toBe(false);
    expect(item(state, app(NOTEPAD)).windows).toEqual([]);
    expect(item(state, app(SPOTIFY)).windows[0]?.isMinimized).toBe(true);
    expect(item(state, 'fixed:recycle-bin').iconKey).toBe('shell:recycle-bin-full');
    expect(state.foreground).toBe(2);
  });

  it('shows window titles until identities resolve 300 ms later, then names and icons', async () => {
    vi.useFakeTimers();
    const b = new MockBackend({ latencyMs: [0, 0], identityDelayMs: 300, log: false });
    backends.push(b);
    const states = recordStates(b);

    const before = await b.getDockState();
    expect(item(before, app(FIREFOX)).title).toBe('Stem — Mozilla Firefox');
    expect(item(before, app(FIREFOX)).iconKey).toBeNull();
    expect(item(before, app(NOTEPAD)).title).toBe(NOTEPAD);

    await vi.advanceTimersByTimeAsync(299);
    expect(states).toHaveLength(0);
    await vi.advanceTimersByTimeAsync(1);
    expect(states).toHaveLength(1);

    const after = states[0]!;
    expect(after.revision).toBeGreaterThan(before.revision);
    expect(item(after, app(FIREFOX)).title).toBe('firefox');
    expect(item(after, app(FIREFOX)).iconKey).toBe(`mock:${FIREFOX}`);
    expect(item(after, app(CODE)).title).toBe('Code');
    // Pinned-only apps are named from their launch target, or from the label.
    expect(item(after, app(NOTEPAD)).title).toBe('notepad');
    expect(item(after, app(NOTEPAD)).iconKey).toBe(`mock:${NOTEPAD}`);
    expect(item(after, app(SETTINGS)).title).toBe('Settings');
  });

  it('simulates latency within the configured range', async () => {
    vi.useFakeTimers();
    const b = new MockBackend({ latencyMs: [20, 80], identityDelayMs: 0, log: false });
    backends.push(b);
    let settled = false;
    void b.getDockState().then(() => {
      settled = true;
    });
    await vi.advanceTimersByTimeAsync(19);
    expect(settled).toBe(false);
    await vi.advanceTimersByTimeAsync(61);
    expect(settled).toBe(true);
  });

  it('runs commands in the order they were issued, whatever latency each one drew', async () => {
    vi.useFakeTimers();
    // A wide range: with twenty draws some later command is bound to draw
    // a shorter latency than an earlier one.
    const b = new MockBackend({ latencyMs: [1, 200], identityDelayMs: 0, log: false });
    backends.push(b);
    const issued = Array.from({ length: 20 }, (_, i) => i % 2 === 0);
    const settled: boolean[] = [];
    const all = Promise.all(
      issued.map((hidden) =>
        b.setDockAutoHidden(hidden).then(() => {
          settled.push(hidden);
        }),
      ),
    );
    await vi.advanceTimersByTimeAsync(200);
    await all;
    expect(b.calls.map((c) => (c.type === 'setDockAutoHidden' ? c.hidden : null))).toEqual(issued);
    expect(settled).toEqual(issued);

    // A failure keeps its place and stops nothing after it.
    b.failNextCommand('refused');
    const first = b.openSettings();
    const second = b.quit();
    await vi.advanceTimersByTimeAsync(200);
    await expect(first).rejects.toBe('refused');
    await second;
    expect(b.calls.slice(-1)).toEqual([{ type: 'quit' }]);
  });
});

describe('click semantics (mirror of DockEngine::primary_action)', () => {
  it('cycles to the next window of the active multi-window app', async () => {
    const b = fast();
    await b.primaryClick(app(FIREFOX));
    expect(b.calls).toEqual([{ type: 'windowAction', windowId: 3, action: 'activate' }]);
    const state = await b.getDockState();
    expect(state.foreground).toBe(3);
    expect(item(state, app(FIREFOX)).windows[1]?.isMinimized).toBe(false);
  });

  it('activates an inactive window, then minimises the active single window', async () => {
    const b = fast();
    await b.primaryClick(app(CODE));
    expect((await b.getDockState()).foreground).toBe(4);
    expect(item(await b.getDockState(), app(CODE)).isActive).toBe(true);

    await b.primaryClick(app(CODE));
    expect(b.calls).toEqual([
      { type: 'windowAction', windowId: 4, action: 'activate' },
      { type: 'windowAction', windowId: 4, action: 'minimize' },
    ]);
    const state = await b.getDockState();
    expect(state.foreground).toBeNull();
    expect(item(state, app(CODE)).windows[0]?.isMinimized).toBe(true);
    expect(item(state, app(CODE)).isActive).toBe(false);
  });

  it('activates instead of minimising when clickActiveMinimizes is off', async () => {
    const b = fast();
    const settings = await b.getSettings();
    settings.behavior.clickActiveMinimizes = false;
    await b.updateSettings(settings);
    await b.primaryClick(app(CODE));
    await b.primaryClick(app(CODE));
    expect(b.calls.map((c) => c.type === 'windowAction' && c.action)).toEqual([
      'activate',
      'activate',
    ]);
  });

  it('launches a pinned app without windows and bounces it', async () => {
    const b = fast();
    const bounces: BouncePayload[] = [];
    b.on('dock://item-bounce', (p) => {
      bounces.push(p);
    });
    await b.primaryClick(app(NOTEPAD));
    expect(b.calls).toEqual([
      {
        type: 'launch',
        target: { kind: 'exe', path: 'C:\\Windows\\notepad.exe', args: [], workingDir: null },
      },
    ]);
    expect(bounces).toEqual([{ itemKey: app(NOTEPAD) }]);

    await b.primaryClick(app(SETTINGS));
    expect(b.calls[1]).toEqual({ type: 'launch', target: { kind: 'uri', uri: 'ms-settings:' } });
  });

  it('maps fixed items to shell actions and rejects unknown keys', async () => {
    const b = fast();
    await b.primaryClick('fixed:launcher');
    await b.primaryClick('fixed:recycle-bin');
    expect(b.calls).toEqual([
      { type: 'shell', action: 'openStartMenu' },
      { type: 'shell', action: 'openRecycleBin' },
    ]);
    await expect(b.primaryClick('bogus')).rejects.toBe('unknown dock item: bogus');
    await expect(b.primaryClick('app:exe:c:/nope.exe')).rejects.toMatch(/unknown dock item/);
  });

  it('activates the first window when none of several is active', async () => {
    const b = fast();
    await b.primaryClick(app(CODE)); // foreground → 4
    await b.primaryClick(app(FIREFOX)); // firefox has 2 + 3, neither active
    expect(b.calls.at(-1)).toEqual({ type: 'windowAction', windowId: 2, action: 'activate' });
  });
});

describe('pins (mirror of stem-service pin/unpin/reorder)', () => {
  it('pin moves a running app into the pinned group, reorder and unpin move it around', async () => {
    const b = fast();
    const states = recordStates(b);

    await b.pinApp(SPOTIFY);
    let state = states.at(-1)!;
    expect(keys(state).slice(2, 9)).toEqual([
      app(EXPLORER),
      app(FIREFOX),
      app(TERMINAL),
      app(NOTEPAD),
      app(SETTINGS),
      app(SPOTIFY),
      'sep:2',
    ]);
    expect(item(state, app(SPOTIFY)).pinned).toBe(true);
    expect(item(state, app(SPOTIFY)).windows).toHaveLength(1);
    expect((await b.getSettings()).pinned.map((p) => p.id)).toContain(SPOTIFY);

    await b.reorderPinned([SPOTIFY, EXPLORER]);
    state = states.at(-1)!;
    expect(keys(state).slice(2, 8)).toEqual([
      app(SPOTIFY),
      app(EXPLORER),
      app(FIREFOX),
      app(TERMINAL),
      app(NOTEPAD),
      app(SETTINGS),
    ]);

    await b.unpinApp(SPOTIFY);
    state = states.at(-1)!;
    // Back in the running group, after Code (first-seen order).
    expect(keys(state).slice(-4)).toEqual([app(CODE), app(SPOTIFY), 'sep:3', 'fixed:recycle-bin']);
    await expect(b.unpinApp(SPOTIFY)).rejects.toMatch(/not pinned/);
  });

  it('pinning twice is a no-op and pinning an unknown app fails', async () => {
    const b = fast();
    const states = recordStates(b);
    await b.pinApp(EXPLORER);
    expect(states).toHaveLength(0);
    await expect(b.pinApp('exe:c:/nope.exe')).rejects.toMatch(/no launch target/);
  });

  it('pinTarget pins under the canonical id with its identity already known', async () => {
    const b = fast();
    const id = await b.pinTarget({
      kind: 'exe',
      path: 'C:\\Tools\\Foo.EXE',
      args: [],
      workingDir: null,
    });
    expect(id).toBe('exe:c:/tools/foo.exe');
    const state = await b.getDockState();
    const foo = item(state, app(id));
    expect(foo.title).toBe('Foo');
    expect(foo.iconKey).toBe(`mock:${id}`);
    expect(foo.pinned).toBe(true);
    // A second pin of the same target changes nothing.
    expect(await b.pinTarget({ kind: 'shortcut', path: 'c:/tools/foo.exe' })).toBe(id);
    expect((await b.getSettings()).pinned.filter((p) => p.id === id)).toHaveLength(1);
  });

  it('records each pin request and each new order, the way the UI asked for them', async () => {
    const b = fast();
    const target = { kind: 'shortcut' as const, path: 'C:\\Links\\Foo.lnk' };
    const id = await b.pinTarget(target);
    await b.reorderPinned([id, EXPLORER]);
    expect(b.calls).toEqual([
      { type: 'pinTarget', target },
      { type: 'reorderPinned', order: [id, EXPLORER] },
    ]);
    expect((await b.getSettings()).pinned.map((p) => p.id).slice(0, 2)).toEqual([id, EXPLORER]);
    // A command that failed did nothing and is not recorded.
    b.failNextCommand('boom');
    await expect(b.pinTarget(target)).rejects.toBe('boom');
    expect(b.calls).toHaveLength(2);
  });

  it('a pinned app survives its last window closing; an unpinned one disappears', async () => {
    const b = fast();
    await b.windowAction(1, 'close'); // Explorer is pinned
    await b.windowAction(4, 'close'); // Code is not
    const state = await b.getDockState();
    expect(item(state, app(EXPLORER)).windows).toEqual([]);
    expect(keys(state)).not.toContain(app(CODE));
    await expect(b.windowAction(4, 'close')).rejects.toMatch(/window not found/);
  });
});

describe('file drags (docs/ipc/file-drags.md; simulated: a browser gets no OLE drags)', () => {
  it('delivers each simulated event, as a copy, to every subscriber until it unsubscribes', () => {
    const b = fast();
    const seen: FileDragEvent[] = [];
    const other: string[] = [];
    const off = b.onFileDrag((e) => seen.push(e));
    b.onFileDrag((e) => other.push(e.type));
    const enter: FileDragEvent = {
      type: 'enter',
      paths: ['C:\\Windows\\notepad.exe'],
      position: { x: 10, y: 20 },
    };
    b.simulateFileDrag(enter);
    b.simulateFileDrag({ type: 'over', position: { x: 12, y: 20 } });
    b.simulateFileDrag({ type: 'leave' });
    expect(seen).toEqual([enter, { type: 'over', position: { x: 12, y: 20 } }, { type: 'leave' }]);
    expect(seen[0]).not.toBe(enter);
    off();
    b.simulateFileDrag({ type: 'leave' });
    expect(seen).toHaveLength(3);
    expect(other).toEqual(['enter', 'over', 'leave', 'leave']);
    // A drag by itself pins nothing: that is the UI's call.
    expect(b.calls).toEqual([]);
  });
});

describe('the Recycle Bin drop (mirror of rules::recycle and the service)', () => {
  it('recyclePlan keeps plain absolute paths in the shell form, like plan_recycle', () => {
    expect(
      recyclePlan([
        'C:\\Users\\me\\Desktop\\notes.txt',
        'C:/Users/me/Desktop/Folder/',
        'd:\\Data\\\\archive.zip',
        '//nas/share/report.pdf',
      ]),
    ).toEqual({
      recycle: [
        'C:\\Users\\me\\Desktop\\notes.txt',
        'C:\\Users\\me\\Desktop\\Folder',
        'd:\\Data\\archive.zip',
        '\\\\nas\\share\\report.pdf',
      ],
      skipped: [],
    });
  });

  it('recyclePlan leaves out odd paths, roots, missing ones, repeats and folder contents', () => {
    const plan = recyclePlan(
      [
        'notes.txt',
        'C:\\',
        '\\\\nas\\share',
        '\\\\?\\C:\\x',
        'C:\\a\\..\\b',
        'C:\\Work\\report.docx',
        'C:\\Work\\gone.txt',
        'c:/work/REPORT.DOCX',
        'C:\\Work\\Drafts\\one.txt',
        'C:\\Work\\Drafts',
        'C:\\Work\\Drafts2\\two.txt',
      ],
      (path) => !path.endsWith('gone.txt'),
    );
    expect(plan.recycle).toEqual([
      'C:\\Work\\report.docx',
      'C:\\Work\\Drafts',
      'C:\\Work\\Drafts2\\two.txt',
    ]);
    expect(plan.skipped.map((s) => [s.path, s.reason])).toEqual([
      ['notes.txt', 'notAFilePath'],
      ['C:\\', 'root'],
      ['\\\\nas\\share', 'root'],
      ['\\\\?\\C:\\x', 'notAFilePath'],
      ['C:\\a\\..\\b', 'notAFilePath'],
      ['C:\\Work\\gone.txt', 'missing'],
      ['c:/work/REPORT.DOCX', 'duplicate'],
      ['C:\\Work\\Drafts\\one.txt', 'insideDropped'],
    ]);
  });

  it('nothingToRecycle says why, in the words of nothing_to_recycle', () => {
    expect(nothingToRecycle({ recycle: [], skipped: [] })).toBe(
      'no paths to move to the Recycle Bin',
    );
    expect(nothingToRecycle(recyclePlan(['C:\\Users\\me\\notes.txt'], () => false))).toBe(
      'C:\\Users\\me\\notes.txt does not exist',
    );
    expect(nothingToRecycle(recyclePlan(['C:\\a.txt', 'C:\\b.txt'], () => false))).toBe(
      'none of the 2 paths exists',
    );
    expect(nothingToRecycle(recyclePlan(['C:\\', 'notes.txt']))).toBe(
      'none of the 2 paths can be moved to the Recycle Bin',
    );
  });

  it('deleteToRecycleBin records the paths and the bin turns full at once', async () => {
    const b = fast();
    await b.shellAction('emptyRecycleBin');
    const states = recordStates(b);
    const paths = ['C:\\Users\\me\\Desktop\\notes.txt', 'C:\\Users\\me\\Desktop\\Old photos'];

    await b.deleteToRecycleBin(paths);
    expect(b.calls.at(-1)).toEqual({ type: 'deleteToRecycleBin', paths });
    expect(states).toHaveLength(1);
    expect(item(states[0]!, 'fixed:recycle-bin').iconKey).toBe('shell:recycle-bin-full');
    // Nothing else happened: no pin, no launch.
    expect(b.calls.map((c) => c.type)).toEqual(['shell', 'deleteToRecycleBin']);
  });

  it('an empty drop asks nothing; nothing usable rejects like Rust', async () => {
    const b = fast();
    await b.shellAction('emptyRecycleBin');
    await b.deleteToRecycleBin([]);
    expect(b.calls).toEqual([{ type: 'shell', action: 'emptyRecycleBin' }]);
    await expect(b.deleteToRecycleBin(['C:\\'])).rejects.toBe('C:\\ is a drive or share root');
    expect(b.calls.at(-1)).toEqual({ type: 'deleteToRecycleBin', paths: ['C:\\'] });
    expect(item(await b.getDockState(), 'fixed:recycle-bin').iconKey).toBe(
      'shell:recycle-bin-empty',
    );
  });
});

describe('revisions and events', () => {
  it('bumps the revision on every change and emits one state event per mutation', async () => {
    const b = fast();
    const states = recordStates(b);
    const r0 = (await b.getDockState()).revision;

    // Activating window 4 (not minimised) only changes the foreground: +1.
    await b.primaryClick(app(CODE));
    // Minimising window 5 changes the window, not the foreground: +1.
    await b.windowAction(5, 'minimize');
    // New settings: +1.
    await b.pinApp(SPOTIFY);
    expect(states.map((s) => s.revision)).toEqual([r0 + 1, r0 + 2, r0 + 3]);

    // Settings that change nothing visible: settings event, no state event.
    await b.updateSettings(await b.getSettings());
    expect(states).toHaveLength(3);
  });

  it('updateSettings validates, emits dock://settings and re-renders when items change', async () => {
    const b = fast();
    const states = recordStates(b);
    const settingsEvents: number[] = [];
    b.on('dock://settings', (s) => {
      settingsEvents.push(s.iconSize);
    });
    const settings = await b.getSettings();
    settings.iconSize = 999;
    settings.items.launcher = false;
    const saved = await b.updateSettings(settings);
    expect(saved.iconSize).toBe(128);
    expect(settingsEvents).toEqual([128]);
    expect(keys(states.at(-1)!)[0]).toBe(app(EXPLORER));
  });

  it('unsubscribing stops delivery', async () => {
    const b = fast();
    const seen: number[] = [];
    const off = b.on('dock://state', (s) => {
      seen.push(s.revision);
    });
    await b.windowAction(5, 'minimize');
    off();
    await b.windowAction(5, 'restore');
    expect(seen).toHaveLength(1);
  });

  it('emptying the recycle bin via shellAction switches its icon', async () => {
    const b = fast();
    await b.shellAction('emptyRecycleBin');
    expect(item(await b.getDockState(), 'fixed:recycle-bin').iconKey).toBe(
      'shell:recycle-bin-empty',
    );
  });

  it('resetSettings restores the defaults, emits dock://settings and re-renders', async () => {
    const b = fast();
    const states = recordStates(b);
    const events: Settings[] = [];
    b.on('dock://settings', (s) => {
      events.push(s);
    });
    const before = await b.getSettings();
    before.iconSize = 64;
    await b.updateSettings(before);

    const reset = await b.resetSettings();
    expect(reset).toEqual(defaultSettings());
    expect(events.at(-1)).toEqual(defaultSettings());
    expect(await b.getSettings()).toEqual(defaultSettings());
    // The demo pins are gone, so the pinned group disappeared from the dock.
    expect(states.at(-1)!.items.filter((i) => i.pinned)).toHaveLength(0);
    expect(keys(states.at(-1)!)).toContain(app(FIREFOX)); // still running
  });

  it('openSettings is recorded like the other side effects', async () => {
    const b = fast();
    await b.openSettings();
    expect(b.calls).toEqual([{ type: 'openSettings' }]);
  });

  it('setDockAutoHidden records each report; requestShow emits dock://show with no payload', async () => {
    const b = fast();
    const shows: unknown[] = [];
    const off = b.on('dock://show', (payload) => {
      shows.push(payload);
    });
    const states = recordStates(b);

    await b.setDockAutoHidden(true);
    await b.setDockAutoHidden(false);
    expect(b.calls).toEqual([
      { type: 'setDockAutoHidden', hidden: true },
      { type: 'setDockAutoHidden', hidden: false },
    ]);
    // The flag is the app layer's; nothing in the snapshot changes.
    expect(states).toHaveLength(0);

    b.requestShow();
    expect(shows).toEqual([null]);
    b.requestShow();
    expect(shows).toEqual([null, null]);
    expect(states).toHaveLength(0);

    off();
    b.requestShow();
    expect(shows).toHaveLength(2);
  });

  it('setFullscreen mirrors FullscreenChanged: in the snapshot, one state event per change', async () => {
    const b = fast();
    const states = recordStates(b);
    const before = await b.getDockState();
    expect(before.fullscreen).toBe(false);
    // The demo windows carry no geometry, like the Rust demo.
    expect(item(before, app(FIREFOX)).windows).toHaveLength(2);

    // The first verdict is usually `false`: no change, no event.
    b.setFullscreen(false);
    expect(states).toHaveLength(0);

    b.setFullscreen(true);
    expect(states).toHaveLength(1);
    expect(states[0]!.fullscreen).toBe(true);
    expect(states[0]!.revision).toBe(before.revision + 1);
    // Items and the foreground are untouched by it.
    expect(keys(states[0]!)).toEqual(keys(before));
    expect(states[0]!.foreground).toBe(before.foreground);
    expect((await b.getDockState()).fullscreen).toBe(true);

    b.setFullscreen(true);
    expect(states).toHaveLength(1);
    b.setFullscreen(false);
    expect(states).toHaveLength(2);
    expect(states[1]!.fullscreen).toBe(false);
    expect(states[1]!.revision).toBe(before.revision + 2);
  });
});

describe('a window over the dock (mirror of crates/stem-core/src/overlap.rs)', () => {
  /** A 560 × 72 panel at the bottom of the mock monitor's 2560 × 1392 work area. */
  const PANEL: Rect = { x: 1000, y: 1320, width: 560, height: 72 };
  /** A frame covering the panel's left half. */
  const OVER: Rect = { x: 600, y: 900, width: 700, height: 480 };
  /** A frame in the top-left corner, clear of the panel. */
  const CLEAR: Rect = { x: 0, y: 0, width: 800, height: 600 };

  function framed(id: number, rect: Rect | null, extra: Partial<RawWindow> = {}): RawWindow {
    return {
      id,
      title: `W${id}`,
      className: 'Window',
      processId: 1000 + id,
      exePath: 'C:\\a.exe',
      appUserModelId: null,
      isVisible: true,
      isMinimized: false,
      isCloaked: false,
      monitor: null,
      rect,
      iconKey: null,
      ...extra,
    };
  }

  it('rectsIntersect needs a positive shared area; touching edges do not count', () => {
    const a = { x: 0, y: 0, width: 10, height: 10 };
    expect(rectsIntersect(a, { x: 5, y: 5, width: 10, height: 10 })).toBe(true);
    expect(rectsIntersect(a, { x: 2, y: 2, width: 2, height: 2 })).toBe(true);
    expect(rectsIntersect(a, { x: 10, y: 0, width: 10, height: 10 })).toBe(false);
    expect(rectsIntersect(a, { x: 0, y: 10, width: 10, height: 10 })).toBe(false);
    expect(rectsIntersect(a, { x: 10, y: 10, width: 10, height: 10 })).toBe(false);
    expect(rectsIntersect(a, { x: 5, y: 5, width: 0, height: 10 })).toBe(false);
    expect(rectsIntersect(a, { x: 5, y: 5, width: 10, height: -1 })).toBe(false);
  });

  it('overlapped: a shown window of another process whose frame intersects the panel', () => {
    const over = framed(1, OVER);
    const beside = framed(2, { x: 0, y: 0, width: 900, height: 1392 });
    const above = framed(3, { x: 0, y: 0, width: 2560, height: 1320 });
    expect(overlapped(PANEL, [beside, above, over], null)).toBe(true);
    expect(overlapped(PANEL, [beside, above], null)).toBe(false);
    expect(overlapped(PANEL, [], null)).toBe(false);
    expect(overlapped(null, [over], null)).toBe(false);
    expect(overlapped({ ...PANEL, width: 0 }, [over], null)).toBe(false);
    // One pixel is enough; an edge is not.
    expect(
      overlapped(PANEL, [framed(4, { x: 1559, y: 1391, width: 100, height: 100 })], null),
    ).toBe(true);
    expect(
      overlapped(PANEL, [framed(5, { x: 1560, y: 1320, width: 100, height: 100 })], null),
    ).toBe(false);
    // Not on screen, or no frame: never.
    const frame = { x: 0, y: 0, width: 2560, height: 1392 };
    expect(overlapped(PANEL, [framed(6, frame, { isMinimized: true })], null)).toBe(false);
    expect(overlapped(PANEL, [framed(7, frame, { isCloaked: true })], null)).toBe(false);
    expect(overlapped(PANEL, [framed(8, frame, { isVisible: false })], null)).toBe(false);
    expect(overlapped(PANEL, [framed(9, null)], null)).toBe(false);
    // Our own windows never count.
    expect(overlapped(PANEL, [framed(10, frame)], 1010)).toBe(false);
    expect(overlapped(PANEL, [framed(10, frame)], 1011)).toBe(true);
  });

  it('panelOnScreen anchors the viewport to the bottom-left of the work area', () => {
    const workArea = { x: 0, y: 0, width: 2560, height: 1392 };
    // Playwright's 1280 × 240 viewport: the panel at y 168 rests 72 px above the screen's bottom.
    expect(panelOnScreen({ x: 360, y: 168, width: 561, height: 64 }, workArea, 240)).toEqual({
      x: 360,
      y: 1320,
      width: 561,
      height: 64,
    });
    // The window grew for a drag (viewport 960 high; the UI reports y + 720): the same spot.
    expect(panelOnScreen({ x: 360, y: 888, width: 561, height: 64 }, workArea, 960)).toEqual({
      x: 360,
      y: 1320,
      width: 561,
      height: 64,
    });
    // A work area that does not start at the origin.
    expect(
      panelOnScreen(
        { x: 10, y: 20, width: 30, height: 40 },
        { x: -1920, y: 100, width: 1920, height: 900 },
        100,
      ),
    ).toEqual({ x: -1910, y: 920, width: 30, height: 40 });
  });

  it('the demo windows have frames on the mock monitor, none over a panel at its bottom', async () => {
    const b = fast();
    const panel = (await b.getDockState()).items; // identities resolved; windows are in the engine
    expect(panel.length).toBeGreaterThan(0);
    const windows = [...b['engine'].windows.values()].map((t) => t.raw);
    expect(windows).toHaveLength(6);
    for (const w of windows) {
      expect(w.rect).not.toBeNull();
      expect(w.rect!.x).toBeGreaterThanOrEqual(0);
      expect(w.rect!.y).toBeGreaterThanOrEqual(0);
      expect(w.rect!.x + w.rect!.width).toBeLessThanOrEqual(2560);
      expect(w.rect!.y + w.rect!.height).toBeLessThanOrEqual(1392 - 250);
    }
    expect(overlapped({ x: 0, y: 1392 - 250, width: 2560, height: 250 }, windows, null)).toBe(
      false,
    );
    // Twenty apps, nineteen windows (Chrome has three; eight pinned apps have none).
    const many = fast({ scenario: 'many' });
    const manyWindows = [...many['engine'].windows.values()].map((t) => t.raw);
    expect(manyWindows).toHaveLength(19);
    expect(overlapped({ x: 0, y: 1392 - 250, width: 2560, height: 250 }, manyWindows, null)).toBe(
      false,
    );
  });

  it('setDockPanelRect and moveWindow drive DockState.overlapped; only flips emit a state', async () => {
    // No `window` in node: viewport px are screen px (see MockOptions.viewportHeight).
    const b = fast();
    const states = recordStates(b);
    const before = await b.getDockState();
    expect(before.overlapped).toBe(false);
    expect(b.dockPanel()).toBeNull();

    await b.setDockPanelRect(PANEL);
    expect(b.calls).toEqual([{ type: 'setDockPanelRect', rect: PANEL }]);
    expect(b.dockPanel()).toEqual(PANEL);
    // Every demo window is clear of it: nothing emitted.
    expect(states).toHaveLength(0);

    // Code's window is dragged over the dock: a rect-only update, one state.
    b.moveWindow(4, OVER);
    expect(states).toHaveLength(1);
    expect(states[0]!.overlapped).toBe(true);
    expect(states[0]!.revision).toBe(before.revision + 1);
    expect(keys(states[0]!)).toEqual(keys(before));
    expect(states[0]!.foreground).toBe(before.foreground);
    // A nudge that keeps it over the dock: stored, nothing emitted.
    b.moveWindow(4, { ...OVER, x: OVER.x + 20 });
    expect(states).toHaveLength(1);
    expect(b['engine'].windows.get(4)!.raw.rect).toEqual({ ...OVER, x: OVER.x + 20 });
    // Away again: cleared.
    b.moveWindow(4, CLEAR);
    expect(states).toHaveLength(2);
    expect(states[1]!.overlapped).toBe(false);

    // A minimised window over the dock does not count; restoring it does.
    b.moveWindow(6, OVER);
    expect(states).toHaveLength(2);
    await b.windowAction(6, 'restore');
    expect(states.at(-1)!.overlapped).toBe(true);
    // An unreadable frame says nothing.
    b.moveWindow(6, null);
    expect(states.at(-1)!.overlapped).toBe(false);
    // Unknown windows are ignored.
    const count = states.length;
    b.moveWindow(99, OVER);
    expect(states).toHaveLength(count);

    // The panel moves (a wider dock) but stays clear: nothing emitted; onto
    // a window: set.
    await b.setDockPanelRect({ ...PANEL, x: 900, width: 760 });
    expect(states).toHaveLength(count);
    await b.setDockPanelRect({ x: 200, y: 40, width: 100, height: 100 });
    expect(states.at(-1)!.overlapped).toBe(true);
    expect((await b.getDockState()).overlapped).toBe(true);
  });

  it('a viewport height places the reported panel like the app layer does', async () => {
    const b = fast({ viewportHeight: 240 });
    await b.setDockPanelRect({ x: 360, y: 168, width: 561, height: 64 });
    expect(b.dockPanel()).toEqual({ x: 360, y: 1320, width: 561, height: 64 });
    expect((await b.getDockState()).overlapped).toBe(false);
    b.moveWindow(1, { x: 0, y: 1200, width: 2560, height: 192 });
    expect((await b.getDockState()).overlapped).toBe(true);
  });
});

describe('context menus (docs/ipc/context-menus.md, v1.2; native in Rust, recorded here)', () => {
  it('records show_item_menu and show_dock_menu with the item key and the point', async () => {
    const b = fast();
    await b.showItemMenu(app(FIREFOX), 120, 40);
    await b.showItemMenu('fixed:recycle-bin', 1000.5, 41);
    await b.showDockMenu(300, 12);
    expect(b.calls).toEqual([
      { type: 'showItemMenu', itemKey: app(FIREFOX), x: 120, y: 40 },
      { type: 'showItemMenu', itemKey: 'fixed:recycle-bin', x: 1000.5, y: 41 },
      { type: 'showDockMenu', x: 300, y: 12 },
    ]);
  });

  it('shows nothing for a separator and rejects an unknown item like Rust', async () => {
    const b = fast();
    await b.showItemMenu('sep:1', 5, 5);
    await expect(b.showItemMenu('app:exe:c:/nope.exe', 5, 5)).rejects.toBe(
      'unknown dock item: app:exe:c:/nope.exe',
    );
    expect(b.calls).toEqual([]);
  });

  it('reports the request with console.info when logging is on', async () => {
    const info = vi.spyOn(console, 'info').mockImplementation(() => undefined);
    try {
      const b = new MockBackend({ latencyMs: [0, 0], identityDelayMs: 0, log: true });
      backends.push(b);
      await b.showItemMenu('fixed:launcher', 10, 20);
      await b.showDockMenu(1, 2);
      expect(info).toHaveBeenCalledWith('[mock] show_item_menu(fixed:launcher, 10, 20)');
      expect(info).toHaveBeenCalledWith('[mock] show_dock_menu(1, 2)');
    } finally {
      info.mockRestore();
    }
  });

  it('failNextCommand rejects exactly one command with the given message', async () => {
    const b = fast();
    b.failNextCommand('boom');
    await expect(b.getDockState()).rejects.toBe('boom');
    await expect(b.getDockState()).resolves.toBeTruthy();
  });
});

describe('icons', () => {
  it('returns null for shell and unknown keys, deterministic SVG data URLs otherwise', async () => {
    const b = fast();
    expect(await b.getIcon('shell:start', 48)).toBeNull();
    expect(await b.getIcon('hwnd:1', 48)).toBeNull();
    const a = await b.getIcon(`mock:${FIREFOX}`, 96);
    const again = await b.getIcon(`mock:${FIREFOX}`, 96);
    expect(a).toBe(again);
    expect(a).toMatch(/^data:image\/svg\+xml;utf8,/);
    const svg = decodeURIComponent(a!.slice('data:image/svg+xml;utf8,'.length));
    expect(svg).toContain('width="96"');
    expect(svg).toContain('>F</text>');
    // The labelled Settings entry draws its label's initial.
    expect(decodeURIComponent((await b.getIcon(`mock:${SETTINGS}`, 48))!)).toContain('>S</text>');
  });

  it('hashes keys to stable colours in range', () => {
    const a = colorFor('mock:exe:x');
    expect(colorFor('mock:exe:x')).toEqual(a);
    expect(colorFor('mock:exe:y')).not.toEqual(a);
    for (const channel of a) {
      expect(channel).toBeGreaterThanOrEqual(0);
      expect(channel).toBeLessThanOrEqual(255);
      expect(Number.isInteger(channel)).toBe(true);
    }
  });
});

describe('helpers mirrored from stem-core', () => {
  it('fileStem treats both separators alike', () => {
    expect(fileStem('C:\\A\\b.EXE')).toBe('b');
    expect(fileStem('c:/a/b')).toBe('b');
    expect(fileStem('.hidden')).toBe('.hidden');
  });

  it('validateSettings clamps and dedupes like Settings::validate', () => {
    const s = defaultSettings();
    s.iconSize = 999;
    s.magnification.maxScale = 9;
    s.magnification.rangeIcons = 0;
    s.appearance.opacity = -1;
    s.pinned = [
      { id: 'a', label: null, launch: { kind: 'uri', uri: 'a' } },
      { id: 'a', label: null, launch: { kind: 'uri', uri: 'a' } },
      { id: '', label: null, launch: { kind: 'uri', uri: '' } },
      { id: 'b', label: null, launch: { kind: 'uri', uri: 'b' } },
    ];
    const v = validateSettings(s);
    expect(v.iconSize).toBe(128);
    expect(v.magnification.maxScale).toBe(2.5);
    expect(v.magnification.rangeIcons).toBe(1);
    expect(v.appearance.opacity).toBe(0);
    expect(v.pinned.map((p) => p.id)).toEqual(['a', 'b']);
    // Idempotent and non-mutating.
    expect(validateSettings(v)).toEqual(v);
    expect(s.iconSize).toBe(999);
  });
});

describe('the "many" scenario', () => {
  it('has twenty app items, eight of them pinned', async () => {
    const b = fast({ scenario: 'many' });
    const state = await b.getDockState();
    const apps = state.items.filter((i) => i.kind === 'app');
    expect(apps).toHaveLength(20);
    expect(apps.filter((i) => i.pinned)).toHaveLength(8);
    expect(apps.find((i) => i.title === 'Chrome')?.windows).toHaveLength(3);
    expect(apps.find((i) => i.title === 'Chrome')?.isActive).toBe(true);
  });
});
