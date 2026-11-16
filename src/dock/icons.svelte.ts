import { untrack } from 'svelte';

import { iconSources, type IconSources } from '$lib/dock/icon-size';
import type { IconKey } from '$lib/ipc/generated/IconKey';

import type { DockController } from './controller.svelte';
import { retainedIconSizes } from './retained-icon-sizes';

/** The bitmaps the app icons are drawn from: their sizes, their loading and the cache. */
export class DockIcons {
  /** Set in the constructor; the `$derived` fields read it only later. */
  private readonly dock!: DockController;

  // Two bitmaps per app icon: exactly the rest size in device pixels,
  // drawn 1:1, and the ladder size that covers the true magnified box. One
  // request when they coincide (magnification off, reduced motion, the top
  // of the ladder).
  iconSizes = $derived(
    iconSources(this.dock.fit.iconSize, this.dock.view.magnify.maxScale, this.dock.view.dpr),
  );
  dualSources = $derived(this.iconSizes.magnified !== this.iconSizes.rest);
  /**
   * The sizes before the latest change of `iconSizes` (a scale change,
   * the icon size or the magnification settings). An item keeps
   * drawing the bitmap it has at those sizes until the one at the new sizes
   * has arrived (`iconUrlAt`), instead of showing the loading placeholder in
   * between. Taken in the effect's teardown: the sizes it captured are the
   * ones the items were drawn from.
   */
  previousIconSizes = $state.raw<IconSources | null>(null);

  constructor(dock: DockController) {
    this.dock = dock;
    $effect.pre(() => {
      const sizes = this.iconSizes;
      return () => {
        this.previousIconSizes = sizes;
      };
    });
    // --- Icons ------------------------------------------------------------------
    // The rest bitmaps first: they are what shows at startup; the magnified
    // ones can arrive behind them. The snapshot's items rather than the
    // rendered ones: the icons of apps a file drop is pinning load while the
    // gap is still on screen.
    $effect(() => {
      const all = this.dock.store.state?.items ?? [];
      for (const item of all) {
        if (item.iconKey !== null) {
          this.dock.store.ensureIcon(item.iconKey, this.iconSizes.rest);
        }
      }
      if (this.dualSources) {
        for (const item of all) {
          if (item.iconKey !== null) {
            this.dock.store.ensureIcon(item.iconKey, this.iconSizes.magnified);
          }
        }
      }
      // Only the sizes drawn now and the ones drawn before the latest change
      // (`iconUrlAt`'s fallback until the new bitmaps arrive) stay cached:
      // the rest size is exact device pixels, so every icon size or display
      // scale would otherwise keep one more bitmap per app for good.
      const previous = untrack(() => this.previousIconSizes);
      this.dock.store.retainIconSizes(retainedIconSizes(this.iconSizes, previous));
    });
  }

  /** The item's bitmap at `size`; while it loads, the one at `fallback` when there is one. */
  iconUrlAt(key: IconKey, size: number, fallback: number | undefined): string | null | undefined {
    const url = this.dock.store.icon(key, size);
    if (url !== undefined || fallback === undefined || fallback === size) {
      return url;
    }
    return this.dock.store.icon(key, fallback);
  }
}
