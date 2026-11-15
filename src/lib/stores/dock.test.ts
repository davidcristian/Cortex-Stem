import { describe, expect, it } from 'vitest';

import type {
  Backend,
  BackendEvent,
  BackendEvents,
  EventHandler,
  Unsubscribe,
} from '$lib/ipc/backend';
import type { DockState } from '$lib/ipc/generated/DockState';
import type { MonitorInfo } from '$lib/ipc/generated/MonitorInfo';
import type { Settings } from '$lib/ipc/generated/Settings';
import type { ThemeInfo } from '$lib/ipc/generated/ThemeInfo';
import type { UserCss } from '$lib/ipc/generated/UserCss';
import { defaultSettings } from '$lib/ipc/mock';

import { acceptsRevision, DockStore } from './dock.svelte';
import { iconId } from './icon-cache';
import { ReactiveIconCache } from './reactive-icon-cache';

const snapshot = (revision: number): DockState => ({
  revision,
  items: [],
  foreground: null,
  docks: [],
  power: { hasBattery: false, percent: null, charging: false, onAc: false, saver: false },
});

const MONITOR: MonitorInfo = {
  id: 'mock-primary',
  name: 'Mock Display',
  bounds: { x: 0, y: 0, width: 2560, height: 1440 },
  workArea: { x: 0, y: 0, width: 2560, height: 1392 },
  scaleFactor: 1,
  isPrimary: true,
};

const THEME: ThemeInfo = { accent: '#0078D4', dark: true, highContrast: false, transparency: true };

/** Scripted backend: records the order of calls and lets tests emit events. */
class FakeBackend implements Backend {
  readonly log: string[] = [];
  readonly iconRequests: Array<[string, number]> = [];
  private readonly handlers = new Map<BackendEvent, Set<(payload: unknown) => void>>();
  initialState = snapshot(0);
  settings: Settings = defaultSettings();

  emit<E extends BackendEvent>(event: E, payload: BackendEvents[E]): void {
    for (const h of this.handlers.get(event) ?? []) {
      h(payload);
    }
  }

  on<E extends BackendEvent>(event: E, handler: EventHandler<E>): Unsubscribe {
    this.log.push(`on:${event}`);
    let set = this.handlers.get(event);
    if (!set) {
      set = new Set();
      this.handlers.set(event, set);
    }
    const h = handler as (payload: unknown) => void;
    set.add(h);
    return () => {
      set.delete(h);
    };
  }

  async getDockState(): Promise<DockState> {
    this.log.push('get_dock_state');
    return this.initialState;
  }

  async getSettings(): Promise<Settings> {
    this.log.push('get_settings');
    return this.settings;
  }

  async getTheme(): Promise<ThemeInfo> {
    this.log.push('get_theme');
    return THEME;
  }

  monitors: MonitorInfo[] = [];

  async getMonitors(): Promise<MonitorInfo[]> {
    this.log.push('get_monitors');
    return this.monitors;
  }

  async updateSettings(settings: Settings): Promise<Settings> {
    return settings;
  }

  async primaryClick(): Promise<void> {}
  async windowAction(): Promise<void> {}
  async launchApp(): Promise<void> {}
  async pinApp(): Promise<void> {}
  async pinTarget(): Promise<string> {
    return '';
  }
  async unpinApp(): Promise<void> {}
  async hideApp(): Promise<void> {}
  async unhideApp(): Promise<void> {}
  async reorderPinned(): Promise<void> {}

  async getIcon(iconKey: string, size: number): Promise<string | null> {
    this.iconRequests.push([iconKey, size]);
    return iconKey === 'missing' ? null : `data:${iconKey}@${size}`;
  }

  async setDockContentSize(): Promise<void> {}
  async setDockHitRegion(): Promise<void> {}
  async setDockPanelRect(): Promise<void> {}
  async setDockAutoHidden(): Promise<void> {}
  async shellAction(): Promise<void> {}
  async deleteToRecycleBin(): Promise<void> {}
  async openSettings(): Promise<void> {}
  async resetSettings(): Promise<Settings> {
    return this.settings;
  }
  async quit(): Promise<void> {}
  async getAppInfo() {
    return { version: '0.0.0', settingsPath: '', logPath: '', platform: 'fake' };
  }
  async openAppFolder(): Promise<void> {}
  async showItemMenu(): Promise<void> {}
  async showDockMenu(): Promise<void> {}
  async releaseDockFocus(): Promise<void> {}
  async getUserCss(): Promise<UserCss> {
    return { path: 'dock.css', css: null };
  }
  async reloadUserCss(): Promise<UserCss> {
    return { path: 'dock.css', css: null };
  }
  async openUserCss(): Promise<void> {}
  onFileDrag(): Unsubscribe {
    return () => {};
  }
}

const flush = (): Promise<void> => new Promise((r) => setTimeout(r, 0));

