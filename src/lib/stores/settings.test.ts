import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type {
  Backend,
  BackendEvent,
  BackendEvents,
  EventHandler,
  Unsubscribe,
} from '$lib/ipc/backend';
import type { AppFolder } from '$lib/ipc/generated/AppFolder';
import type { AppId } from '$lib/ipc/generated/AppId';
import type { AppInfo } from '$lib/ipc/generated/AppInfo';
import type { DockItem } from '$lib/ipc/generated/DockItem';
import type { DockState } from '$lib/ipc/generated/DockState';
import type { Settings } from '$lib/ipc/generated/Settings';
import type { ThemeInfo } from '$lib/ipc/generated/ThemeInfo';
import type { UserCss } from '$lib/ipc/generated/UserCss';
import { defaultSettings, demoAppInfo, validateSettings } from '$lib/ipc/mock';

import { iconId } from './icon-cache';
import { ReactiveIconCache } from './reactive-icon-cache';
import { SettingsStore } from './settings.svelte';

const THEME: ThemeInfo = { accent: '#0078D4', dark: true, highContrast: false, transparency: true };

const pin = (id: string) => ({ id, label: null, launch: { kind: 'uri' as const, uri: id } });

/** The dock's item for a pinned app once the engine has resolved it. */
const appItem = (appId: string, title: string, iconKey: string | null): DockItem => ({
  key: `app:${appId}`,
  kind: 'app',
  appId,
  title,
  iconKey,
  pinned: true,
  windows: [],
  isActive: false,
  needsAttention: false,
  badge: null,
});

const snapshot = (revision: number, items: DockItem[] = []): DockState => ({
  revision,
  items,
  foreground: null,
  docks: [],
  power: { hasBattery: false, percent: null, charging: false, onAc: false, saver: false },
  clockFormat: { locale: null, hour12: null },
});

/** Scripted backend: a settings document, a call log, and manual control over latency. */
class FakeBackend implements Backend {
  readonly log: string[] = [];
  stored: Settings = { ...defaultSettings(), pinned: [pin('a'), pin('b'), pin('c')] };
  /** What `get_dock_state` answers (`null` makes it fail). */
  state: DockState | null = snapshot(0);
  /** `get_icon` answers by key; a key that is not listed has no icon (`null`). */
  readonly iconUrls = new Map<string, string>();
  /** Every `get_icon` so far, as `key@size`. */
  readonly iconRequests: string[] = [];
  /** Pending `update_settings` resolvers, oldest first (resolved by the test). */
  readonly pendingUpdates: Array<(settings: Settings) => void> = [];
  failNext: string | null = null;
  /** What `get_app_info` answers (`null` makes it fail). */
  appInfo: AppInfo | null = demoAppInfo();
  private readonly handlers = new Map<BackendEvent, Set<(payload: unknown) => void>>();

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

  private check(): void {
    if (this.failNext !== null) {
      const message = this.failNext;
      this.failNext = null;
      throw message;
    }
  }

  async getSettings(): Promise<Settings> {
    this.log.push('get_settings');
    this.check();
    return structuredClone(this.stored);
  }

  updateSettings(settings: Settings): Promise<Settings> {
    this.log.push(`update_settings:${settings.iconSize}`);
    return new Promise((resolve, reject) => {
      try {
        this.check();
      } catch (err) {
        reject(err);
        return;
      }
      this.pendingUpdates.push((s) => {
        this.stored = validateSettings(s);
        resolve(structuredClone(this.stored));
      });
    });
  }

  /** Settle the oldest pending update the way Rust would: validated and stored. */
  settleUpdate(): void {
    const resolve = this.pendingUpdates.shift();
    if (!resolve) {
      throw new Error('no update pending');
    }
    // The resolver captured the document it was called with.
    resolve(this.lastSent!);
  }

  private get lastSent(): Settings | undefined {
    const entry = this.log.filter((l) => l.startsWith('update_settings:')).at(-1);
    if (!entry) {
      return undefined;
    }
    const iconSize = Number(entry.slice('update_settings:'.length));
    return { ...structuredClone(this.stored), iconSize };
  }

  async resetSettings(): Promise<Settings> {
    this.log.push('reset_settings');
    this.check();
    this.stored = defaultSettings();
    return structuredClone(this.stored);
  }

  async reorderPinned(order: AppId[]): Promise<void> {
    this.log.push(`reorder_pinned:${order.join(',')}`);
    this.check();
    const rest = this.stored.pinned.filter((p) => !order.includes(p.id));
    this.stored.pinned = order
      .map((id) => this.stored.pinned.find((p) => p.id === id)!)
      .concat(rest);
  }

