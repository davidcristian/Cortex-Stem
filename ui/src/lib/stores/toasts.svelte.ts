/**
 * Transient notices for failed commands
 * (docs/architecture/error-handling.md): a short queue whose entries
 * vanish after `ttlMs`. The dock renders them beside the panel; nothing
 * here blocks or needs a click.
 */

export interface Toast {
  id: number;
  message: string;
}

interface Pending {
  id: number;
  timer: ReturnType<typeof setTimeout>;
}

export class Toasts {
  /** Oldest first. Replaced wholesale on every change, never mutated. */
  items = $state.raw<Toast[]>([]);

  private nextId = 1;
  private pending: Pending[] = [];

  constructor(
    private readonly ttlMs = 4000,
    private readonly max = 3,
  ) {}

  /** Show `message`; the oldest entry makes room when more than `max` are up. */
  push(message: string): number {
    const id = this.nextId++;
    const next = [...this.items, { id, message }];
    while (next.length > this.max) {
      this.forget(next.shift()!.id);
    }
    this.items = next;
    this.pending.push({ id, timer: setTimeout(() => this.dismiss(id), this.ttlMs) });
    return id;
  }

  dismiss(id: number): void {
    this.forget(id);
    if (this.items.some((t) => t.id === id)) {
      this.items = this.items.filter((t) => t.id !== id);
    }
  }

  clear(): void {
    for (const p of this.pending) {
      clearTimeout(p.timer);
    }
    this.pending = [];
    this.items = [];
  }

  private forget(id: number): void {
    const index = this.pending.findIndex((p) => p.id === id);
    if (index !== -1) {
      clearTimeout(this.pending[index]!.timer);
      this.pending.splice(index, 1);
    }
  }
}

/** One line for a toast: Rust errors cross IPC as strings; anything else is described. */
export function describeError(err: unknown): string {
  if (typeof err === 'string') {
    return err;
  }
  if (err instanceof Error) {
    return err.message;
  }
  return String(err);
}
