/**
 * Icon cache per key + size (docs/ipc/icon-keys.md): one request per entry,
 * a bounded number in flight, one retry on failure. Framework-free so it can
 * be unit-tested; `DockStore` mirrors resolved entries into reactive state.
 */

export type IconLoader = (key: string, size: number) => Promise<string | null>;

/** Called once per entry when it settles; `url` is `null` when there is no icon. */
export type IconResolved = (id: string, key: string, size: number, url: string | null) => void;

export interface IconCacheOptions {
  /** Maximum requests in flight at once (default 4). */
  concurrency?: number;
  /** Extra attempts after a failed load (default 1). */
  retries?: number;
}

export function iconId(key: string, size: number): string {
  return `${key}@${size}`;
}

export class IconCache {
  private readonly done = new Map<string, string | null>();
  private readonly inflight = new Map<string, Promise<string | null>>();
  private readonly queue: Array<() => void> = [];
  private active = 0;
  private readonly concurrency: number;
  private readonly retries: number;

  constructor(
    private readonly load: IconLoader,
    private readonly onResolved: IconResolved,
    options: IconCacheOptions = {},
  ) {
    this.concurrency = Math.max(1, options.concurrency ?? 4);
    this.retries = Math.max(0, options.retries ?? 1);
  }

  /** A URL, `null` when the backend has no icon, `undefined` when not loaded. */
  get(key: string, size: number): string | null | undefined {
    return this.done.get(iconId(key, size));
  }

  /** Requests waiting or in flight. */
  get pending(): number {
    return this.active + this.queue.length;
  }

  /** Idempotent: a key+size is loaded once, even when asked for repeatedly. */
  request(key: string, size: number): Promise<string | null> {
    const id = iconId(key, size);
    const cached = this.done.get(id);
    if (cached !== undefined) {
      return Promise.resolve(cached);
    }
    const running = this.inflight.get(id);
    if (running) {
      return running;
    }
    const promise = this.schedule(() => this.loadWithRetry(key, size)).then((url) => {
      this.inflight.delete(id);
      this.done.set(id, url);
      this.onResolved(id, key, size, url);
      return url;
    });
    this.inflight.set(id, promise);
    return promise;
  }

  /** Forget settled entries (e.g. after a DPI change); in-flight loads still complete. */
  clear(): void {
    this.done.clear();
  }

  private schedule<T>(job: () => Promise<T>): Promise<T> {
    return new Promise<T>((resolve, reject) => {
      const run = (): void => {
        this.active += 1;
        job()
          .then(resolve, reject)
          .finally(() => {
            this.active -= 1;
            this.next();
          });
      };
      if (this.active < this.concurrency) {
        run();
      } else {
        this.queue.push(run);
      }
    });
  }

  private next(): void {
    if (this.active < this.concurrency) {
      this.queue.shift()?.();
    }
  }

  private async loadWithRetry(key: string, size: number): Promise<string | null> {
    for (let attempt = 0; ; attempt += 1) {
      try {
        return await this.load(key, size);
      } catch (err) {
        if (attempt >= this.retries) {
          console.warn(`icon ${key}@${size} failed after ${attempt + 1} attempt(s):`, err);
          return null;
        }
      }
    }
  }
}
