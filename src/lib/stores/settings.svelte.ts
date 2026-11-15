/**
 * Reactive owner of the settings window: the current `Settings`,
 * the OS theme, the dock's state (for the names and icons of the pinned
 * apps), the installation's facts for the About section,
 * and a save pipeline that applies edits live.
 *
 * Edits are optimistic on screen and sent as one `update_settings` after a
 * short debounce; the value Rust returns (validated, clamped) replaces the
 * draft. Pin commands, *Show* for a hidden app and the reset go
 * through their own IPC commands; they wait for pending edits first so the
 * page shows them in order. The pins and hidden apps change only through
 * those commands (from this page or the dock): Rust's `update_settings`
 * keeps its own lists whatever the document says (IPC v1.26), so no save
 * can undo a pin or a hide, even one that reaches Rust while the save is
 * on its way. The page takes the lists from every `dock://settings`, even
 * while an edit is pending or a request is running, so that it shows them.
 */

import type { SvelteMap } from 'svelte/reactivity';

import type { Backend, Unsubscribe } from '$lib/ipc/backend';
import type { AppFolder } from '$lib/ipc/generated/AppFolder';
import type { AppId } from '$lib/ipc/generated/AppId';
import type { AppInfo } from '$lib/ipc/generated/AppInfo';
import type { DockItem } from '$lib/ipc/generated/DockItem';
import type { DockState } from '$lib/ipc/generated/DockState';
import type { IconKey } from '$lib/ipc/generated/IconKey';
import type { Settings } from '$lib/ipc/generated/Settings';
import type { ThemeInfo } from '$lib/ipc/generated/ThemeInfo';

import { acceptsRevision } from './dock.svelte';
import { ReactiveIconCache } from './reactive-icon-cache';
import { describeError } from './toasts.svelte';

export type SaveStatus = 'loading' | 'saved' | 'saving' | 'error';

/** The parts of `Settings` that only Rust's commands change (pins, hidden apps). */
type RustLists = Pick<Settings, 'pinned' | 'hiddenApps'>;

const listsOf = (s: Settings): RustLists => ({ pinned: s.pinned, hiddenApps: s.hiddenApps });

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
  /** Edited but not yet sent. */
  private draft: Settings | null = null;
  private timer: ReturnType<typeof setTimeout> | null = null;
  /** The save or command currently running; nothing else starts until it settles. */
  private inflight: Promise<void> | null = null;
  /**
   * The pins and hidden apps of the last `dock://settings` that arrived
   * while busy, since the running request was sent; `null` if none did.
   */
  private latestLists: RustLists | null = null;
  /**
   * The latest `dock://settings` that arrived while busy and was not shown:
   * a change made elsewhere meanwhile (a pin from the dock, the tray's
   * click-through) unless it is the echo of our own save. Looked at once
   * nothing is pending (`catchUp`).
   */
  private missed: Settings | null = null;

  constructor(
    readonly backend: Backend,
    private readonly debounceMs = 150,
  ) {
    this.iconCache = new ReactiveIconCache(backend);
  }

  /** Subscribe first, then fetch (docs/ipc/events.md startup sequence). */
  async start(): Promise<void> {
    if (this.started) {
      return;
    }
    this.started = true;
    this.subscriptions = [
      this.backend.on('dock://settings', (s) => {
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
    ];
    const [settings, theme, state, info] = await Promise.allSettled([
      this.backend.getSettings(),
      this.backend.getTheme(),
      this.backend.getDockState(),
      this.backend.getAppInfo(),
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

  /** Show a hidden app on the dock again (*Show* in Hidden apps). */
  unhideApp(appId: AppId): Promise<void> {
    return this.command(() => this.backend.unhideApp(appId));
  }

  /** Drop pending edits and restore the defaults. */
  async reset(): Promise<void> {
    await this.settle();
    this.status = 'saving';
    this.error = null;
    this.latestLists = null;
    this.inflight = this.backend
      .resetSettings()
      .then((saved) => {
        this.settings = this.withLatestLists(saved);
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
    // The draft already carries the lists of every event so far.
    this.latestLists = null;
    this.inflight = this.backend
      .updateSettings(draft)
      .then((saved) => {
        // A newer draft, if any, brings its own authoritative value.
        if (this.draft === null) {
          this.settings = this.withLatestLists(saved);
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
   * A command that changes settings on the Rust side (pins, hidden apps).
   * Pending edits go first; afterwards the document is fetched again and a
   * draft made in the meantime takes over the fresh pins and hidden apps,
   * which edits never touch.
   */
  private async command(run: () => Promise<void>): Promise<void> {
    await this.settle();
    this.status = 'saving';
    this.error = null;
    this.inflight = run()
      .then(() => {
        // What `get_settings` answers is at least as new as any event so far.
        this.latestLists = null;
        return this.backend.getSettings();
      })
      .then((fresh) => {
        const current = this.withLatestLists(fresh);
        if (this.draft !== null) {
          this.draft = { ...this.draft, ...listsOf(current) };
          this.settings = this.draft;
        } else {
          this.settings = current;
          this.status = 'saved';
        }
      })
      .catch((err: unknown) => this.fail(err))
      .finally(() => this.finish());
    await this.inflight;
  }

  /**
   * `doc` (the answer of a request) with the pins and hidden apps of an
   * event that arrived while the request ran. Rust emits `dock://settings`
   * after every change, in the order of the changes, each with the whole
   * current document, but an answer and an event travel apart: the event
   * of a change the dock made just after this request (a hide from its
   * menu) can arrive before the answer, which then lacks it. Were
   * the answer shown as it is, the page would show stale lists until the
   * next change (the next edit's save could not undo this one: Rust keeps
   * its own lists, v1.26). An event that is instead older than the answer
   * is always followed by a newer one, which puts the lists right again.
   */
  private withLatestLists(doc: Settings): Settings {
    return this.latestLists === null ? doc : { ...doc, ...this.latestLists };
  }

  private finish(): void {
    this.inflight = null;
    if (this.draft !== null && this.timer === null) {
      this.flush();
      return;
    }
    this.catchUp();
  }

  /**
   * Nothing is pending any more: when a `dock://settings` was skipped
   * meanwhile and says something else than what the page shows, fetch the
   * document again, so the page, and the next edit's copy, start from what
   * Rust has. Rust keeps the pins whatever a save sends (v1.25), but a
   * stale copy would still show the old ones, and would send back an old
   * value of any other field changed elsewhere (the tray's click-through).
   */
  private catchUp(): void {
    if (this.missed === null || this.busy) {
      return;
    }
    const missed = this.missed;
    this.missed = null;
    if (this.settings !== null && JSON.stringify(missed) === JSON.stringify(this.settings)) {
      return;
    }
    this.inflight = this.backend
      .getSettings()
      .then((fresh) => {
        if (this.draft !== null) {
          // An edit made meanwhile keeps its own values but shows the fresh pins.
          this.draft = { ...this.draft, pinned: fresh.pinned };
          this.settings = this.draft;
        } else if (this.timer === null) {
          this.settings = fresh;
        }
      })
      .catch((err: unknown) => {
        console.error('get_settings failed:', err);
      })
      .finally(() => this.finish());
  }

  private fail(err: unknown): void {
    this.status = 'error';
    this.error = describeError(err);
    console.error('settings command failed:', err);
  }
}
