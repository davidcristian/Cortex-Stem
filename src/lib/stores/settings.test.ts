import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type {
  Backend,
  BackendEvent,
  BackendEvents,
  EventHandler,
  Unsubscribe,
} from '$lib/ipc/backend';
import type { AppId } from '$lib/ipc/generated/AppId';
import type { Settings } from '$lib/ipc/generated/Settings';
import type { ThemeInfo } from '$lib/ipc/generated/ThemeInfo';
import { defaultSettings, validateSettings } from '$lib/ipc/mock';

import { SettingsStore } from './settings.svelte';

const THEME: ThemeInfo = { accent: '#0078D4', dark: true, highContrast: false, transparency: true };

const pin = (id: string) => ({ id, label: null, launch: { kind: 'uri' as const, uri: id } });

/** Scripted backend: a settings document, a call log, and manual control over latency. */
class FakeBackend implements Backend {
  readonly log: string[] = [];
  stored: Settings = { ...defaultSettings(), pinned: [pin('a'), pin('b'), pin('c')] };
  /** Pending `update_settings` resolvers, oldest first (resolved by the test). */
  readonly pendingUpdates: Array<(settings: Settings) => void> = [];
  failNext: string | null = null;
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

  async getTheme(): Promise<ThemeInfo> {
    this.log.push('get_theme');
    return THEME;
  }

  async getDockState() {
    return { revision: 0, items: [], foreground: null };
  }
  async getMonitors() {
    return [];
  }
  async primaryClick(): Promise<void> {}
  async windowAction(): Promise<void> {}
  async launchApp(): Promise<void> {}
  async pinApp(): Promise<void> {}
  async pinTarget(): Promise<string> {
    return '';
  }
  async getIcon(): Promise<string | null> {
    return null;
  }
  async setDockContentSize(): Promise<void> {}
  async setDockHitRegion(): Promise<void> {}
  async shellAction(): Promise<void> {}
  async openSettings(): Promise<void> {}
  async quit(): Promise<void> {}
  async showItemMenu(): Promise<void> {}
  async showDockMenu(): Promise<void> {}
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
      'get_settings',
      'get_theme',
    ]);
    expect(store.settings?.pinned.map((p) => p.id)).toEqual(['a', 'b', 'c']);
    expect(store.theme).toEqual(THEME);
    expect(store.status).toBe('saved');
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
  });
});