describe('acceptsRevision', () => {
  it('accepts anything when nothing is shown yet', () => {
    expect(acceptsRevision(null, snapshot(0))).toBe(true);
  });

  it('accepts equal or newer revisions and rejects older ones', () => {
    expect(acceptsRevision(snapshot(5), snapshot(5))).toBe(true);
    expect(acceptsRevision(snapshot(5), snapshot(6))).toBe(true);
    expect(acceptsRevision(snapshot(5), snapshot(4))).toBe(false);
  });
});

describe('DockStore', () => {
  it('subscribes to every event before fetching the initial snapshots', async () => {
    const backend = new FakeBackend();
    const store = new DockStore(backend);
    await store.start();
    expect(backend.log).toEqual([
      'on:dock://state',
      'on:dock://settings',
      'on:dock://theme',
      'on:dock://monitors',
      'on:dock://item-bounce',
      'get_dock_state',
      'get_settings',
      'get_theme',
      'get_monitors',
    ]);
    expect(store.state).toEqual(snapshot(0));
    expect(store.settings).toEqual(defaultSettings());
    expect(store.theme).toEqual(THEME);
    expect(store.monitors).toEqual([]);
    // Starting twice does not subscribe twice.
    await store.start();
    expect(backend.log.filter((l) => l === 'on:dock://state')).toHaveLength(1);
  });

  it('fetches the monitors at start and lets a monitors event that came first stand', async () => {
    const backend = new FakeBackend();
    backend.monitors = [MONITOR];
    const store = new DockStore(backend);
    await store.start();
    expect(store.monitors).toEqual([MONITOR]);

    // The event arrives while the fetch is in flight: it is the fresher list.
    const second = { ...MONITOR, id: '\\\\.\\DISPLAY3', isPrimary: false };
    const racing = new FakeBackend();
    racing.monitors = [MONITOR];
    const late = new DockStore(racing);
    const started = late.start();
    racing.emit('dock://monitors', [MONITOR, second]);
    await started;
    expect(late.monitors).toEqual([MONITOR, second]);
    // Later events replace the list as before.
    racing.emit('dock://monitors', [MONITOR]);
    expect(late.monitors).toEqual([MONITOR]);
  });

  it('drops stale snapshots, accepts equal and newer ones', async () => {
    const backend = new FakeBackend();
    const store = new DockStore(backend);
    await store.start();

    backend.emit('dock://state', snapshot(3));
    expect(store.state?.revision).toBe(3);
    backend.emit('dock://state', snapshot(2));
    expect(store.state?.revision).toBe(3);
    expect(store.droppedSnapshots).toBe(1);
    backend.emit('dock://state', snapshot(3));
    expect(store.state?.revision).toBe(3);
    expect(store.droppedSnapshots).toBe(1);
    backend.emit('dock://state', snapshot(7));
    expect(store.state?.revision).toBe(7);
  });

  it('ignores an initial fetch that resolves after a newer event already arrived', async () => {
    const backend = new FakeBackend();
    backend.initialState = snapshot(1);
    const store = new DockStore(backend);
    const started = store.start();
    // The event races the fetch and wins.
    backend.emit('dock://state', snapshot(4));
    await started;
    expect(store.state?.revision).toBe(4);
    expect(store.droppedSnapshots).toBe(1);
  });

  it('stops delivering after stop()', async () => {
    const backend = new FakeBackend();
    const store = new DockStore(backend);
    await store.start();
    store.stop();
    backend.emit('dock://state', snapshot(9));
    expect(store.state?.revision).toBe(0);
  });

  it('re-triggers a bounce on the same key through the serial', async () => {
    const backend = new FakeBackend();
    const store = new DockStore(backend);
    await store.start();
    backend.emit('dock://item-bounce', { itemKey: 'app:x' });
    backend.emit('dock://item-bounce', { itemKey: 'app:x' });
    expect(store.bounceKey).toBe('app:x');
    expect(store.bounceSerial).toBe(2);
  });

  it('keeps its icons in the shared ReactiveIconCache over its backend', async () => {
    // The cache's own rules (once per key and size, never shell:, null
    // cached) are tested in reactive-icon-cache.test.ts.
    const backend = new FakeBackend();
    const store = new DockStore(backend);
    expect(store.iconCache).toBeInstanceOf(ReactiveIconCache);
    expect(store.icons).toBe(store.iconCache.icons);

    store.ensureIcon('mock:a', 96);
    store.ensureIcon('mock:a', 96);
    store.ensureIcon('shell:start', 96);
    await flush();
    expect(backend.iconRequests).toEqual([['mock:a', 96]]);
    expect(store.icon('mock:a', 96)).toBe('data:mock:a@96');
    expect(store.iconCache.icon('mock:a', 96)).toBe('data:mock:a@96');
    expect(store.icons.get(iconId('mock:a', 96))).toBe('data:mock:a@96');

    store.clearIcons();
    expect(store.icon('mock:a', 96)).toBeUndefined();
    expect(store.icons.size).toBe(0);
    store.ensureIcon('mock:a', 96);
    await flush();
    expect(backend.iconRequests).toHaveLength(2);
  });
});
