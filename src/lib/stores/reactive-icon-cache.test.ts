import { describe, expect, it, vi } from 'vitest';

import { iconId } from './icon-cache';
import { ReactiveIconCache } from './reactive-icon-cache';

/** `get_icon` answering at once, every request recorded as `key@size`; `missing` has no icon. */
function fakeBackend() {
  const requests: string[] = [];
  return {
    requests,
    getIcon: async (key: string, size: number): Promise<string | null> => {
      requests.push(iconId(key, size));
      return key === 'missing' ? null : `data:${key}@${size}`;
    },
  };
}

const flush = (): Promise<void> => new Promise((r) => setTimeout(r, 0));

describe('ReactiveIconCache', () => {
  it('requests each icon once per key and size, never a shell: key, and caches null', async () => {
    const backend = fakeBackend();
    const cache = new ReactiveIconCache(backend);
    cache.ensureIcon('mock:a', 96);
    cache.ensureIcon('mock:a', 96);
    cache.ensureIcon('mock:a', 48);
    cache.ensureIcon('shell:start', 96);
    cache.ensureIcon('missing', 96);
    expect(cache.icon('mock:a', 96)).toBeUndefined();
    await flush();
    expect(backend.requests).toEqual(['mock:a@96', 'mock:a@48', 'missing@96']);

    expect(cache.icon('mock:a', 96)).toBe('data:mock:a@96');
    // Two sizes of one key are two entries (rest and magnified bitmaps,
    // or a display with another scale).
    expect(cache.icon('mock:a', 48)).toBe('data:mock:a@48');
    expect(cache.icons.get(iconId('mock:a', 48))).toBe('data:mock:a@48');
    // The backend has no icon for this key: `null`, not a broken image.
    expect(cache.icon('missing', 96)).toBeNull();
    // A shell tile is drawn by the page and never enters the map.
    expect(cache.icon('shell:start', 96)).toBeUndefined();
    expect(cache.icons.has(iconId('shell:start', 96))).toBe(false);

    // Settled entries, `null` included, are not asked for again.
    cache.ensureIcon('mock:a', 96);
    cache.ensureIcon('missing', 96);
    cache.ensureIcon('shell:start', 96);
    await flush();
    expect(backend.requests).toHaveLength(3);
  });

  it('clear() forgets every entry so the next ensureIcon requests it again', async () => {
    const backend = fakeBackend();
    const cache = new ReactiveIconCache(backend);
    cache.ensureIcon('mock:a', 96);
    await flush();
    expect(cache.icons.size).toBe(1);

    cache.clear();
    expect(cache.icon('mock:a', 96)).toBeUndefined();
    expect(cache.icons.size).toBe(0);
    cache.ensureIcon('mock:a', 96);
    await flush();
    expect(backend.requests).toEqual(['mock:a@96', 'mock:a@96']);
    expect(cache.icon('mock:a', 96)).toBe('data:mock:a@96');
  });

  it('keeps four loads in flight and retries a failed one once by default', async () => {
    const pending: Array<() => void> = [];
    const slow = {
      getIcon: vi.fn(
        (key: string) =>
          new Promise<string | null>((resolve) => {
            pending.push(() => resolve(`data:${key}`));
          }),
      ),
    };
    const cache = new ReactiveIconCache(slow);
    for (let i = 0; i < 6; i += 1) {
      cache.ensureIcon(`k${i}`, 48);
    }
    await flush();
    expect(slow.getIcon).toHaveBeenCalledTimes(4);
    pending.shift()?.();
    await flush();
    expect(slow.getIcon).toHaveBeenCalledTimes(5);
    expect(cache.icon('k0', 48)).toBe('data:k0');

    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    const broken = {
      getIcon: vi.fn(async (): Promise<string | null> => {
        throw new Error('down');
      }),
    };
    const failing = new ReactiveIconCache(broken);
    failing.ensureIcon('exe:a', 48);
    await flush();
    await flush();
    expect(broken.getIcon).toHaveBeenCalledTimes(2);
    expect(failing.icon('exe:a', 48)).toBeNull();
    expect(warn).toHaveBeenCalledTimes(1);
    warn.mockRestore();
  });
});
