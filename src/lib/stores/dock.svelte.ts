/**
 * Reactive owner of everything the dock renders: the latest `DockState`
 * snapshot, settings, OS theme, monitors, the bounce trigger, and the icon
 * cache. It follows the startup sequence of docs/ipc/events.md (subscribe,
 * then fetch) and drops snapshots older than the one on screen.
 */

import type { SvelteMap } from 'svelte/reactivity';

import type { Backend, Unsubscribe } from '$lib/ipc/backend';
import type { DockState } from '$lib/ipc/generated/DockState';
import type { IconKey } from '$lib/ipc/generated/IconKey';
import type { MonitorInfo } from '$lib/ipc/generated/MonitorInfo';
import type { Settings } from '$lib/ipc/generated/Settings';
import type { ThemeInfo } from '$lib/ipc/generated/ThemeInfo';

import { ReactiveIconCache } from './reactive-icon-cache';

/** `DockState.revision` is monotonic; anything older than what is shown is stale. */
export function acceptsRevision(current: DockState | null, next: DockState): boolean {
  return current === null || next.revision >= current.revision;
}

export class DockStore {
  /** Snapshots are replaced wholesale, never mutated, so raw state is enough. */
  state = $state.raw<DockState | null>(null);
  settings = $state.raw<Settings | null>(null);
  theme = $state.raw<ThemeInfo | null>(null);
  monitors = $state.raw<MonitorInfo[]>([]);
  /** Item to bounce; `bounceSerial` changes on every event so repeats re-trigger. */
  bounceKey = $state<string | null>(null);
  bounceSerial = $state(0);
  /** Stale snapshots ignored so far (diagnostics). */
  droppedSnapshots = $state(0);
  /** The icons the dock renders (the same class as the settings page's). */
  readonly iconCache: ReactiveIconCache;

  private subscriptions: Unsubscribe[] = [];
  private started = false;

  constructor(readonly backend: Backend) {
    this.iconCache = new ReactiveIconCache(backend);
  }

  /** Subscribe to every event first, then fetch the initial snapshots. */
  async start(): Promise<void> {
    if (this.started) {
      return;
    }
    this.started = true;
    this.subscriptions = [
      this.backend.on('dock://state', (s) => {
        this.applyState(s);
      }),
      this.backend.on('dock://settings', (s) => {
        this.settings = s;
      }),
      this.backend.on('dock://theme', (t) => {
        this.theme = t;
      }),
      this.backend.on('dock://monitors', (m) => {
        this.monitors = m;
      }),
      this.backend.on('dock://item-bounce', ({ itemKey }) => {
        this.bounce(itemKey);
      }),
    ];

    // The monitors too: `dock://monitors` fires at start, but the
    // platform may have reported them before this page subscribed, and a
    // dock page needs them to tell which monitor it sits on.
    const [state, settings, theme, monitors] = await Promise.allSettled([
      this.backend.getDockState(),
      this.backend.getSettings(),
      this.backend.getTheme(),
      this.backend.getMonitors(),
    ]);
    if (state.status === 'fulfilled') {
      this.applyState(state.value);
    } else {
      console.error('get_dock_state failed:', state.reason);
    }
    if (settings.status === 'fulfilled') {
      this.settings = settings.value;
    } else {
      console.error('get_settings failed:', settings.reason);
    }
    if (theme.status === 'fulfilled') {
      this.theme = theme.value;
    } else {
      console.error('get_theme failed:', theme.reason);
    }
    if (monitors.status === 'fulfilled') {
      // An event that arrived meanwhile is at least as fresh.
      if (this.monitors.length === 0) {
        this.monitors = monitors.value;
      }
    } else {
      console.error('get_monitors failed:', monitors.reason);
    }
  }

  stop(): void {
    for (const unsubscribe of this.subscriptions) {
      unsubscribe();
    }
    this.subscriptions = [];
    this.started = false;
  }

  /** Returns `false` (and changes nothing) for a stale snapshot. */
  applyState(next: DockState): boolean {
    if (!acceptsRevision(this.state, next)) {
      this.droppedSnapshots += 1;
      return false;
    }
    this.state = next;
    return true;
  }

  bounce(itemKey: string): void {
    this.bounceKey = itemKey;
    this.bounceSerial += 1;
  }

  /** Resolved icons by `iconId(key, size)`: a data URL, or `null` when there is none. */
  get icons(): SvelteMap<string, string | null> {
    return this.iconCache.icons;
  }

  /** Reactive read: a data URL, `null` when there is no icon, `undefined` while loading. */
  icon(key: IconKey, size: number): string | null | undefined {
    return this.iconCache.icon(key, size);
  }

  /** Idempotent; safe to call from an effect on every render. `shell:*` keys are never requested. */
  ensureIcon(key: IconKey, size: number): void {
    this.iconCache.ensureIcon(key, size);
  }

  /** Drop every cached icon so each is requested again. */
  clearIcons(): void {
    this.iconCache.clear();
  }

  /** Keep only the icons of `sizes`; the others are requested again if needed. */
  retainIconSizes(sizes: ReadonlySet<number>): void {
    this.iconCache.retainSizes(sizes);
  }
}
