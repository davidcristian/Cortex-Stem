import type { IconSources } from '$lib/dock/icon-size';

/**
 * The icon sizes that stay cached: the ones drawn now and the ones drawn
 * before the latest change (`DockIcons.iconUrlAt`'s fallback until the new
 * bitmaps arrive). A plain module: in a `.svelte.ts` file the lint
 * (`svelte/prefer-svelte-reactivity`) asks for a `SvelteSet`, and the store
 * only reads the set.
 */
export function retainedIconSizes(now: IconSources, previous: IconSources | null): Set<number> {
  const keep = [now.rest, now.magnified];
  if (previous !== null) {
    keep.push(previous.rest, previous.magnified);
  }
  return new Set(keep);
}
