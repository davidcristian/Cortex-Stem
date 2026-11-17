/**
 * Reactive owner of the settings window: the current `Settings`,
 * the OS theme, the dock's state (for the names and icons of the pinned
 * apps), the installation's facts for the About section,
 * and a save pipeline that applies edits live.
 *
 * The document and the save pipeline are the class it extends,
 * `SettingsPipeline` (settings-pipeline.svelte.ts).
 */

import type { SvelteMap } from 'svelte/reactivity';

import type { Backend, Unsubscribe } from '$lib/ipc/backend';
import type { AppFolder } from '$lib/ipc/generated/AppFolder';
import type { AppId } from '$lib/ipc/generated/AppId';
import type { AppInfo } from '$lib/ipc/generated/AppInfo';
import type { DockItem } from '$lib/ipc/generated/DockItem';
import type { DockState } from '$lib/ipc/generated/DockState';
import type { IconKey } from '$lib/ipc/generated/IconKey';
import type { ThemeInfo } from '$lib/ipc/generated/ThemeInfo';

import { acceptsRevision } from './dock.svelte';
import { ReactiveIconCache } from './reactive-icon-cache';
import { listsOf, SettingsPipeline } from './settings-pipeline.svelte';
import { describeError } from './toasts.svelte';

export type { SaveStatus } from './settings-pipeline.svelte';

export class SettingsStore extends SettingsPipeline {
  theme = $state.raw<ThemeInfo | null>(null);
  /**
   * The dock's latest snapshot: the item of a pinned app carries the title
   * and icon key the engine resolved for it. `null` until the first
   * one arrives; stale revisions are dropped like the dock does.
   */
  dockState = $state.raw<DockState | null>(null);
  /**
   * Version, settings file, log file and platform name for the About
   * section: `null` until `get_app_info` has answered (the facts
   * do not change during a run, so it is asked once).
   */
  appInfo = $state.raw<AppInfo | null>(null);
  /**
   * The About section's own notice: why its facts are missing, or why the
   * last *Open folder* failed. Cleared by the next attempt. Kept
   * apart from `error`, which is about saving.
   */
  aboutError = $state<string | null>(null);
  /** The pinned apps' icons (the same class as the dock's). */
  readonly iconCache: ReactiveIconCache;

  private subscriptions: Unsubscribe[] = [];
  private started = false;
  /**
   * A `dock://settings` has arrived since `start` subscribed. Until the
   * first `get_settings` answers, such an event is at least as new as the
   * answer may be: see `start`.
   */
  private sawSettingsEvent = false;

  constructor(backend: Backend, debounceMs?: number) {
    super(backend, debounceMs);
    this.iconCache = new ReactiveIconCache(backend);
  }

  /** Subscribe first, then fetch (docs/ipc/events.md startup sequence). */
  async start(): Promise<void> {
    if (this.started) {
      return;
    }
    this.started = true;
    this.sawSettingsEvent = false;
    this.subscriptions = [
      this.backend.on('dock://settings', (s) => {
        this.sawSettingsEvent = true;
        if (!this.busy) {
          this.missed = null;
          this.settings = s;
          return;
        }
        // While an edit of ours is pending or running, its own result is the
        // newer truth, except for the pins and hidden apps, which only Rust's
        // commands change (the dock's menu, a drop). Those are taken
        // at once: into the pending edit, so the page shows them (Rust keeps
        // its own lists on the save anyway, v1.26), or into what is shown, so
        // the next edit starts from them; and they are laid over the answer
        // of the request in flight, which may be older than this event (see
        // `withLatestLists`).
        // What else changed meanwhile is fetched once nothing is pending
        // (`catchUp`).
        this.missed = s;
        const lists = listsOf(s);
        this.latestLists = lists;
        if (this.draft !== null) {
          this.draft = { ...this.draft, ...lists };
          this.settings = this.draft;
        } else if (this.settings !== null) {
          this.settings = { ...this.settings, ...lists };
        }
      }),
      this.backend.on('dock://theme', (t) => {
        this.theme = t;
      }),
      this.backend.on('dock://state', (s) => {
        this.applyState(s);
      }),
      this.backend.on('dock://settings-closing', () => {
        void this.closing();
      }),
    ];
    const [settings, theme, state, info] = await Promise.allSettled([
      this.backend.getSettings(),
      this.backend.getTheme(),
      this.backend.getDockState(),
      this.backend.getAppInfo(),
    ]);
    if (settings.status === 'fulfilled') {
      if (!this.busy) {
        // `Settings` carries no revision, so the answer cannot say whether
        // it is newer than a `dock://settings` that came in while it was
        // on its way (a pin from the dock, the tray's click-through). The
        // event is kept: Rust emits one after every change, in
        // order, and the page subscribed before asking, so the answer can
        // only be newer than the event if a change came between them,
        // whose own event is still on its way and puts the page right.
        if (!this.sawSettingsEvent) {
          this.settings = settings.value;
        }
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
    if (info.status === 'fulfilled') {
      this.appInfo = info.value;
    } else {
      // The rest of the page works; the About section says why it is empty.
      this.aboutError = describeError(info.reason);
      console.error('get_app_info failed:', info.reason);
    }
  }

  /**
   * Open the log folder or the settings file's folder in Explorer.
   * Not a settings command: it changes no document, so it waits for nothing
   * and leaves the save status alone; a failure is the About section's
   * notice.
   */
  async openAppFolder(folder: AppFolder): Promise<void> {
    this.aboutError = null;
    try {
      await this.backend.openAppFolder(folder);
    } catch (err) {
      this.aboutError = describeError(err);
      console.error(`open_app_folder(${folder}) failed:`, err);
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

  /**
   * The window is closing (`dock://settings-closing`): Rust has
   * hidden it and destroys it a moment later. What the page still holds
   * back goes now: a typed value in the focused field (its blur commits
   * it; a text field commits only on Enter or blur) and the debounced
   * draft, sent at once instead of after the debounce.
   */
  async closing(): Promise<void> {
    if (typeof document !== 'undefined' && document.activeElement instanceof HTMLElement) {
      document.activeElement.blur();
    }
    await this.settle();
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
}
