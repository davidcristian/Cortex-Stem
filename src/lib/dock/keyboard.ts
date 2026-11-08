/**
 * Keyboard navigation along the dock's toolbar: pure functions, so
 * `Dock.svelte` only finds the focused item, calls these and moves the focus.
 *
 * The toolbar is one tab stop (a roving tabindex): exactly one item has
 * `tabindex="0"`, the one the focus was on last (`tabStopKey`), the others
 * `-1`. ArrowRight and ArrowDown go to the next item, ArrowLeft and ArrowUp
 * to the previous one (the vertical pair is there for a dock on a side edge
 * later), Home and End to the first and the last; separators are skipped.
 * The focus stops at either end instead of wrapping around, so Home and End
 * and a held arrow all land in the same place.
 */

import type { DockItemKind } from '$lib/ipc/generated/DockItemKind';

/** What these functions need of a `DockItem`. */
export interface NavigableItem {
  readonly key: string;
  readonly kind: DockItemKind;
}

/** The keys that move the focus along the toolbar. */
export type NavigationKey = 'ArrowLeft' | 'ArrowRight' | 'ArrowUp' | 'ArrowDown' | 'Home' | 'End';

const NAVIGATION_KEYS: ReadonlySet<string> = new Set<NavigationKey>([
  'ArrowLeft',
  'ArrowRight',
  'ArrowUp',
  'ArrowDown',
  'Home',
  'End',
]);

/** `key` (a `KeyboardEvent.key`) moves the focus along the toolbar. */
export function isNavigationKey(key: string): key is NavigationKey {
  return NAVIGATION_KEYS.has(key);
}

/** Every item but a separator takes keyboard focus. */
export function isFocusable(item: Pick<NavigableItem, 'kind'>): boolean {
  return item.kind !== 'separator';
}

/** The first focusable index from `from` in steps of `dir` (±1), or `null`. */
function seek(
  items: readonly Pick<NavigableItem, 'kind'>[],
  from: number,
  dir: 1 | -1,
): number | null {
  for (let i = from; i >= 0 && i < items.length; i += dir) {
    if (isFocusable(items[i]!)) {
      return i;
    }
  }
  return null;
}

/**
 * The index of the item `key` moves the focus to from the item at
 * `current`, or `null` when it moves nothing: `key` is not a navigation key,
 * or no item takes focus. At an end the focus stays where it is (`current`).
 * A `current` outside the list (the panel itself has the focus, or the
 * focused item has just gone) counts as before the first item for the
 * forward keys and after the last for the backward ones.
 */
export function nextFocusIndex(
  items: readonly Pick<NavigableItem, 'kind'>[],
  current: number,
  key: string,
): number | null {
  if (!isNavigationKey(key)) {
    return null;
  }
  const last = items.length - 1;
  const inList = Number.isInteger(current) && current >= 0 && current <= last;
  switch (key) {
    case 'Home':
      return seek(items, 0, 1);
    case 'End':
      return seek(items, last, -1);
    case 'ArrowRight':
    case 'ArrowDown':
      if (!inList) {
        return seek(items, 0, 1);
      }
      return seek(items, current + 1, 1) ?? seek(items, current, -1);
    case 'ArrowLeft':
    case 'ArrowUp':
      if (!inList) {
        return seek(items, last, -1);
      }
      return seek(items, current - 1, -1) ?? seek(items, current, 1);
  }
}

/**
 * The key of the item that is the toolbar's tab stop: `remembered` (the
 * item the focus was on last) while it is still a focusable item of
 * `items`, otherwise the first focusable item; `null` when there is none.
 */
export function tabStopKey(
  items: readonly NavigableItem[],
  remembered: string | null,
): string | null {
  if (remembered !== null && items.some((i) => i.key === remembered && isFocusable(i))) {
    return remembered;
  }
  return items.find(isFocusable)?.key ?? null;
}

/**
 * Where the focus goes when the item `key` has left the dock while it had
 * the focus (its app's last window closed, say): the first item after it
 * in `before` (the list as it was) that is still in `after` and takes focus,
 * else the nearest such item before it, else the first focusable item of
 * `after` (the old list said nothing useful); `null` when `after` has none.
 */
export function successorKey(
  before: readonly NavigableItem[],
  after: readonly NavigableItem[],
  key: string,
): string | null {
  const remaining = new Set(after.filter(isFocusable).map((i) => i.key));
  const at = before.findIndex((i) => i.key === key);
  if (at >= 0) {
    for (let i = at + 1; i < before.length; i += 1) {
      if (remaining.has(before[i]!.key)) {
        return before[i]!.key;
      }
    }
    for (let i = at - 1; i >= 0; i -= 1) {
      if (remaining.has(before[i]!.key)) {
        return before[i]!.key;
      }
    }
  }
  return tabStopKey(after, null);
}
