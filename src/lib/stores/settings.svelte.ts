/**
 * Reactive owner of the settings window: the current `Settings`,
 * the OS theme, the dock's state (for the names and icons of the pinned
 * apps), and a save pipeline that applies edits live.
 *
 * Edits are optimistic on screen and sent as one `update_settings` after a
 * short debounce; the value Rust returns (validated, clamped) replaces the
 * draft. Pin commands and the reset go through their own IPC commands; they
 * wait for pending edits first so a stale full document can never overwrite
 * what the command just changed.
 */

import { SvelteMap } from 'svelte/reactivity';

import { isShellIconKey, type Backend, type Unsubscribe } from '$lib/ipc/backend';
import type { AppId } from '$lib/ipc/generated/AppId';
import type { DockItem } from '$lib/ipc/generated/DockItem';
import type { DockState } from '$lib/ipc/generated/DockState';
import type { IconKey } from '$lib/ipc/generated/IconKey';
import type { Settings } from '$lib/ipc/generated/Settings';
import type { ThemeInfo } from '$lib/ipc/generated/ThemeInfo';

import { acceptsRevision } from './dock.svelte';
import { IconCache, iconId } from './icon-cache';
import { describeError } from './toasts.svelte';

export type SaveStatus = 'loading' | 'saved' | 'saving' | 'error';

export class SettingsStore {
  /** What the page renders: the persisted document, or the draft while an edit is pending. */
  settings = $state.raw<Settings | null>(null);
  theme = $state.raw<ThemeInfo | null>(null);
  /**
   * The dock's latest snapshot: the item of a pinned app carries the title
   * and icon key the engine resolved for it. `null` until the first
   * one arrives; stale revisions are dropped like the dock does.
   */
  dockState = $state.raw<DockState | null>(null);
  status = $state<SaveStatus>('loading');
  /** Message of the last failed command, for the footer. */
  error = $state<string | null>(null);
  /** Resolved icons by `iconId(key, size)`: a data URL, or `null` when there is none. */
  readonly icons = new SvelteMap<string, string | null>();

  private readonly cache: IconCache;
  private subscriptions: Unsubscribe[] = [];
  private started = false;
  /** Edited but not yet sent. */
  private draft: Settings | null = null;
  private timer: ReturnType<typeof setTimeout> | null = null;
  /** The save or command currently running; nothing else starts until it settles. */
  private inflight: Promise<void> | null = null;

  constructor(
    readonly backend: Backend,
    private readonly debounceMs = 150,
  ) {
    this.cache = new IconCache(
      (key, size) => backend.getIcon(key, size),
      (id, _key, _size, url) => {
        this.icons.set(id, url);
      },
      { concurrency: 4, retries: 1 },
    );
  }

  /** Subscribe first, then fetch (docs/ipc/events.md startup sequence). */
  async start(): Promise<void> {
    if (this.started) {
      return;
    }
    this.started = true;
    this.subscriptions = [
      this.backend.on('dock://settings', (s) => {
        // While an edit of ours is pending, its own result is the newer truth.
        if (!this.busy) {
          this.settings = s;
        }
      }),
      this.backend.on('dock://theme', (t) => {
        this.theme = t;
      }),
      this.backend.on('dock://state', (s) => {
        this.applyState(s);
      }),
    ];
    const [settings, theme, state] = await Promise.allSettled([
      this.backend.getSettings(),
      this.backend.getTheme(),
      this.backend.getDockState(),
    ]);
    if (settings.status === 'fulfilled') {
      if (!this.busy) {
        this.settings = settings.value;
        this.status = 'saved';
      }
    } else {
      this.fail(settings.reason);
    }
    if (theme.status === 'fulfilled') {
      this.theme = theme.value;
    } else {
      console.error('get_theme failed:', theme.reason);
    }
    if (state.status === 'fulfilled') {
      this.applyState(state.value);
    } else {
      // The page still works without it: pins show their id and a placeholder.
      console.error('get_dock_state failed:', state.reason);
    }
  }

  stop(): void {
    for (const unsubscribe of this.subscriptions) {
      unsubscribe();
    }
    this.subscriptions = [];
    if (this.timer !== null) {
      clearTimeout(this.timer);
      this.timer = null;
    }
    this.started = false;
  }

