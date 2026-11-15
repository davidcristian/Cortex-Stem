/**
 * Keyboard navigation along the dock's toolbar: pure functions, so
 * `Dock.svelte` only finds the focused item, calls these and moves the focus.
 *
 * The toolbar is one tab stop (a roving tabindex): exactly one item has
 * `tabindex="0"`, the one the focus was on last (`tabStopKey`), the others
 * `-1`. The arrows of the dock's main axis move along it (the
 * toolbar's `aria-orientation` says which): ArrowRight and ArrowLeft on a
 * horizontal dock, ArrowDown and ArrowUp on a vertical one, to the next and
 * the previous item; the other pair does nothing. Home and End go to the
 * first and the last item; separators are skipped. The focus stops at
 * either end instead of wrapping around, so Home and End and a held arrow
 * all land in the same place.
 *
 * The focus shortcut brings the keyboard in from another app: the
 * page then focuses `entryKey`, the active app's item, else the first.
 */

import type { DockItemKind } from '$lib/ipc/generated/DockItemKind';

import type { DockAxis } from './edge';

/** What these functions need of a `DockItem`. */
export interface NavigableItem {
  readonly key: string;
  readonly kind: DockItemKind;
}

/** The keys that can move the focus along the toolbar. */
export type NavigationKey = 'ArrowLeft' | 'ArrowRight' | 'ArrowUp' | 'ArrowDown' | 'Home' | 'End';

/** The arrow to the next item and the one to the previous item along `axis`. */
function arrowsOf(axis: DockAxis): { next: NavigationKey; previous: NavigationKey } {
  return axis === 'horizontal'
    ? { next: 'ArrowRight', previous: 'ArrowLeft' }
    : { next: 'ArrowDown', previous: 'ArrowUp' };
}

/** `key` (a `KeyboardEvent.key`) moves the focus along a toolbar whose orientation is `axis`. */
export function isNavigationKey(key: string, axis: DockAxis): key is NavigationKey {
  const { next, previous } = arrowsOf(axis);
  return key === next || key === previous || key === 'Home' || key === 'End';
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
 * `current` on a toolbar whose orientation is `axis`, or `null` when it
 * moves nothing: `key` is not a navigation key there (an arrow across the
 * axis is not), or no item takes focus. At an end the focus stays where it
 * is (`current`).
 * A `current` outside the list (the panel itself has the focus, or the
 * focused item has just gone) counts as before the first item for the
 * forward keys and after the last for the backward ones.
 */
export function nextFocusIndex(
  items: readonly Pick<NavigableItem, 'kind'>[],
  current: number,
  key: string,
  axis: DockAxis,
): number | null {
  if (!isNavigationKey(key, axis)) {
    return null;
  }
  const last = items.length - 1;
  const inList = Number.isInteger(current) && current >= 0 && current <= last;
  if (key === 'Home') {
    return seek(items, 0, 1);
  }
  if (key === 'End') {
    return seek(items, last, -1);
  }
  if (key === arrowsOf(axis).next) {
    if (!inList) {
      return seek(items, 0, 1);
    }
    return seek(items, current + 1, 1) ?? seek(items, current, -1);
  }
  if (!inList) {
    return seek(items, last, -1);
  }
  return seek(items, current - 1, -1) ?? seek(items, current, 1);
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

/** What `entryKey` needs of a `DockItem`. */
export interface EntryItem extends NavigableItem {
  readonly isActive: boolean;
}

/**
 * The item the focus shortcut puts the keyboard on
 * (docs/ipc/keyboard-entry.md): the active app's item (the app whose
 * window was in front at the press) when this dock shows it, else the
 * first item that takes focus; `null` when none does.
 */
export function entryKey(items: readonly EntryItem[]): string | null {
  const active = items.find((i) => i.kind === 'app' && i.isActive);
  return active?.key ?? tabStopKey(items, null);
}

/** What `isBrowserShortcut` reads of a `KeyboardEvent`. */
export type ShortcutKeys = Pick<
  KeyboardEvent,
  'key' | 'ctrlKey' | 'altKey' | 'metaKey' | 'shiftKey'
>;

/** Keys that mean a browser command whatever the modifiers: find next, reload, caret browsing. */
const BROWSER_FUNCTION_KEYS: ReadonlySet<string> = new Set([
  'F3',
  'F5',
  'F7',
  'BrowserBack',
  'BrowserForward',
  'BrowserRefresh',
]);
/** Ctrl+<letter>: reload, find, find next, print, save, view source, history, downloads, open. */
const BROWSER_CTRL_LETTERS: ReadonlySet<string> = new Set([
  'r',
  'f',
  'g',
  'p',
  's',
  'u',
  'h',
  'j',
  'o',
]);

/**
 * Whether `e` is one of WebView2's browser shortcuts, which mean nothing
 * in a dock and would reload the page (F5, Ctrl+R), print it (Ctrl+P),
 * open a find bar (Ctrl+F), save its HTML (Ctrl+S) or go back (Alt+Left)
 * while the focus shortcut gives the dock the keyboard. The page
 * cancels them on keydown, which keeps WebView2 from acting on them. The
 * developer tools' shortcuts (F12, Ctrl+Shift+I) are left alone: release
 * builds have no developer tools. AltGr (Ctrl+Alt) never counts, so it
 * still types.
 */
export function isBrowserShortcut(e: ShortcutKeys): boolean {
  if (BROWSER_FUNCTION_KEYS.has(e.key)) {
    return true;
  }
  const key = e.key.length === 1 ? e.key.toLowerCase() : e.key;
  if (e.altKey && !e.ctrlKey && !e.metaKey && (key === 'ArrowLeft' || key === 'ArrowRight')) {
    return true;
  }
  const ctrlOnly = e.ctrlKey && !e.altKey && !e.metaKey;
  return ctrlOnly && (e.shiftKey ? key === 'r' : BROWSER_CTRL_LETTERS.has(key));
}
