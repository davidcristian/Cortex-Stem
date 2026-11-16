/**
 * The document of the settings window and how edits and commands reach Rust:
 * the save pipeline `SettingsStore` extends.
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

import type { Backend } from '$lib/ipc/backend';
import type { AppId } from '$lib/ipc/generated/AppId';
import type { Settings } from '$lib/ipc/generated/Settings';

import { describeError } from './toasts.svelte';

export type SaveStatus = 'loading' | 'saved' | 'saving' | 'error';

/** The parts of `Settings` that only Rust's commands change (pins, hidden apps). */
type RustLists = Pick<Settings, 'pinned' | 'hiddenApps'>;

export const listsOf = (s: Settings): RustLists => ({ pinned: s.pinned, hiddenApps: s.hiddenApps });

export class SettingsPipeline {
  /** What the page renders: the persisted document, or the draft while an edit is pending. */
  settings = $state.raw<Settings | null>(null);
  status = $state<SaveStatus>('loading');
  /** Message of the last failed command, for the footer. */
  error = $state<string | null>(null);
  /** Edited but not yet sent. */
  protected draft: Settings | null = null;
  protected timer: ReturnType<typeof setTimeout> | null = null;
  /** The save or command currently running; nothing else starts until it settles. */
  private inflight: Promise<void> | null = null;
  /**
   * The pins and hidden apps of the last `dock://settings` that arrived
   * while busy, since the running request was sent; `null` if none did.
   */
  protected latestLists: RustLists | null = null;
  /**
   * The latest `dock://settings` that arrived while busy and was not shown:
   * a change made elsewhere meanwhile (a pin from the dock, the tray's
   * click-through) unless it is the echo of our own save. Looked at once
   * nothing is pending (`catchUp`).
   */
  protected missed: Settings | null = null;

  constructor(
    readonly backend: Backend,
    private readonly debounceMs = 150,
  ) {}

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
  protected async settle(): Promise<void> {
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

  protected fail(err: unknown): void {
    this.status = 'error';
    this.error = describeError(err);
    console.error('settings command failed:', err);
  }
}