  async unpinApp(appId: AppId): Promise<void> {
    this.log.push(`unpin_app:${appId}`);
    this.check();
    this.stored.pinned = this.stored.pinned.filter((p) => p.id !== appId);
  }

  async hideApp(appId: AppId): Promise<void> {
    this.log.push(`hide_app:${appId}`);
    this.check();
    this.stored.hiddenApps.push({ id: appId, name: appId.toUpperCase() });
  }

  async unhideApp(appId: AppId): Promise<void> {
    this.log.push(`unhide_app:${appId}`);
    this.check();
    this.stored.hiddenApps = this.stored.hiddenApps.filter((h) => h.id !== appId);
  }

  async getTheme(): Promise<ThemeInfo> {
    this.log.push('get_theme');
    return THEME;
  }

  async getDockState(): Promise<DockState> {
    this.log.push('get_dock_state');
    if (this.state === null) {
      throw 'engine not running';
    }
    return this.state;
  }
  async getAppInfo(): Promise<AppInfo> {
    this.log.push('get_app_info');
    if (this.appInfo === null) {
      throw 'no app info';
    }
    return { ...this.appInfo };
  }
  async openAppFolder(folder: AppFolder): Promise<void> {
    this.log.push(`open_app_folder:${folder}`);
    this.check();
  }
  async getIcon(iconKey: string, size: number): Promise<string | null> {
    this.iconRequests.push(iconId(iconKey, size));
    return this.iconUrls.get(iconKey) ?? null;
  }
  async getMonitors() {
    return [];
  }
  async primaryClick(): Promise<void> {}
  async scrollItem(): Promise<void> {}
  async windowAction(): Promise<void> {}
  async launchApp(): Promise<void> {}
  async pinApp(): Promise<void> {}
  async pinTarget(): Promise<string> {
    return '';
  }
  async setDockContentSize(): Promise<void> {}
  async setDockHitRegion(): Promise<void> {}
  async setDockPanelRect(): Promise<void> {}
  async setDockAutoHidden(): Promise<void> {}
  async shellAction(): Promise<void> {}
  async deleteToRecycleBin(): Promise<void> {}
  async openSettings(): Promise<void> {}
  async quit(): Promise<void> {}
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

const tick = async (): Promise<void> => {
  await vi.advanceTimersByTimeAsync(0);
};

async function started(): Promise<{ backend: FakeBackend; store: SettingsStore }> {
  const backend = new FakeBackend();
  const store = new SettingsStore(backend, 150);
  await store.start();
  return { backend, store };
}

beforeEach(() => {
  vi.useFakeTimers();
  vi.spyOn(console, 'error').mockImplementation(() => {});
});

afterEach(() => {
  vi.useRealTimers();
});

describe('SettingsStore', () => {
  it('subscribes before fetching and reports "saved" once loaded', async () => {
    const { backend, store } = await started();
    expect(backend.log).toEqual([
      'on:dock://settings',
      'on:dock://theme',
      'on:dock://state',
      'get_settings',
      'get_theme',
      'get_dock_state',
      'get_app_info',
    ]);
    expect(store.settings?.pinned.map((p) => p.id)).toEqual(['a', 'b', 'c']);
    expect(store.theme).toEqual(THEME);
    expect(store.dockState).toEqual(snapshot(0));
    expect(store.appInfo).toEqual(demoAppInfo());
    expect(store.aboutError).toBeNull();
    expect(store.status).toBe('saved');
  });

  it('a failed get_app_info leaves the page usable and the About section with a notice', async () => {
    const backend = new FakeBackend();
    backend.appInfo = null;
    const store = new SettingsStore(backend, 150);
    await store.start();
    expect(store.status).toBe('saved');
    expect(store.settings?.pinned).toHaveLength(3);
    expect(store.appInfo).toBeNull();
    expect(store.aboutError).toBe('no app info');
    expect(console.error).toHaveBeenCalledWith('get_app_info failed:', 'no app info');
  });

  it('openAppFolder asks Rust, touches no save state, and reports a failure in the About section', async () => {
    const { backend, store } = await started();
    store.edit((s) => {
      s.iconSize = 50;
    });
    expect(store.status).toBe('saving');

    // Not a settings command: it does not wait for the pending edit.
    await store.openAppFolder('logs');
    expect(backend.log.at(-1)).toBe('open_app_folder:logs');
    expect(backend.pendingUpdates).toHaveLength(0);
    expect(store.status).toBe('saving');
    expect(store.aboutError).toBeNull();

    backend.failNext = 'the log folder is unknown on this host';
    await store.openAppFolder('settings');
    expect(backend.log.at(-1)).toBe('open_app_folder:settings');
    expect(store.aboutError).toBe('the log folder is unknown on this host');
    expect(store.status).toBe('saving');
    expect(store.error).toBeNull();

    // The next attempt clears the notice.
    await store.openAppFolder('settings');
    expect(store.aboutError).toBeNull();
  });

  it("keeps the dock state current and finds a pin's item in it", async () => {
    const { backend, store } = await started();
    expect(store.dockItem('a')).toBeUndefined();

    backend.emit('dock://state', snapshot(2, [appItem('a', 'App A', 'exe:a')]));
    expect(store.dockItem('a')).toMatchObject({ title: 'App A', iconKey: 'exe:a' });
    expect(store.dockItem('b')).toBeUndefined();

    // A snapshot older than the one shown is dropped.
    expect(store.applyState(snapshot(1, []))).toBe(false);
    expect(store.dockItem('a')?.title).toBe('App A');
    // The same revision again (the startup fetch after an event) is fine.
    expect(store.applyState(snapshot(2, [appItem('a', 'Renamed', null)]))).toBe(true);
    expect(store.dockItem('a')).toMatchObject({ title: 'Renamed', iconKey: null });
  });

  it('a failed get_dock_state leaves the page usable without names and icons', async () => {
    const backend = new FakeBackend();
    backend.state = null;
    const store = new SettingsStore(backend, 150);
    await store.start();
    expect(store.status).toBe('saved');
    expect(store.settings?.pinned).toHaveLength(3);
    expect(store.dockState).toBeNull();
    expect(store.dockItem('a')).toBeUndefined();
    expect(console.error).toHaveBeenCalledWith('get_dock_state failed:', 'engine not running');
  });

  it('keeps its icons in the shared ReactiveIconCache over its backend', async () => {
    // The cache's own rules (once per key and size, never shell:, null
    // cached) are tested in reactive-icon-cache.test.ts.
    const { backend, store } = await started();
    expect(store.iconCache).toBeInstanceOf(ReactiveIconCache);
    expect(store.icons).toBe(store.iconCache.icons);
    backend.iconUrls.set('exe:a', 'data:image/png;base64,AAAA');
    expect(store.icon('exe:a', 48)).toBeUndefined();

    store.ensureIcon('exe:a', 48);
    store.ensureIcon('exe:a', 48);
    store.ensureIcon('exe:b', 48);
    store.ensureIcon('shell:start', 48);
    await tick();
    await tick();
    expect(backend.iconRequests).toEqual(['exe:a@48', 'exe:b@48']);
    expect(store.icon('exe:a', 48)).toBe('data:image/png;base64,AAAA');
    expect(store.iconCache.icon('exe:a', 48)).toBe('data:image/png;base64,AAAA');
    // The backend has no icon for this key: `null`, not a broken image.
    expect(store.icon('exe:b', 48)).toBeNull();
    expect(store.icons.get(iconId('exe:b', 48))).toBeNull();
  });

  it('shows edits at once, coalesces them, and saves 150 ms after the last one', async () => {
    const { backend, store } = await started();
    store.edit((s) => {
      s.iconSize = 60;
    });
    expect(store.settings?.iconSize).toBe(60);
    expect(store.status).toBe('saving');
    await vi.advanceTimersByTimeAsync(100);
    store.edit((s) => {
      s.iconSize = 64;
    });
    await vi.advanceTimersByTimeAsync(149);
    expect(backend.log.filter((l) => l.startsWith('update_settings'))).toEqual([]);
    await vi.advanceTimersByTimeAsync(1);
    expect(backend.log.filter((l) => l.startsWith('update_settings'))).toEqual([
      'update_settings:64',
    ]);
    backend.settleUpdate();
    await tick();
    expect(store.status).toBe('saved');
    expect(store.settings?.iconSize).toBe(64);
  });

  it('re-renders from the value Rust kept (clamped)', async () => {
    const { backend, store } = await started();
    store.edit((s) => {
      s.iconSize = 999;
    });
    expect(store.settings?.iconSize).toBe(999);
    await vi.advanceTimersByTimeAsync(150);
    backend.settleUpdate();
    await tick();
    expect(store.settings?.iconSize).toBe(128);
    expect(store.status).toBe('saved');
  });

  it('sends an edit made during a save afterwards, and lets its result win', async () => {
    const { backend, store } = await started();
    store.edit((s) => {
      s.iconSize = 60;
    });
    await vi.advanceTimersByTimeAsync(150);
    expect(backend.pendingUpdates).toHaveLength(1);
    store.edit((s) => {
      s.iconSize = 70;
    });
    await vi.advanceTimersByTimeAsync(150);
    // The second save waits for the first.
    expect(backend.pendingUpdates).toHaveLength(1);
    backend.settleUpdate();
    await tick();
    // The first result did not clobber the newer draft.
    expect(store.settings?.iconSize).toBe(70);
    expect(store.status).toBe('saving');
    expect(backend.pendingUpdates).toHaveLength(1);
    backend.settleUpdate();
    await tick();
    expect(store.settings?.iconSize).toBe(70);
    expect(store.status).toBe('saved');
  });

  it('applies dock://settings events unless an edit of its own is pending', async () => {
    const { backend, store } = await started();
    const pushed = { ...defaultSettings(), iconSize: 32 };
    backend.emit('dock://settings', pushed);
    expect(store.settings?.iconSize).toBe(32);

    store.edit((s) => {
      s.iconSize = 80;
    });
    backend.emit('dock://settings', { ...defaultSettings(), iconSize: 40 });
    expect(store.settings?.iconSize).toBe(80);
    await vi.advanceTimersByTimeAsync(150);
    backend.settleUpdate();
    await tick();
    expect(store.settings?.iconSize).toBe(80);
  });

  it('fetches the document again after a save during which another change was announced', async () => {
    const { backend, store } = await started();
    store.edit((s) => {
      s.iconSize = 60;
    });
    await vi.advanceTimersByTimeAsync(150);
    expect(backend.pendingUpdates).toHaveLength(1);
    // While the save is on its way the dock pins an app and the tray flips
    // click-through: Rust announces both. The page, busy, shows the pin at
    // once (the lists only Rust's commands change) but not the other
    // field, which its own pending save would otherwise appear to undo.
    const clickThrough = !backend.stored.behavior.clickThrough;
    backend.stored.pinned.push(pin('d'));
    backend.stored.behavior.clickThrough = clickThrough;
    backend.emit('dock://settings', structuredClone(backend.stored));
    expect(store.settings?.pinned.map((p) => p.id)).toEqual(['a', 'b', 'c', 'd']);
    expect(store.settings?.behavior.clickThrough).toBe(!clickThrough);
    backend.settleUpdate();
    await tick();
    await tick();
    expect(backend.log.at(-1)).toBe('get_settings');
    expect(store.settings?.pinned.map((p) => p.id)).toEqual(['a', 'b', 'c', 'd']);
    expect(store.settings?.behavior.clickThrough).toBe(clickThrough);
    expect(store.settings?.iconSize).toBe(60);
    expect(store.status).toBe('saved');
  });

  it('does not fetch again for the echo of its own save', async () => {
    const { backend, store } = await started();
    store.edit((s) => {
      s.iconSize = 60;
    });
    await vi.advanceTimersByTimeAsync(150);
    // Rust's event for this very save arrives before the invoke resolves.
    backend.emit('dock://settings', validateSettings({ ...backend.stored, iconSize: 60 }));
    backend.settleUpdate();
    await tick();
    await tick();
    expect(backend.log.filter((l) => l === 'get_settings')).toHaveLength(1);
    expect(store.settings?.iconSize).toBe(60);
  });

  it('reports a failed save and retries with the next edit', async () => {
    const { backend, store } = await started();
    backend.failNext = 'disk full';
    store.edit((s) => {
      s.iconSize = 60;
    });
    await vi.advanceTimersByTimeAsync(150);
    await tick();
    expect(store.status).toBe('error');
    expect(store.error).toBe('disk full');
    expect(store.settings?.iconSize).toBe(60);

    store.edit((s) => {
      s.iconSize = 61;
    });
    expect(store.status).toBe('saving');
    expect(store.error).toBeNull();
    await vi.advanceTimersByTimeAsync(150);
    backend.settleUpdate();
    await tick();
    expect(store.status).toBe('saved');
  });

  it('pin commands flush pending edits first, then refetch', async () => {
    const { backend, store } = await started();
    store.edit((s) => {
      s.iconSize = 60;
    });
    const done = store.reorderPinned(['c', 'a', 'b']);
    await tick();
    // The pending edit went out immediately; the command waits for it.
    expect(backend.log.filter((l) => l.startsWith('update_settings'))).toEqual([
      'update_settings:60',
    ]);
    expect(backend.log.some((l) => l.startsWith('reorder_pinned'))).toBe(false);
    backend.settleUpdate();
    await done;
    expect(backend.log.slice(-2)).toEqual(['reorder_pinned:c,a,b', 'get_settings']);
    expect(store.settings?.pinned.map((p) => p.id)).toEqual(['c', 'a', 'b']);
    expect(store.settings?.iconSize).toBe(60);
    expect(store.status).toBe('saved');
  });

  it('a draft made during a pin command takes over the fresh pins', async () => {
    const { backend, store } = await started();
    const done = store.unpinApp('b');
    store.edit((s) => {
      s.iconSize = 90;
    });
    await done;
    expect(store.settings?.pinned.map((p) => p.id)).toEqual(['a', 'c']);
    expect(store.settings?.iconSize).toBe(90);
    expect(store.status).toBe('saving');
    await vi.advanceTimersByTimeAsync(150);
    // The save carries the fresh pins, not the ones from before the unpin.
    backend.settleUpdate();
    await tick();
    expect(backend.stored.pinned.map((p) => p.id)).toEqual(['a', 'c']);
    expect(store.status).toBe('saved');
  });

  it('reset drops pending edits and shows the defaults', async () => {
    const { backend, store } = await started();
    store.edit((s) => {
      s.iconSize = 60;
    });
    const done = store.reset();
    await tick();
    // The pending edit is flushed first (so the file never ends up with it after the reset).
    expect(backend.log.filter((l) => l.startsWith('update_settings'))).toEqual([
      'update_settings:60',
    ]);
    backend.settleUpdate();
    await done;
    expect(backend.log.at(-1)).toBe('reset_settings');
    expect(store.settings).toEqual(defaultSettings());
    expect(store.status).toBe('saved');
  });

  it('Show for a hidden app flushes pending edits, runs unhide_app and refetches', async () => {
    const backend = new FakeBackend();
    backend.stored.hiddenApps = [
      { id: 'x', name: 'X' },
      { id: 'y', name: 'Y' },
    ];
    const store = new SettingsStore(backend, 150);
    await store.start();
    expect(store.settings?.hiddenApps.map((h) => h.id)).toEqual(['x', 'y']);
    store.edit((s) => {
      s.iconSize = 60;
    });
    const done = store.unhideApp('x');
    await tick();
    expect(backend.log.some((l) => l.startsWith('unhide_app'))).toBe(false);
    backend.settleUpdate();
    await done;
    expect(backend.log.slice(-2)).toEqual(['unhide_app:x', 'get_settings']);
    expect(store.settings?.hiddenApps).toEqual([{ id: 'y', name: 'Y' }]);
    expect(store.settings?.iconSize).toBe(60);
    expect(store.status).toBe('saved');
  });

  it('a draft made during Show takes over the fresh hidden apps', async () => {
    const backend = new FakeBackend();
    backend.stored.hiddenApps = [{ id: 'x', name: 'X' }];
    const store = new SettingsStore(backend, 150);
    await store.start();
    const done = store.unhideApp('x');
    store.edit((s) => {
      s.iconSize = 90;
    });
    await done;
    // The draft the debounce sends is the fresh list with the edit on top,
    // so the save does not bring the app back.
    expect(store.settings?.hiddenApps).toEqual([]);
    expect(store.settings?.iconSize).toBe(90);
    expect(store.status).toBe('saving');
    await vi.advanceTimersByTimeAsync(150);
    backend.settleUpdate();
    await tick();
    expect(store.status).toBe('saved');
  });

  it("a settings event during a pending edit hands the draft Rust's pins and hidden apps", async () => {
    const { backend, store } = await started();
    store.edit((s) => {
      s.iconSize = 70;
    });
    // The dock's menu hid an app and unpinned another meanwhile.
    const fromRust: Settings = {
      ...structuredClone(backend.stored),
      iconSize: 48,
      pinned: [pin('a'), pin('c')],
      hiddenApps: [{ id: 'x', name: 'X' }],
    };
    backend.emit('dock://settings', fromRust);
    // The edit stays; the lists are Rust's, on screen and in what is sent.
    expect(store.settings?.iconSize).toBe(70);
    expect(store.settings?.pinned.map((p) => p.id)).toEqual(['a', 'c']);
    expect(store.settings?.hiddenApps).toEqual([{ id: 'x', name: 'X' }]);
    expect(store.status).toBe('saving');
    const sent: Settings[] = [];
    const update = backend.updateSettings.bind(backend);
    backend.updateSettings = (s: Settings) => {
      sent.push(structuredClone(s));
      return update(s);
    };
    await vi.advanceTimersByTimeAsync(150);
    expect(sent).toHaveLength(1);
    expect(sent[0]?.iconSize).toBe(70);
    expect(sent[0]?.pinned.map((p) => p.id)).toEqual(['a', 'c']);
    expect(sent[0]?.hiddenApps).toEqual([{ id: 'x', name: 'X' }]);
  });

  /** The dock's menu hides `id` in Rust, which emits the new document. */
  const dockHides = (backend: FakeBackend, id: string): void => {
    backend.stored.hiddenApps = [...backend.stored.hiddenApps, { id, name: id.toUpperCase() }];
    backend.emit('dock://settings', structuredClone(backend.stored));
  };

  /** Records every document `update_settings` is sent from now on. */
  const captureSent = (backend: FakeBackend): Settings[] => {
    const sent: Settings[] = [];
    const update = backend.updateSettings.bind(backend);
    backend.updateSettings = (s: Settings) => {
      sent.push(structuredClone(s));
      return update(s);
    };
    return sent;
  };

  it("a hide whose event overtakes a running save's answer is kept, and the next edit sends it", async () => {
    const { backend, store } = await started();
    store.edit((s) => {
      s.iconSize = 60;
    });
    await vi.advanceTimersByTimeAsync(150);
    expect(backend.pendingUpdates).toHaveLength(1);
    // Rust saves the page's document and answers; the dock hides x right
    // after, and that event reaches the page before the answer does.
    backend.settleUpdate();
    dockHides(backend, 'x');
    expect(store.settings?.hiddenApps).toEqual([{ id: 'x', name: 'X' }]);
    expect(store.settings?.iconSize).toBe(60);
    await tick();
    // The answer (from before the hide) does not take the list back.
    expect(store.status).toBe('saved');
    expect(store.settings?.iconSize).toBe(60);
    expect(store.settings?.hiddenApps).toEqual([{ id: 'x', name: 'X' }]);
    const sent = captureSent(backend);
    store.edit((s) => {
      s.iconSize = 70;
    });
    await vi.advanceTimersByTimeAsync(150);
    expect(sent).toHaveLength(1);
    expect(sent[0]?.iconSize).toBe(70);
    expect(sent[0]?.hiddenApps).toEqual([{ id: 'x', name: 'X' }]);
  });

  it("a hide whose event overtakes a command's refetch is kept", async () => {
    const { backend, store } = await started();
    const get = backend.getSettings.bind(backend);
    let hideDuringGet = true;
    backend.getSettings = async () => {
      const fresh = await get();
      if (hideDuringGet) {
        hideDuringGet = false;
        dockHides(backend, 'x');
      }
      return fresh;
    };
    await store.unpinApp('b');
    expect(store.settings?.pinned.map((p) => p.id)).toEqual(['a', 'c']);
    expect(store.settings?.hiddenApps).toEqual([{ id: 'x', name: 'X' }]);
    expect(store.status).toBe('saved');
  });

  it("a hide whose event overtakes the reset's answer is kept", async () => {
    const { backend, store } = await started();
    const reset = backend.resetSettings.bind(backend);
    backend.resetSettings = async () => {
      const defaults = await reset();
      dockHides(backend, 'x');
      return defaults;
    };
    await store.reset();
    expect(store.settings).toEqual({ ...defaultSettings(), hiddenApps: [{ id: 'x', name: 'X' }] });
    expect(store.status).toBe('saved');
  });

  it('a failed command is reported', async () => {
    const { backend, store } = await started();
    backend.failNext = 'app "x" is not pinned';
    await store.unpinApp('x');
    expect(store.status).toBe('error');
    expect(store.error).toBe('app "x" is not pinned');
  });

  it('stop() cancels a pending save and unsubscribes', async () => {
    const { backend, store } = await started();
    store.edit((s) => {
      s.iconSize = 60;
    });
    store.stop();
    await vi.advanceTimersByTimeAsync(500);
    expect(backend.log.filter((l) => l.startsWith('update_settings'))).toEqual([]);
    backend.emit('dock://settings', { ...defaultSettings(), iconSize: 32 });
    expect(store.settings?.iconSize).toBe(60);
    backend.emit('dock://state', snapshot(5, [appItem('a', 'App A', null)]));
    expect(store.dockState?.revision).toBe(0);
  });
});
