/**
 * The icons a page renders, as reactive state: an `IconCache` over
 * the backend's `get_icon`, mirrored into a `SvelteMap` so a component that
 * reads an entry re-renders when it settles. `DockStore` and `SettingsStore`
 * each own one and expose its surface (`icon`, `ensureIcon`, `icons`) as
 * their own.
 */

import { SvelteMap } from 'svelte/reactivity';

import { isShellIconKey, type Backend } from '$lib/ipc/backend';
import type { IconKey } from '$lib/ipc/generated/IconKey';

import { IconCache, iconId, type IconCacheOptions } from './icon-cache';

/** Four loads in flight and one retry, what both pages have always used. */
const DEFAULT_OPTIONS: IconCacheOptions = { concurrency: 4, retries: 1 };

export class ReactiveIconCache {
  /** Resolved icons by `iconId(key, size)`: a data URL, or `null` when there is none. */
  readonly icons = new SvelteMap<string, string | null>();

  private readonly cache: IconCache;

  constructor(backend: Pick<Backend, 'getIcon'>, options: IconCacheOptions = DEFAULT_OPTIONS) {
    this.cache = new IconCache(
      (key, size) => backend.getIcon(key, size),
      (id, _key, _size, url) => {
        this.icons.set(id, url);
      },
      options,
    );
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

  /** Drop every entry (e.g. on a DPI change) so the next `ensureIcon` requests it again. */
  clear(): void {
    this.cache.clear();
    this.icons.clear();
  }
}
