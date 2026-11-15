import { afterEach, describe, expect, it, vi } from 'vitest';

import { panelRectReport } from '$lib/dock/geometry';
import type { DockState } from '$lib/ipc/generated/DockState';
import type { Settings } from '$lib/ipc/generated/Settings';

import type { BouncePayload, FileDragEvent } from './backend';
import { applyDemoQuery, demoPosition } from './demo';
import type { RawWindow } from '$lib/ipc/generated/RawWindow';
import type { Rect } from '$lib/ipc/generated/Rect';

import type { UserCss } from '$lib/ipc/generated/UserCss';

import {
  appIdForExe,
  colorFor,
  defaultSettings,
  DEMO_USER_CSS_PATH,
  demoAppInfo,
  dockLabel,
  fileStem,
  MOCK_USER_CSS_TEMPLATE,
  MockBackend,
  nothingToRecycle,
  overlapped,
  panelOnScreen,
  reanchorPanel,
  readUserCss,
  rectsIntersect,
  recyclePlan,
  SECOND_MONITOR_ID,
  USER_CSS_MAX_BYTES,
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

/**
 * The mock monitor's work area as a viewport: what a backend without
 * `viewportWidth`/`viewportHeight` takes for the window's in node (viewport
 * px are screen px), so a panel reported in it is not re-anchored.
 */
const SCREEN = { width: 2560, height: 1392 } as const;

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

  describe('Quick Settings and Notification Center', () => {
    async function withItems(
      b: MockBackend,
      items: Partial<Settings['items']>,
    ): Promise<DockState> {
      const settings = await b.getSettings();
      settings.items = { ...settings.items, ...items };
      await b.updateSettings(settings);
      return b.getDockState();
    }

    it('are off by default, in the demo too', async () => {
      const b = fast();
      expect(defaultSettings().items.quickSettings).toBe(false);
      expect(defaultSettings().items.notificationCenter).toBe(false);
      const kinds = (await b.getDockState()).items.map((i) => i.kind);
      expect(kinds).not.toContain('quickSettings');
      expect(kinds).not.toContain('notificationCenter');
    });

    it('sit before the Recycle Bin in that order, after one separator', async () => {
      const b = fast();
      const states = recordStates(b);
      const state = await withItems(b, { quickSettings: true, notificationCenter: true });
      expect(states).toHaveLength(1);
      expect(keys(state).slice(-6)).toEqual([
        app(CODE),
        app(SPOTIFY),
        'sep:3',
        'fixed:quick-settings',
        'fixed:notification-center',
        'fixed:recycle-bin',
      ]);
      const qs = item(state, 'fixed:quick-settings');
      expect(qs).toMatchObject({
        kind: 'quickSettings',
        title: 'Quick Settings',
        iconKey: 'shell:quick-settings',
        appId: null,
        pinned: false,
        windows: [],
      });
      expect(item(state, 'fixed:notification-center')).toMatchObject({
        kind: 'notificationCenter',
        title: 'Notification Center',
        iconKey: 'shell:notification-center',
        appId: null,
        pinned: false,
        windows: [],
      });

      // Either one alone keeps its place before the bin.
      expect(keys(await withItems(b, { notificationCenter: false })).slice(-3)).toEqual([
        'sep:3',
        'fixed:quick-settings',
        'fixed:recycle-bin',
      ]);
      expect(
        keys(await withItems(b, { quickSettings: false, notificationCenter: true })).slice(-3),
      ).toEqual(['sep:3', 'fixed:notification-center', 'fixed:recycle-bin']);
    });

    it('close the dock without the bin, and draw no separator alone or with separators off', async () => {
      const b = fast();
      let state = await withItems(b, {
        quickSettings: true,
        notificationCenter: true,
        recycleBin: false,
      });
      expect(keys(state).slice(-4)).toEqual([
        app(SPOTIFY),
        'sep:3',
        'fixed:quick-settings',
        'fixed:notification-center',
      ]);

      state = await withItems(b, { separators: false });
      expect(state.items.filter((i) => i.kind === 'separator')).toHaveLength(0);
      expect(keys(state).slice(-3)).toEqual([
        app(SPOTIFY),
        'fixed:quick-settings',
        'fixed:notification-center',
      ]);

      // Nothing but the two: no separator before them.
      const settings = await b.getSettings();
      settings.pinned = [];
      settings.items = { ...settings.items, launcher: false, separators: true };
      await b.updateSettings(settings);
      for (const id of [1, 2, 3, 4, 5, 6]) {
        await b.windowAction(id, 'close');
      }
      expect(keys(await b.getDockState())).toEqual([
        'fixed:quick-settings',
        'fixed:notification-center',
      ]);
    });

    it('a click runs their shell actions, which change nothing else', async () => {
      const b = fast();
      await withItems(b, { quickSettings: true, notificationCenter: true });
      const states = recordStates(b);
      await b.primaryClick('fixed:quick-settings');
      await b.primaryClick('fixed:notification-center');
      expect(b.calls).toEqual([
        { type: 'shell', action: 'openQuickSettings' },
        { type: 'shell', action: 'openNotificationCenter' },
      ]);
      await b.shellAction('openQuickSettings');
      expect(b.calls.at(-1)).toEqual({ type: 'shell', action: 'openQuickSettings' });
      expect(states).toHaveLength(0);
      // The icon keys are not item keys.
      await expect(b.primaryClick('shell:quick-settings')).rejects.toMatch(/unknown dock item/);
    });
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

  it('getAppInfo answers the demo facts until setAppInfo changes them; openAppFolder is recorded', async () => {
    const b = fast();
    expect(await b.getAppInfo()).toEqual(demoAppInfo());
    expect(await b.getAppInfo()).toEqual({
      version: '0.1.0-mock',
      settingsPath: 'C:\\Users\\me\\AppData\\Roaming\\dev.davidcristian.stem\\settings.json',
      logPath: 'C:\\Users\\me\\AppData\\Local\\dev.davidcristian.stem\\logs\\stem.log',
      platform: 'mock',
    });
    // A run with file logging off; the other facts stay.
    b.setAppInfo({ logPath: '' });
    expect(await b.getAppInfo()).toEqual({ ...demoAppInfo(), logPath: '' });

    const states = recordStates(b);
    await b.openAppFolder('settings');
    await b.openAppFolder('logs');
    expect(b.calls).toEqual([
      { type: 'openAppFolder', folder: 'settings' },
      { type: 'openAppFolder', folder: 'logs' },
    ]);
    // Nothing in the snapshot or the settings changes.
    expect(states).toHaveLength(0);
  });

  it('the user css: read at start from MockOptions.userCss, again on reload, which emits it; open writes the template only once', async () => {
    const none = fast();
    expect(await none.getUserCss()).toEqual({ path: DEMO_USER_CSS_PATH, css: null });

    const css = '.dock-stage { --stem-dot: #ffb900; }';
    const b = fast({ userCss: css });
    const events: UserCss[] = [];
    b.on('dock://user-css', (payload) => {
      events.push(payload);
    });
    const states = recordStates(b);
    expect(await b.getUserCss()).toEqual({ path: DEMO_USER_CSS_PATH, css });

    // An edit shows only once reloaded; the reload answers what it sent.
    b.writeUserCss('.dock-stage { --stem-dot: red; }');
    expect((await b.getUserCss()).css).toBe(css);
    expect(events).toHaveLength(0);
    const reloaded = await b.reloadUserCss();
    expect(reloaded).toEqual({ path: DEMO_USER_CSS_PATH, css: '.dock-stage { --stem-dot: red; }' });
    expect(events).toEqual([reloaded]);
    expect(await b.getUserCss()).toEqual(reloaded);

    // A deleted file, then a too large one: no CSS, still sent.
    b.writeUserCss(null);
    expect((await b.reloadUserCss()).css).toBeNull();
    b.writeUserCss('a'.repeat(USER_CSS_MAX_BYTES + 1));
    expect((await b.reloadUserCss()).css).toBeNull();
    expect(events.map((e) => e.css)).toEqual(['.dock-stage { --stem-dot: red; }', null, null]);

    // Open creates the template when there is no file, and never again.
    b.writeUserCss(null);
    await b.openUserCss();
    expect(b.userCssOnDisk()).toBe(MOCK_USER_CSS_TEMPLATE);
    b.writeUserCss('mine');
    await b.openUserCss();
    expect(b.userCssOnDisk()).toBe('mine');
    expect(b.calls).toEqual([
      { type: 'reloadUserCss' },
      { type: 'reloadUserCss' },
      { type: 'reloadUserCss' },
      { type: 'openUserCss', created: true },
      { type: 'openUserCss', created: false },
    ]);
    expect(states).toHaveLength(0);
  });

  it('readUserCss mirrors the Rust read: 64 KiB of UTF-8 at most, a leading BOM dropped', () => {
    const bom = String.fromCharCode(0xfeff);
    const eAcute = String.fromCharCode(0xe9);
    expect(readUserCss(null)).toBeNull();
    expect(readUserCss('')).toBe('');
    expect(readUserCss(`${bom}a {}`)).toBe('a {}');
    expect(readUserCss(`a${bom}`)).toBe(`a${bom}`);
    expect(readUserCss('a'.repeat(USER_CSS_MAX_BYTES))).toHaveLength(USER_CSS_MAX_BYTES);
    expect(readUserCss('a'.repeat(USER_CSS_MAX_BYTES + 1))).toBeNull();
    // Bytes, not characters: an e with an acute accent is two bytes in UTF-8.
    expect(readUserCss(eAcute.repeat(USER_CSS_MAX_BYTES / 2))).not.toBeNull();
    expect(readUserCss(`${eAcute.repeat(USER_CSS_MAX_BYTES / 2)}a`)).toBeNull();
    // The BOM's three bytes count.
    expect(readUserCss(`${bom}${'a'.repeat(USER_CSS_MAX_BYTES - 3)}`)).toHaveLength(
      USER_CSS_MAX_BYTES - 3,
    );
    expect(readUserCss(`${bom}${'a'.repeat(USER_CSS_MAX_BYTES - 2)}`)).toBeNull();
  });

  it('the default settings have the standard theme preset, like Settings::default()', () => {
    expect(defaultSettings().appearance.theme).toBe('standard');
    const solid = defaultSettings();
    solid.appearance.theme = 'solid';
    expect(validateSettings(solid).appearance.theme).toBe('solid');
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

  it('requestFocus emits dock://focus, after dock://show while the page reports its panel slid out; releaseDockFocus is recorded', async () => {
    const b = fast();
    const events: string[] = [];
    const offs = (['dock://show', 'dock://focus', 'dock://release-focus'] as const).map((name) =>
      b.on(name, (payload) => {
        events.push(`${name.slice('dock://'.length)}:${String(payload)}`);
      }),
    );
    const states = recordStates(b);

    b.requestFocus();
    expect(events).toEqual(['focus:null']);
    // The page gives the focus back; with the panel slid out, the next
    // press brings it back first.
    await b.releaseDockFocus();
    await b.setDockAutoHidden(true);
    events.length = 0;
    b.requestFocus();
    expect(events).toEqual(['show:null', 'focus:null']);
    await b.releaseDockFocus();
    await b.setDockAutoHidden(false);
    events.length = 0;
    b.requestFocus();
    expect(events).toEqual(['focus:null']);

    await b.releaseDockFocus();
    await b.releaseDockFocus();
    expect(b.calls.filter((c) => c.type === 'releaseDockFocus')).toHaveLength(4);
    expect(b.calls.filter((c) => c.type === 'releaseDockFocus').at(-1)).toEqual({
      type: 'releaseDockFocus',
    });
    // Neither touches the snapshot or the settings.
    expect(states).toHaveLength(0);

    for (const off of offs) {
      off();
    }
    b.requestFocus();
    expect(events).toEqual(['focus:null']);
  });

  it('a second requestFocus while the dock has the keyboard emits dock://release-focus, until the focus is given back or another window comes to the front', async () => {
    const b = fast();
    const events: string[] = [];
    const offs = (['dock://show', 'dock://focus', 'dock://release-focus'] as const).map((name) =>
      b.on(name, () => {
        events.push(name.slice('dock://'.length));
      }),
    );
    const press = (): string[] => {
      events.length = 0;
      b.requestFocus();
      return [...events];
    };

    // Enter, then a second press lets go; the page answers with
    // releaseDockFocus, and the third press enters again.
    expect(press()).toEqual(['focus']);
    expect(press()).toEqual(['release-focus']);
    expect(press()).toEqual(['release-focus']);
    await b.releaseDockFocus();
    expect(press()).toEqual(['focus']);

    // A click that minimises or launches leaves the dock in front with the
    // keyboard: the next press still lets go.
    const state = await b.getDockState();
    const shown = state.items.flatMap((item) => item.windows).find((w) => !w.isMinimized)!;
    await b.windowAction(shown.id, 'minimize');
    expect(press()).toEqual(['release-focus']);
    const app = state.items.find((item) => item.kind === 'app' && item.appId !== null)!;
    await b.launchApp(app.appId!);
    expect(b.calls.filter((c) => c.type === 'launch')).toHaveLength(1);
    expect(press()).toEqual(['release-focus']);

    // A window brought to the front (a click that activates, or one from
    // elsewhere) takes the activation from the dock: the next press enters.
    await b.windowAction(shown.id, 'activate');
    expect(press()).toEqual(['focus']);
    b.pushForeground(null);
    expect(press()).toEqual(['focus']);

    for (const off of offs) {
      off();
    }
  });

  it('setFullscreen mirrors FullscreenChanged: in the dock’s facts, one state event per change', async () => {
    const b = fast();
    const states = recordStates(b);
    // The page registers its dock window (no panel rect yet): one entry on
    // the primary monitor with both facts clear.
    expect((await b.getDockState()).docks).toEqual([]);
    await b.setDockPanelRect({ x: 1000, y: 1320, width: 560, height: 72 }, 'bottom', SCREEN);
    const before = await b.getDockState();
    expect(before.docks).toEqual([
      { monitor: 'mock-primary', fullscreen: false, overlapped: false },
    ]);
    expect(item(before, app(FIREFOX)).windows).toHaveLength(2);
    states.length = 0;

    // The first verdict is usually `false`: no change, no event.
    b.setFullscreen(false);
    expect(states).toHaveLength(0);

    // A verdict naming the dock's monitor.
    b.setFullscreen(true, 'mock-primary');
    expect(states).toHaveLength(1);
    expect(states[0]!.docks).toEqual([
      { monitor: 'mock-primary', fullscreen: true, overlapped: false },
    ]);
    expect(states[0]!.revision).toBe(before.revision + 1);
    // Items and the foreground are untouched by it.
    expect(keys(states[0]!)).toEqual(keys(before));
    expect(states[0]!.foreground).toBe(before.foreground);
    expect((await b.getDockState()).docks[0]!.fullscreen).toBe(true);

    b.setFullscreen(true, 'mock-primary');
    expect(states).toHaveLength(1);
    b.setFullscreen(false);
    expect(states).toHaveLength(2);
    expect(states[1]!.docks[0]!.fullscreen).toBe(false);
    expect(states[1]!.revision).toBe(before.revision + 2);

    // A verdict with no monitor lands on the primary's dock; one about a
    // monitor without a dock hides nobody.
    b.setFullscreen(true);
    expect(states.at(-1)!.docks[0]!.fullscreen).toBe(true);
    b.setFullscreen(true, '\\\\.\\DISPLAY9');
    expect(states.at(-1)!.docks[0]!.fullscreen).toBe(false);
    expect(states).toHaveLength(4);
  });
});

describe('virtual desktops (mirror of DockEngine::is_shown)', () => {
  const PAINT = 'C:\\Windows\\System32\\mspaint.exe';
  const CALC = 'C:\\Program Files\\WindowsApps\\Calculator\\CalculatorApp.exe';
  const WORDPAD = 'C:\\Program Files\\Windows NT\\Accessories\\wordpad.exe';

  function raw(id: number, exe: string, extra: Partial<RawWindow> = {}): RawWindow {
    return {
      id,
      title: `W${id}`,
      className: 'Window',
      processId: 1000 + id,
      exePath: exe,
      appUserModelId: null,
      isVisible: true,
      isMinimized: false,
      isCloaked: false,
      onCurrentDesktop: null,
      monitor: null,
      rect: null,
      iconKey: null,
      ...extra,
    };
  }

  it('onCurrentDesktop decides while showOnlyCurrentDesktop is on; the cloak state only when unknown', async () => {
    const b = fast();
    // Parked on another desktop (the shell cloaks it too).
    b.pushWindow(raw(90, PAINT, { isCloaked: true, onCurrentDesktop: false }));
    // Cloaked on this desktop: a suspended Store app, or the switch animation.
    b.pushWindow(raw(91, CALC, { isCloaked: true, onCurrentDesktop: true }));
    // Unknown: cloaked counts as away.
    b.pushWindow(raw(92, WORDPAD, { isCloaked: true }));
    let state = await b.getDockState();
    expect(keys(state)).not.toContain(app(appIdForExe(PAINT)));
    expect(keys(state)).toContain(app(appIdForExe(CALC)));
    expect(keys(state)).not.toContain(app(appIdForExe(WORDPAD)));

    // With the setting off every window is listed whatever the desktop.
    const settings = await b.getSettings();
    settings.behavior.showOnlyCurrentDesktop = false;
    await b.updateSettings(settings);
    state = await b.getDockState();
    for (const exe of [PAINT, CALC, WORDPAD]) {
      expect(keys(state)).toContain(app(appIdForExe(exe)));
    }

    // A desktop switch, as the events thread reports it: one update per
    // window, then a snapshot with the same facts (absorbed).
    settings.behavior.showOnlyCurrentDesktop = true;
    await b.updateSettings(settings);
    b.pushWindow(raw(90, PAINT, { onCurrentDesktop: true }));
    b.pushWindow(raw(91, CALC, { isCloaked: true, onCurrentDesktop: false }));
    state = await b.getDockState();
    expect(keys(state)).toContain(app(appIdForExe(PAINT)));
    expect(keys(state)).not.toContain(app(appIdForExe(CALC)));
    expect(keys(state)).not.toContain(app(appIdForExe(WORDPAD)));
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
      onCurrentDesktop: true,
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
    const viewport = { width: 1280, height: 240 };
    expect(
      panelOnScreen({ x: 360, y: 168, width: 561, height: 64 }, workArea, viewport, 'bottom'),
    ).toEqual({ x: 360, y: 1320, width: 561, height: 64 });
    // The window grew for a drag (viewport 960 high; the UI reports y + 720): the same spot.
    expect(
      panelOnScreen(
        { x: 360, y: 888, width: 561, height: 64 },
        workArea,
        { width: 1280, height: 960 },
        'bottom',
      ),
    ).toEqual({ x: 360, y: 1320, width: 561, height: 64 });
    // A work area that does not start at the origin.
    expect(
      panelOnScreen(
        { x: 10, y: 20, width: 30, height: 40 },
        { x: -1920, y: 100, width: 1920, height: 900 },
        { width: 1920, height: 100 },
        'bottom',
      ),
    ).toEqual({ x: -1910, y: 920, width: 30, height: 40 });
  });

  it('panelOnScreen anchors the viewport to the corner of the work area on the dock\u2019s edge', () => {
    const workArea = { x: 0, y: 0, width: 2560, height: 1392 };
    // On top and on the left the viewport's top-left corner is the work area's.
    expect(
      panelOnScreen(
        { x: 360, y: 8, width: 561, height: 64 },
        workArea,
        { width: 1280, height: 240 },
        'top',
      ),
    ).toEqual({ x: 360, y: 8, width: 561, height: 64 });
    expect(
      panelOnScreen(
        { x: 8, y: 400, width: 64, height: 561 },
        workArea,
        { width: 330, height: 1392 },
        'left',
      ),
    ).toEqual({ x: 8, y: 400, width: 64, height: 561 });
    // On the right its top-right corner is: a 330 px wide viewport starts at x = 2230.
    expect(
      panelOnScreen(
        { x: 258, y: 400, width: 64, height: 561 },
        workArea,
        { width: 330, height: 1392 },
        'right',
      ),
    ).toEqual({ x: 2488, y: 400, width: 64, height: 561 });
    // The window grew for a drag on the right (viewport 2560 wide; the UI reports x + 2230): the same spot.
    expect(
      panelOnScreen(
        { x: 2488, y: 400, width: 64, height: 561 },
        workArea,
        { width: 2560, height: 1392 },
        'right',
      ),
    ).toEqual({ x: 2488, y: 400, width: 64, height: 561 });
    // A work area that does not start at the origin (a taskbar on the left, a monitor above).
    const offset = { x: 62, y: -1080, width: 1858, height: 1080 };
    expect(
      panelOnScreen(
        { x: 8, y: 20, width: 64, height: 40 },
        offset,
        { width: 300, height: 1080 },
        'left',
      ),
    ).toEqual({ x: 70, y: -1060, width: 64, height: 40 });
    expect(
      panelOnScreen(
        { x: 228, y: 20, width: 64, height: 40 },
        offset,
        { width: 300, height: 1080 },
        'right',
      ),
    ).toEqual({ x: 62 + 1858 - 300 + 228, y: -1060, width: 64, height: 40 });
    expect(
      panelOnScreen(
        { x: 10, y: 8, width: 30, height: 64 },
        offset,
        { width: 1858, height: 200 },
        'top',
      ),
    ).toEqual({ x: 72, y: -1072, width: 30, height: 64 });
  });

  it('panelOnScreen scales the viewport on the right edge for a monitor at 150 %', () => {
    const workArea = { x: 2560, y: 0, width: 1920, height: 1032 };
    // A 300 × 688 CSS viewport is 450 × 1032 physical px flush with the right edge (x 4030).
    expect(
      panelOnScreen(
        { x: 228, y: 100, width: 64, height: 561 },
        workArea,
        { width: 300, height: 688 },
        'right',
        1.5,
      ),
    ).toEqual({ x: 4030 + 342, y: 150, width: 96, height: 842 });
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

  it('the demo windows keep clear of a vertical panel\u2019s band too, on every monitor', () => {
    for (const scenario of ['default', 'many', 'twoMonitors'] as const) {
      const b = fast({ scenario });
      const windows = [...b['engine'].windows.values()].map((t) => t.raw);
      for (const monitor of b['engine'].monitors) {
        const { x, y, width, height } = monitor.workArea;
        const mine = windows.filter((w) => w.monitor === monitor.id);
        expect(mine.length).toBeGreaterThan(0);
        expect(overlapped({ x, y, width: 250, height }, mine, null)).toBe(false);
        expect(overlapped({ x: x + width - 250, y, width: 250, height }, mine, null)).toBe(false);
      }
    }
  });

  it('a position seeds the scenario\u2019s settings; setPosition changes it like a settings change', async () => {
    expect((await fast().getSettings()).position).toBe('bottom');
    expect((await fast({ position: 'left' }).getSettings()).position).toBe('left');
    const b = fast();
    const seen: string[] = [];
    b.on('dock://settings', (settings) => seen.push(settings.position));
    b.setPosition('right');
    expect(seen).toEqual(['right']);
    expect((await b.getSettings()).position).toBe('right');
  });

  it('the demo page takes its edge from ?position= and ignores anything else', async () => {
    expect(demoPosition('?mock=1&position=top')).toBe('top');
    expect(demoPosition('?position=left&monitor=x')).toBe('left');
    expect(demoPosition('?mock=1')).toBeNull();
    expect(demoPosition('?position=middle')).toBeNull();
    expect(demoPosition('?position=Left')).toBeNull();
    const b = fast();
    applyDemoQuery(b, '?mock=1&position=right');
    expect((await b.getSettings()).position).toBe('right');
    const unchanged = fast();
    applyDemoQuery(unchanged, '?mock=1&position=sideways');
    expect((await unchanged.getSettings()).position).toBe('bottom');
  });

  it('a viewport width places a panel reported on the right edge like the app layer does', async () => {
    const b = fast({ viewportHeight: 1392, viewportWidth: 330, position: 'right' });
    await b.setDockPanelRect({ x: 258, y: 400, width: 64, height: 561 }, 'right', {
      width: 330,
      height: 1392,
    });
    expect(b.dockPanel()).toEqual({ x: 2488, y: 400, width: 64, height: 561 });
    expect(overlappedOn(await b.getDockState())).toBe(false);
    // Terminal's window dragged over the right edge.
    b.moveWindow(5, { x: 1500, y: 500, width: 1060, height: 600 });
    expect(overlappedOn(await b.getDockState())).toBe(true);
    // The same report on the left edge: the viewport's left edge is the work area's.
    const left = fast({ viewportHeight: 1392, viewportWidth: 330, position: 'left' });
    await left.setDockPanelRect({ x: 8, y: 400, width: 64, height: 561 }, 'left', {
      width: 330,
      height: 1392,
    });
    expect(left.dockPanel()).toEqual({ x: 8, y: 400, width: 64, height: 561 });
    expect(overlappedOn(await left.getDockState())).toBe(false);
  });

  /** The primary dock's `overlapped`, or `undefined` without an entry. */
  const overlappedOn = (state: DockState, monitor = 'mock-primary'): boolean | undefined =>
    state.docks.find((d) => d.monitor === monitor)?.overlapped;

  it('setDockPanelRect and moveWindow drive the dock\u2019s overlapped; only flips emit a state', async () => {
    // No `window` in node: viewport px are screen px (see MockOptions.viewportHeight).
    const b = fast();
    const states = recordStates(b);
    const before = await b.getDockState();
    expect(before.docks).toEqual([]);
    expect(b.dockPanel()).toBeNull();
    expect(b.pageLabel).toBe('dock');

    // The first report registers the dock: one state with its entry, clear
    // of every demo window.
    await b.setDockPanelRect(PANEL, 'bottom', SCREEN);
    expect(b.calls).toEqual([
      { type: 'setDockPanelRect', rect: PANEL, edge: 'bottom', viewport: SCREEN },
    ]);
    expect(b.dockPanel()).toEqual(PANEL);
    expect(b.dockPanel('dock')).toEqual(PANEL);
    expect(b.dockPanel('dock-DISPLAY3')).toBeNull();
    expect(states).toHaveLength(1);
    expect(states[0]!.docks).toEqual([
      { monitor: 'mock-primary', fullscreen: false, overlapped: false },
    ]);
    const registered = states[0]!.revision;
    expect(registered).toBe(before.revision + 1);

    // Code's window is dragged over the dock: a rect-only update, one state.
    b.moveWindow(4, OVER);
    expect(states).toHaveLength(2);
    expect(overlappedOn(states[1]!)).toBe(true);
    expect(states[1]!.revision).toBe(registered + 1);
    expect(keys(states[1]!)).toEqual(keys(before));
    expect(states[1]!.foreground).toBe(before.foreground);
    // A nudge that keeps it over the dock: stored, nothing emitted.
    b.moveWindow(4, { ...OVER, x: OVER.x + 20 });
    expect(states).toHaveLength(2);
    expect(b['engine'].windows.get(4)!.raw.rect).toEqual({ ...OVER, x: OVER.x + 20 });
    // Away again: cleared.
    b.moveWindow(4, CLEAR);
    expect(states).toHaveLength(3);
    expect(overlappedOn(states[2]!)).toBe(false);

    // A minimised window over the dock does not count; restoring it does.
    b.moveWindow(6, OVER);
    expect(states).toHaveLength(3);
    await b.windowAction(6, 'restore');
    expect(overlappedOn(states.at(-1)!)).toBe(true);
    // An unreadable frame says nothing.
    b.moveWindow(6, null);
    expect(overlappedOn(states.at(-1)!)).toBe(false);
    // Unknown windows are ignored.
    const count = states.length;
    b.moveWindow(99, OVER);
    expect(states).toHaveLength(count);

    // The panel moves (a wider dock) but stays clear: nothing emitted; onto
    // a window: set.
    await b.setDockPanelRect({ ...PANEL, x: 900, width: 760 }, 'bottom', SCREEN);
    expect(states).toHaveLength(count);
    await b.setDockPanelRect({ x: 200, y: 40, width: 100, height: 100 }, 'bottom', SCREEN);
    expect(overlappedOn(states.at(-1)!)).toBe(true);
    expect(overlappedOn(await b.getDockState())).toBe(true);
  });

  it('a viewport height places the reported panel like the app layer does', async () => {
    const b = fast({ viewportHeight: 240 });
    await b.setDockPanelRect({ x: 360, y: 168, width: 561, height: 64 }, 'bottom', {
      width: 2560,
      height: 240,
    });
    expect(b.dockPanel()).toEqual({ x: 360, y: 1320, width: 561, height: 64 });
    expect(overlappedOn(await b.getDockState())).toBe(false);
    b.moveWindow(1, { x: 0, y: 1200, width: 2560, height: 192 });
    expect(overlappedOn(await b.getDockState())).toBe(true);
  });

  it('panelOnScreen scales the CSS rect and the viewport for a monitor at 150 %', () => {
    const workArea = { x: 2560, y: 0, width: 1920, height: 1032 };
    // A 1280 × 240 CSS viewport is 1920 × 360 physical px: the panel at
    // y 168 css (252 px) rests 108 px above the work area's bottom edge.
    const viewport = { width: 1280, height: 240 };
    expect(
      panelOnScreen({ x: 360, y: 168, width: 561, height: 64 }, workArea, viewport, 'bottom', 1.5),
    ).toEqual({
      x: 2560 + 540,
      y: 1032 - 360 + 252,
      width: 842,
      height: 96,
    });
    // Rounded outwards like a hit region: 11 css → 16.5 → 16; 21 css → 31.5 → 32.
    expect(
      panelOnScreen(
        { x: 11, y: 0, width: 10, height: 8 },
        workArea,
        { width: 1280, height: 100 },
        'bottom',
        1.5,
      ),
    ).toEqual({
      x: 2560 + 16,
      y: 1032 - 150,
      width: 16,
      height: 12,
    });
    // A scale of 1 (or a bad one) is the plain placement.
    const small = { width: 1280, height: 100 };
    expect(
      panelOnScreen({ x: 10, y: 20, width: 30, height: 40 }, workArea, small, 'bottom', 0),
    ).toEqual(panelOnScreen({ x: 10, y: 20, width: 30, height: 40 }, workArea, small, 'bottom'));
  });
});

describe('report provenance (mirror of src-tauri/src/windows.rs)', () => {
  const row: Rect = { x: 960, y: 16, width: 640, height: 64 };
  const topRow: Rect = { x: 960, y: 8, width: 640, height: 64 };
  const column: Rect = { x: 8, y: 400, width: 64, height: 640 };
  const rightColumn: Rect = { x: 78, y: 400, width: 64, height: 640 };
  const wide = { width: 2560, height: 96 };
  const tall = { width: 150, height: 1440 };

  it('reanchorPanel keeps the distance to the edge side and the offset from the centre', () => {
    // A thickness change.
    expect(reanchorPanel(row, 'bottom', wide, { width: 2560, height: 144 })).toEqual({
      ...row,
      y: 64,
    });
    expect(reanchorPanel(topRow, 'top', wide, { width: 2560, height: 144 })).toEqual(topRow);
    expect(reanchorPanel(column, 'left', tall, { width: 198, height: 1440 })).toEqual(column);
    expect(reanchorPanel(rightColumn, 'right', tall, { width: 198, height: 1440 })).toEqual({
      ...rightColumn,
      x: 126,
    });
    // A length change: half of it along the edge.
    expect(reanchorPanel(row, 'bottom', wide, { width: 1920, height: 96 })).toEqual({
      ...row,
      x: 640,
    });
    expect(reanchorPanel(topRow, 'top', wide, { width: 1920, height: 96 })).toEqual({
      ...topRow,
      x: 640,
    });
    expect(reanchorPanel(column, 'left', tall, { width: 150, height: 1040 })).toEqual({
      ...column,
      y: 200,
    });
    expect(reanchorPanel(rightColumn, 'right', tall, { width: 198, height: 1040 })).toEqual({
      ...rightColumn,
      x: 126,
      y: 200,
    });
    // At 1.5x the window's viewport is fractional (2560 x 144 px = 1706.67 x
    // 96 css): less than a px from the page's is the same viewport.
    const fractional = { width: 2560 / 1.5, height: 96 };
    expect(reanchorPanel(row, 'bottom', { width: 1707, height: 96 }, fractional)).toEqual(row);
    expect(reanchorPanel(row, 'bottom', { width: 1706, height: 96 }, fractional)).toEqual(row);
    // Half a px along the edge rounds away from zero, like Rust's `round`.
    expect(reanchorPanel(row, 'bottom', wide, { width: 2563, height: 96 }).x).toBe(962);
    expect(reanchorPanel(row, 'bottom', wide, { width: 2557, height: 96 }).x).toBe(959);
    expect(reanchorPanel({ ...row, x: 0 }, 'top', wide, { width: 2559, height: 96 }).x).toBe(-1);
    expect(reanchorPanel({ ...row, x: 1 }, 'top', wide, { width: 2558, height: 96 }).x).toBe(0);
  });

  it('setDockPanelRect records its provenance and re-anchors the rect to the window’s viewport', async () => {
    // The window is 300 css tall now; the page measured in a 240 css one
    // (its content size has been applied since): 60 css lower, the same
    // place on screen as a report measured in a 240 css window.
    const b = fast({ viewportHeight: 300, viewportWidth: 1280 });
    const rect = { x: 360, y: 168, width: 561, height: 64 };
    await b.setDockPanelRect(rect, 'bottom', { width: 1280, height: 240 });
    expect(b.calls).toEqual([
      { type: 'setDockPanelRect', rect, edge: 'bottom', viewport: { width: 1280, height: 240 } },
    ]);
    expect(b.dockPanel()).toEqual({ x: 360, y: 1320, width: 561, height: 64 });
    // Measured in a window 280 css shorter along the edge: 140 css further along.
    await b.setDockPanelRect(rect, 'bottom', { width: 1000, height: 240 });
    expect(b.dockPanel()).toEqual({ x: 500, y: 1320, width: 561, height: 64 });

    // On top, a thicker window leaves the rect where it is.
    const top = fast({ position: 'top', viewportHeight: 300, viewportWidth: 1280 });
    await top.setDockPanelRect({ ...rect, y: 8 }, 'top', { width: 1280, height: 240 });
    expect(top.dockPanel()).toEqual({ x: 360, y: 8, width: 561, height: 64 });
  });

  it('a panel measured before a length change it has not measured yet converts to its real place', async () => {
    // The page measured its panel centred in a window 1280 px long; the
    // window is now 1000 px long (another work area) and the page has not
    // measured again when something else makes it report (an auto-hide
    // phase, a file drag). The panel is centred in the shorter window, 140
    // px further towards the start; the report says which length its rect
    // describes (`panelRectReport`), and the conversion moves it there.
    const workArea = { x: 0, y: 0, ...SCREEN };
    const cases = [
      {
        edge: 'bottom',
        measured: { width: 1280, height: 240 },
        now: { width: 1000, height: 240 },
        rest: { x: 360, y: 168, width: 560, height: 64 },
        real: { x: 220, y: 168, width: 560, height: 64 },
      },
      {
        edge: 'top',
        measured: { width: 1280, height: 240 },
        now: { width: 1000, height: 300 },
        rest: { x: 360, y: 8, width: 560, height: 64 },
        real: { x: 220, y: 8, width: 560, height: 64 },
      },
      {
        edge: 'left',
        measured: { width: 240, height: 1280 },
        now: { width: 240, height: 1000 },
        rest: { x: 8, y: 360, width: 64, height: 560 },
        real: { x: 8, y: 220, width: 64, height: 560 },
      },
      {
        edge: 'right',
        measured: { width: 240, height: 1280 },
        now: { width: 300, height: 1000 },
        rest: { x: 168, y: 360, width: 64, height: 560 },
        real: { x: 228, y: 220, width: 64, height: 560 },
      },
    ] as const;
    for (const { edge, measured, now, rest, real } of cases) {
      const b = fast({ position: edge, viewportWidth: now.width, viewportHeight: now.height });
      const report = panelRectReport(edge, rest, measured, now);
      await b.setDockPanelRect(report.rect, edge, report.viewport);
      expect(b.dockPanel(), edge).toEqual(panelOnScreen(real, workArea, now, edge));
      // Sent with the viewport of the moment, the rect would be read as
      // describing the shorter window and stay 140 px off.
      await b.setDockPanelRect(report.rect, edge, now);
      expect(b.dockPanel(), edge).not.toEqual(panelOnScreen(real, workArea, now, edge));
    }
  });

  it('a report measured for another edge than the window’s is recorded and dropped', async () => {
    const b = fast({ viewportHeight: 240, viewportWidth: 1280 });
    const rest = { x: 360, y: 168, width: 561, height: 64 };
    await b.setDockPanelRect(rest, 'bottom', { width: 1280, height: 240 });
    const atRest = b.dockPanel();
    expect(atRest).toEqual({ x: 360, y: 1320, width: 561, height: 64 });

    // The dock moves to the left edge; reports the page measured for the
    // bottom edge are still on their way.
    b.setPosition('left');
    await b.setDockContentSize(1300, 135, 'bottom');
    await b.setDockHitRegion(rest, { ...rest, y: 60, height: 172 }, 'bottom');
    await b.setDockPanelRect({ ...rest, x: 300 }, 'bottom', { width: 1280, height: 240 });
    expect(b.dockPanel()).toEqual(atRest);
    expect(b.calls.map((c) => ('edge' in c ? [c.type, c.edge] : [c.type]))).toEqual([
      ['setDockPanelRect', 'bottom'],
      ['setDockContentSize', 'bottom'],
      ['setDockHitRegion', 'bottom'],
      ['setDockPanelRect', 'bottom'],
    ]);

    // The page's first report for the left edge is taken.
    await b.setDockPanelRect({ x: 8, y: 400, width: 64, height: 561 }, 'left', {
      width: 1280,
      height: 240,
    });
    expect(b.dockPanel()).toEqual({ x: 8, y: 400, width: 64, height: 561 });
  });

  it('a viewport that is not positive and finite rejects the report, like Rust', async () => {
    const b = fast();
    const rect = { x: 0, y: 0, width: 10, height: 10 };
    for (const viewport of [
      { width: 0, height: 240 },
      { width: 1280, height: -1 },
      { width: Number.NaN, height: 240 },
      { width: 1280, height: Number.POSITIVE_INFINITY },
    ]) {
      await expect(b.setDockPanelRect(rect, 'bottom', viewport)).rejects.toMatch(
        /positive, finite width and height/,
      );
    }
    expect(b.dockPanel()).toBeNull();
    // The queue goes on.
    await b.setDockPanelRect(rect, 'bottom', SCREEN);
    expect(b.dockPanel()).toEqual(rect);
  });
});

describe('two monitors', () => {
  const DISPLAY3 = '\\\\.\\DISPLAY3';
  const PAINT = appIdForExe('C:\\Windows\\System32\\mspaint.exe');
  const factsOn = (state: DockState, monitor: string) =>
    state.docks.find((d) => d.monitor === monitor);

  it('has a second monitor to the right with two windows on it', async () => {
    const b = fast({ scenario: 'twoMonitors' });
    const monitors = await b.getMonitors();
    expect(monitors.map((m) => m.id)).toEqual(['mock-primary', DISPLAY3]);
    expect(monitors[1]).toMatchObject({
      bounds: { x: 2560, y: 0, width: 1920, height: 1080 },
      workArea: { x: 2560, y: 0, width: 1920, height: 1032 },
      scaleFactor: 1.5,
      isPrimary: false,
    });
    expect(SECOND_MONITOR_ID).toBe(DISPLAY3);
    const state = await b.getDockState();
    // Everything on one list: the engine filters nothing by monitor.
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
      app(PAINT),
      'sep:3',
      'fixed:recycle-bin',
    ]);
    expect(item(state, app(NOTEPAD)).windows.map((w) => w.monitor)).toEqual([DISPLAY3]);
    expect(item(state, app(PAINT)).windows.map((w) => w.monitor)).toEqual([DISPLAY3]);
    expect(item(state, app(CODE)).windows.map((w) => w.monitor)).toEqual(['mock-primary']);
    // Both new frames stay clear of the second dock's panel area.
    const windows = [...b['engine'].windows.values()].map((t) => t.raw);
    expect(windows).toHaveLength(8);
    expect(overlapped({ x: 2560, y: 1032 - 250, width: 1920, height: 250 }, windows, null)).toBe(
      false,
    );
  });

  it('labels the page\u2019s dock window like the app layer and keys the panels by it', async () => {
    expect(dockLabel(null, 'mock-primary')).toBe('dock');
    expect(dockLabel('mock-primary', 'mock-primary')).toBe('dock');
    expect(dockLabel(DISPLAY3, 'mock-primary')).toBe('dock-DISPLAY3');
    expect(dockLabel('a b/c:d_e', 'x')).toBe('dock-abcd_e');

    const primary = fast({ scenario: 'twoMonitors', viewportHeight: 240 });
    const second = fast({ scenario: 'twoMonitors', viewportHeight: 240, monitor: DISPLAY3 });
    expect(primary.pageLabel).toBe('dock');
    expect(primary.pageMonitor).toBeNull();
    expect(second.pageLabel).toBe('dock-DISPLAY3');
    expect(second.pageMonitor).toBe(DISPLAY3);

    // Each page's panel lands on its own monitor, the second at 150 %.
    const rect = { x: 360, y: 168, width: 561, height: 64 };
    await primary.setDockPanelRect(rect, 'bottom', { width: 2560, height: 240 });
    await second.setDockPanelRect(rect, 'bottom', { width: 1920, height: 240 });
    expect(primary.dockPanel()).toEqual({ x: 360, y: 1320, width: 561, height: 64 });
    expect(second.dockPanel()).toEqual({ x: 3100, y: 924, width: 842, height: 96 });
    // One backend serves one page: each knows only its own dock.
    expect((await primary.getDockState()).docks).toEqual([
      { monitor: 'mock-primary', fullscreen: false, overlapped: false },
    ]);
    expect((await second.getDockState()).docks).toEqual([
      { monitor: DISPLAY3, fullscreen: false, overlapped: false },
    ]);
    expect(second.dockPanel('dock')).toBeNull();
  });

  it('fullscreen and overlapped are per dock', async () => {
    const second = fast({ scenario: 'twoMonitors', viewportHeight: 240, monitor: DISPLAY3 });
    const states = recordStates(second);
    await second.setDockPanelRect({ x: 360, y: 168, width: 561, height: 64 }, 'bottom', {
      width: 1920,
      height: 240,
    });
    states.length = 0;

    // A video goes fullscreen on the primary: not this dock's business.
    second.setFullscreen(true, 'mock-primary');
    expect(states).toHaveLength(0);
    second.setFullscreen(true);
    expect(states).toHaveLength(0);
    // On this monitor: hidden.
    second.setFullscreen(true, DISPLAY3);
    expect(states).toHaveLength(1);
    expect(factsOn(states[0]!, DISPLAY3)?.fullscreen).toBe(true);
    second.setFullscreen(false);
    expect(factsOn(states.at(-1)!, DISPLAY3)?.fullscreen).toBe(false);

    // Paint dragged under this dock's panel: overlapped; the same frame on
    // the primary page's backend overlaps nothing there.
    const under = { x: 3000, y: 800, width: 900, height: 300 };
    second.moveWindow(8, under);
    expect(factsOn(states.at(-1)!, DISPLAY3)?.overlapped).toBe(true);
    const primary = fast({ scenario: 'twoMonitors', viewportHeight: 240 });
    await primary.setDockPanelRect({ x: 360, y: 168, width: 561, height: 64 }, 'bottom', {
      width: 2560,
      height: 240,
    });
    const primaryStates = recordStates(primary);
    primary.moveWindow(8, under);
    expect(primaryStates).toHaveLength(0);
    expect(factsOn(await primary.getDockState(), 'mock-primary')?.overlapped).toBe(false);
  });

  /** A Notepad window on `monitor`, as the platform reports it. */
  const notepadOn = (id: number, monitor: string): RawWindow => ({
    id,
    title: `Notepad ${id}`,
    className: 'Notepad',
    processId: 1000 + id,
    exePath: 'C:\\Windows\\notepad.exe',
    appUserModelId: null,
    isVisible: true,
    isMinimized: false,
    isCloaked: false,
    onCurrentDesktop: null,
    monitor,
    rect: null,
    iconKey: null,
  });

  /**
   * A page's backend with its panel registered, `monitors = all`, the
   * setting as given, and Notepad with window 9 on the primary, seen
   * first, and window 7 on the second monitor.
   */
  async function notepadOnBoth(
    monitor: string | null,
    showOnlyCurrentMonitor: boolean,
  ): Promise<MockBackend> {
    const b = fast({ scenario: 'twoMonitors', viewportHeight: 240, viewportWidth: 1000, monitor });
    await b.setDockPanelRect({ x: 360, y: 168, width: 561, height: 64 }, 'bottom', {
      width: 1000,
      height: 240,
    });
    b.pushWindowRemoved(7);
    b.pushWindow(notepadOn(9, 'mock-primary'));
    b.pushWindow(notepadOn(7, DISPLAY3));
    const settings = await b.getSettings();
    settings.monitors = 'all';
    settings.behavior.showOnlyCurrentMonitor = showOnlyCurrentMonitor;
    await b.updateSettings(settings);
    b.calls.length = 0;
    return b;
  }

  it('a click acts on the windows the page\u2019s own dock lists (DockEngine::windows_on_dock)', async () => {
    const second = await notepadOnBoth(DISPLAY3, true);
    // Nothing of Notepad's is active: this dock's window, not the first seen.
    await second.primaryClick(app(NOTEPAD));
    // Now it is active and alone here: minimised, not cycled to window 9.
    await second.primaryClick(app(NOTEPAD));
    expect(second.calls).toEqual([
      { type: 'windowAction', windowId: 7, action: 'activate' },
      { type: 'windowAction', windowId: 7, action: 'minimize' },
    ]);

    // The primary's page acts on window 9 alone; once that closes, the
    // pinned app has no window there and is launched.
    const primary = await notepadOnBoth(null, true);
    await primary.primaryClick(app(NOTEPAD));
    primary.pushWindowRemoved(9);
    await primary.primaryClick(app(NOTEPAD));
    expect(primary.calls).toEqual([
      { type: 'windowAction', windowId: 9, action: 'activate' },
      {
        type: 'launch',
        target: { kind: 'exe', path: 'C:\\Windows\\notepad.exe', args: [], workingDir: null },
      },
    ]);
  });

  it('every window counts with the setting off, one dock, or the dock not yet placed', async () => {
    // The setting off: the first-seen window, then a cycle to the other.
    const off = await notepadOnBoth(DISPLAY3, false);
    await off.primaryClick(app(NOTEPAD));
    await off.primaryClick(app(NOTEPAD));
    expect(off.calls.map((c) => c.type === 'windowAction' && c.windowId)).toEqual([9, 7]);

    // On, with `monitors = primary`: the same.
    const one = await notepadOnBoth(DISPLAY3, true);
    const settings = await one.getSettings();
    settings.monitors = 'primary';
    await one.updateSettings(settings);
    await one.primaryClick(app(NOTEPAD));
    expect(one.calls.at(-1)).toEqual({ type: 'windowAction', windowId: 9, action: 'activate' });

    // On, but the page never reported its panel: its monitor is unknown.
    const unplaced = fast({ scenario: 'twoMonitors', monitor: DISPLAY3 });
    unplaced.pushWindowRemoved(7);
    unplaced.pushWindow(notepadOn(9, 'mock-primary'));
    unplaced.pushWindow(notepadOn(7, DISPLAY3));
    const all = await unplaced.getSettings();
    all.monitors = 'all';
    all.behavior.showOnlyCurrentMonitor = true;
    await unplaced.updateSettings(all);
    await unplaced.primaryClick(app(NOTEPAD));
    expect(unplaced.calls.at(-1)).toEqual({
      type: 'windowAction',
      windowId: 9,
      action: 'activate',
    });
  });
});

describe('attention per window (mirror of WindowInfo::needs_attention)', () => {
  const flags = (state: DockState, key: string): [boolean, boolean[]] => {
    const found = item(state, key);
    return [found.needsAttention, found.windows.map((w) => w.needsAttention)];
  };

  it('flags the flashing window, the item while any window is flagged, and clears on activation', async () => {
    // Firefox has windows 2 (in front) and 3 in the demo.
    const b = fast();
    const states = recordStates(b);
    expect(flags(await b.getDockState(), app(FIREFOX))).toEqual([false, [false, false]]);

    b.pushAttention(3, true);
    expect(flags(states.at(-1)!, app(FIREFOX))).toEqual([true, [false, true]]);

    b.pushForeground(3);
    expect(flags(states.at(-1)!, app(FIREFOX))).toEqual([false, [false, false]]);

    // Window 2 is behind now and flashes; the item says which one.
    b.pushAttention(2, true);
    expect(flags(states.at(-1)!, app(FIREFOX))).toEqual([true, [true, false]]);
    b.pushAttention(2, false);
    expect(flags(states.at(-1)!, app(FIREFOX))).toEqual([false, [false, false]]);
    // Other items never carry the flag of another app's window.
    expect(states.at(-1)!.items.every((i) => i.windows.every((w) => !w.needsAttention))).toBe(true);
  });

  it('never flags the window in front, like the engine', async () => {
    // Window 2 (Firefox) is in front in the demo.
    const b = fast();
    const states = recordStates(b);
    await b.getDockState();
    const before = states.length;

    b.pushAttention(2, true);
    expect(states.length).toBe(before);
    expect(flags(await b.getDockState(), app(FIREFOX))).toEqual([false, [false, false]]);

    // A window behind it still asks.
    b.pushAttention(3, true);
    expect(flags(states.at(-1)!, app(FIREFOX))).toEqual([true, [false, true]]);
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
