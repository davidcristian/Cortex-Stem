/**
 * Reactive owner of the settings window: the current `Settings`,
 * the OS theme, and a save pipeline that applies edits live.
 *
 * Edits are optimistic on screen and sent as one `update_settings` after a
 * short debounce; the value Rust returns (validated, clamped) replaces the
 * draft. Pin commands and the reset go through their own IPC commands; they
 * wait for pending edits first so a stale full document can never overwrite
 * what the command just changed.
 */

import type { Backend, Unsubscribe } from '$lib/ipc/backend';
import type { AppId } from '$lib/ipc/generated/AppId';
import type { Settings } from '$lib/ipc/generated/Settings';
import type { ThemeInfo } from '$lib/ipc/generated/ThemeInfo';

import { describeError } from './toasts.svelte';

export type SaveStatus = 'loading' | 'saved' | 'saving' | 'error';

export class SettingsStore {
  /** What the page renders: the persisted document, or the draft while an edit is pending. */
  settings = $state.raw<Settings | null>(null);
  theme = $state.raw<ThemeInfo | null>(null);
  status = $state<SaveStatus>('loading');
  /** Message of the last failed command, for the footer. */
  error = $state<string | null>(null);

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
  ) {}

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
    ];
    const [settings, theme] = await Promise.allSettled([
      this.backend.getSettings(),
      this.backend.getTheme(),
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
