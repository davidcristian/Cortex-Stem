import { describe, expect, it, vi } from 'vitest';

import { IconCache, iconId } from './icon-cache';

interface Deferred {
  key: string;
  size: number;
  resolve: (url: string | null) => void;
  reject: (err: unknown) => void;
}

/** A loader whose requests stay pending until the test settles them. */
function deferredLoader() {
  const pending: Deferred[] = [];
  const load = (key: string, size: number): Promise<string | null> =>
    new Promise((resolve, reject) => {
      pending.push({ key, size, resolve, reject });
    });
  return { pending, load };
}

const flush = (): Promise<void> => new Promise((r) => setTimeout(r, 0));

describe('IconCache', () => {
  it('loads each key+size once and reports the result', async () => {
    const load = vi.fn(async (key: string, size: number) => `data:${key}@${size}`);
    const resolved: string[] = [];
    const cache = new IconCache(load, (id) => resolved.push(id));

    const [a, b, c] = await Promise.all([
      cache.request('k', 48),
      cache.request('k', 48),
      cache.request('k', 96),
    ]);
    expect(a).toBe('data:k@48');
    expect(b).toBe('data:k@48');
    expect(c).toBe('data:k@96');
    expect(load).toHaveBeenCalledTimes(2);
    expect(resolved).toEqual([iconId('k', 48), iconId('k', 96)]);

    // Settled entries are served from the cache without a new load.
    expect(cache.get('k', 48)).toBe('data:k@48');
    expect(await cache.request('k', 48)).toBe('data:k@48');
    expect(load).toHaveBeenCalledTimes(2);
  });

  it('caches "no icon" (null) answers', async () => {
    const load = vi.fn(async () => null);
    const cache = new IconCache(load, () => {});
    expect(await cache.request('none', 32)).toBeNull();
    expect(cache.get('none', 32)).toBeNull();
    expect(await cache.request('none', 32)).toBeNull();
    expect(load).toHaveBeenCalledTimes(1);
    expect(cache.get('other', 32)).toBeUndefined();
  });

  it('keeps at most `concurrency` loads in flight', async () => {
    const { pending, load } = deferredLoader();
    const cache = new IconCache(load, () => {}, { concurrency: 4, retries: 0 });
    const all = Array.from({ length: 10 }, (_, i) => cache.request(`k${i}`, 48));
    await flush();
    expect(pending).toHaveLength(4);
    expect(cache.pending).toBe(10);

    pending[0]!.resolve('a');
    await flush();
    expect(pending).toHaveLength(5);
    expect(pending[4]!.key).toBe('k4');

    for (const p of pending.splice(0)) {
      p.resolve(`url:${p.key}`);
    }
    await flush();
    for (const p of pending.splice(0)) {
      p.resolve(`url:${p.key}`);
    }
    await flush();
    for (const p of pending.splice(0)) {
      p.resolve(`url:${p.key}`);
    }
    const urls = await Promise.all(all);
    expect(urls).toHaveLength(10);
    expect(urls[9]).toBe('url:k9');
    expect(cache.pending).toBe(0);
  });

  it('retries a failed load once, then settles with null', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    let calls = 0;
    const flaky = vi.fn(async (key: string) => {
      calls += 1;
      if (calls === 1) {
        throw new Error('transient');
      }
      return `ok:${key}`;
    });
    const cache = new IconCache(flaky, () => {}, { retries: 1 });
    expect(await cache.request('flaky', 48)).toBe('ok:flaky');
    expect(flaky).toHaveBeenCalledTimes(2);
    expect(warn).not.toHaveBeenCalled();

    const broken = vi.fn(async () => {
      throw new Error('down');
    });
    const resolved: Array<string | null> = [];
    const cache2 = new IconCache(broken, (_id, _k, _s, url) => resolved.push(url), { retries: 1 });
    expect(await cache2.request('broken', 48)).toBeNull();
    expect(broken).toHaveBeenCalledTimes(2);
    expect(resolved).toEqual([null]);
    expect(warn).toHaveBeenCalledTimes(1);
    warn.mockRestore();
  });

  it('clear() forgets settled entries so they load again', async () => {
    const load = vi.fn(async () => 'x');
    const cache = new IconCache(load, () => {});
    await cache.request('k', 48);
    cache.clear();
    expect(cache.get('k', 48)).toBeUndefined();
    await cache.request('k', 48);
    expect(load).toHaveBeenCalledTimes(2);
  });
});
