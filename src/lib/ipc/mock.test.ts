import { afterEach, describe, expect, it, vi } from 'vitest';

import type { DockState } from '$lib/ipc/generated/DockState';
import type { Settings } from '$lib/ipc/generated/Settings';

import type { BouncePayload, FileDragEvent } from './backend';
import {
  appIdForExe,
  colorFor,
  defaultSettings,
  fileStem,
  MockBackend,
  nothingToRecycle,
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