  /** An edit is pending or a save/command is running. */
  get busy(): boolean {
    return this.timer !== null || this.draft !== null || this.inflight !== null;
  }

  /** Returns `false` (and changes nothing) for a stale snapshot. */
  applyState(next: DockState): boolean {
    if (!acceptsRevision(this.dockState, next)) {
      return false;
    }
    this.dockState = next;
    return true;
  }

  /** The dock's item for `appId` (its resolved title and icon key), if the snapshot has one. */
  dockItem(appId: AppId): DockItem | undefined {
    return this.dockState?.items.find((item) => item.appId === appId);
  }

  /** Reactive read: a data URL, `null` when there is no icon, `undefined` while loading. */
  icon(key: IconKey, size: number): string | null | undefined {
    return this.icons.get(iconId(key, size));
  }

  /** Idempotent; safe to call from an effect on every render. `shell:*` keys are never requested. */
  ensureIcon(key: IconKey, size: number): void {
    if (isShellIconKey(key)) {
      return;
    }
    void this.cache.request(key, size);
  }

  /**
   * Apply `mutate` to a copy of the current document, show it at once, and
   * send it after the debounce. Repeated edits coalesce into one save.
   */
  edit(mutate: (draft: Settings) => void): void {
    const base = this.draft ?? this.settings;
    if (base === null) {
      return;
    }
    const next = structuredClone(base);
    mutate(next);
    this.draft = next;
    this.settings = next;
    this.status = 'saving';
    this.error = null;
    if (this.timer !== null) {
      clearTimeout(this.timer);
    }
    this.timer = setTimeout(() => {
      this.timer = null;
      this.flush();
    }, this.debounceMs);
  }

  reorderPinned(order: AppId[]): Promise<void> {
    return this.command(() => this.backend.reorderPinned(order));
  }

  unpinApp(appId: AppId): Promise<void> {
    return this.command(() => this.backend.unpinApp(appId));
  }

  /** Drop pending edits and restore the defaults. */
  async reset(): Promise<void> {
    await this.settle();
    this.status = 'saving';
    this.error = null;
    this.inflight = this.backend
      .resetSettings()
      .then((saved) => {
        this.settings = saved;
        this.status = 'saved';
      })
      .catch((err: unknown) => this.fail(err))
      .finally(() => this.finish());
    await this.inflight;
  }

  // --- internals

  /** Start the save of the draft, if nothing else is running. */
  private flush(): void {
    if (this.inflight !== null || this.draft === null) {
      return;
    }
    const draft = this.draft;
    this.draft = null;
    this.inflight = this.backend
      .updateSettings(draft)
      .then((saved) => {
        // A newer draft, if any, brings its own authoritative value.
        if (this.draft === null) {
          this.settings = saved;
          this.status = 'saved';
        }
      })
      .catch((err: unknown) => this.fail(err))
      .finally(() => this.finish());
  }

  /** Wait until no edit is pending and nothing is running. */
  private async settle(): Promise<void> {
    if (this.timer !== null) {
      clearTimeout(this.timer);
      this.timer = null;
    }
    for (;;) {
      if (this.inflight !== null) {
        await this.inflight;
        continue;
      }
      if (this.draft === null) {
        return;
      }
      this.flush();
    }
  }

  /**
   * A command that changes settings on the Rust side (pins). Pending edits go
   * first; afterwards the document is fetched again and a draft made in the
   * meantime takes over the fresh pins, which edits never touch.
   */
  private async command(run: () => Promise<void>): Promise<void> {
    await this.settle();
    this.status = 'saving';
    this.error = null;
    this.inflight = run()
      .then(() => this.backend.getSettings())
      .then((fresh) => {
        if (this.draft !== null) {
          this.draft = { ...this.draft, pinned: fresh.pinned };
          this.settings = this.draft;
        } else {
          this.settings = fresh;
          this.status = 'saved';
        }
      })
      .catch((err: unknown) => this.fail(err))
      .finally(() => this.finish());
    await this.inflight;
  }

  private finish(): void {
    this.inflight = null;
    if (this.draft !== null && this.timer === null) {
      this.flush();
    }
  }

  private fail(err: unknown): void {
    this.status = 'error';
    this.error = describeError(err);
    console.error('settings command failed:', err);
  }
}
