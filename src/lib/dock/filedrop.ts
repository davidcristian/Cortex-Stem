/**
 * Pinning by dropping files on the dock: pure pieces, so the file
 * drag controller in `Dock.svelte` only measures, calls these, and renders
 * the gap.
 *
 * Which files can be pinned is Rust's decision (`pin_target`). The UI only
 * leaves out what cannot be a program, so a drag of documents makes no
 * pointless calls and opens no gap.
 */

import type { AppId } from '$lib/ipc/generated/AppId';
import type { DockItem } from '$lib/ipc/generated/DockItem';
import type { DockItemKind } from '$lib/ipc/generated/DockItemKind';
import type { LaunchTarget } from '$lib/ipc/generated/LaunchTarget';

import { insertionIndex } from './reorder';

/** The last component of a path; `/` and `\` both separate. */
export function fileNameOf(path: string): string {
  return path.split(/[/\\]/).pop() ?? path;
}

/**
 * What `pin_target` is asked to pin for a dropped path: a `.lnk` as a
 * shortcut, an `.exe` as a program (any letter case); `null` for anything
 * else (documents, folders, files without an extension).
 */
export function pinTargetFor(path: string): LaunchTarget | null {
  const name = fileNameOf(path);
  const dot = name.lastIndexOf('.');
  const extension = dot <= 0 ? '' : name.slice(dot + 1).toLowerCase();
  switch (extension) {
    case 'lnk':
      return { kind: 'shortcut', path };
    case 'exe':
      return { kind: 'exe', path, args: [], workingDir: null };
    default:
      return null;
  }
}

/** A dropped path `pin_target` will be asked about. */
export interface PinCandidate {
  path: string;
  target: LaunchTarget;
}

/** The paths of a drag that `pinTargetFor` accepts, in the drag's order. */
export function pinCandidates(paths: readonly string[]): PinCandidate[] {
  const candidates: PinCandidate[] = [];
  for (const path of paths) {
    const target = pinTargetFor(path);
    if (target !== null) {
      candidates.push({ path, target });
    }
  }
  return candidates;
}

/** The ids of the pinned app items, in dock order. */
export function pinnedIds(items: readonly DockItem[]): AppId[] {
  const ids: AppId[] = [];
  for (const item of items) {
    if (item.kind === 'app' && item.pinned && item.appId !== null) {
      ids.push(item.appId);
    }
  }
  return ids;
}

/**
 * Where among the pinned items a file dropped at `x` goes: the number of
 * them whose centre lies left of `x` (`0..=n` for `n` items; `reorder.ts`'s
 * `insertionIndex` with nothing lifted). `centres` are where the items are
 * drawn now, gap included: an open gap pushes its neighbours away from the
 * pointer, so the index under a pointer that does not move never flips.
 */
export function dropIndex(x: number, centres: ArrayLike<number>): number {
  return insertionIndex(x, centres, -1);
}

/**
 * The pinned order after a drop: `dropped` (the canonical ids `pin_target`
 * returned, each once, in drop order) at `index` of `pinned`, the order
 * before the drop. Ids that were pinned already move there instead of
 * appearing twice. `index` counts positions in `pinned` and is clamped to
 * `0..=pinned.length`.
 */
export function orderWithDropped(
  pinned: readonly AppId[],
  dropped: readonly AppId[],
  index: number,
): AppId[] {
  const moving = [...new Set(dropped)];
  const set = new Set(moving);
  const at = Math.min(pinned.length, Math.max(0, Math.round(index)));
  const keep = (id: AppId): boolean => !set.has(id);
  return [...pinned.slice(0, at).filter(keep), ...moving, ...pinned.slice(at).filter(keep)];
}

/** Where the drop gap is drawn: next to one item, or alone in an empty panel (`key: null`). */
export interface GapAnchor {
  key: string | null;
  side: 'before' | 'after';
}

export function sameAnchor(a: GapAnchor | null, b: GapAnchor | null): boolean {
  return a === b || (a !== null && b !== null && a.key === b.key && a.side === b.side);
}

const LEFT_FIXED: ReadonlySet<DockItemKind> = new Set(['launcher', 'taskView', 'showDesktop']);

/**
 * Where the gap for pinned position `index` goes among `items` (dock
 * order): before the pinned item now at `index`, or after the last pinned
 * item. With nothing pinned, where the pinned group would start: before the
 * first running app, else after the fixed items on the left, else before
 * the first item, else alone.
 */
export function gapAnchor(items: readonly DockItem[], index: number): GapAnchor {
  const pinned = items.filter((item) => item.kind === 'app' && item.pinned);
  const last = pinned.at(-1);
  if (last !== undefined) {
    const next = pinned[Math.max(0, Math.round(index))];
    return next !== undefined
      ? { key: next.key, side: 'before' }
      : { key: last.key, side: 'after' };
  }
  const firstApp = items.find((item) => item.kind === 'app');
  if (firstApp !== undefined) {
    return { key: firstApp.key, side: 'before' };
  }
  let left = 0;
  while (left < items.length && LEFT_FIXED.has(items[left]!.kind)) {
    left += 1;
  }
  if (left > 0) {
    return { key: items[left - 1]!.key, side: 'after' };
  }
  const first = items[0];
  return first !== undefined ? { key: first.key, side: 'before' } : { key: null, side: 'after' };
}

/** A dock item's horizontal extent where it is drawn now (viewport CSS px). */
export interface ItemSpan {
  key: string;
  left: number;
  right: number;
}

/** The item whose span contains `x` (left edge inclusive), or `null`: padding, a separator's margin, the gap. */
export function itemAt(x: number, spans: readonly ItemSpan[]): ItemSpan | null {
  return spans.find((span) => x >= span.left && x < span.right) ?? null;
}

/** Where a file drag points, as the dock measured it. */
export interface FileDragSpot {
  /** Over the panel itself, not the transparent margin around it. */
  onPanel: boolean;
  /** The item under the pointer (`itemAt`). */
  over: ItemSpan | null;
  /** Where the pin gap would open (`dropIndex`). */
  index: number;
}

/**
 * What a file drag does at a spot: nothing, or pin at `index`.
 *
 * Which item is under the pointer is kept apart from where the pin gap
 * would open because a drop on the Recycle Bin item is to delete to the bin
 * instead of pinning (a further action decided here from `over`); for now
 * every spot on the panel pins.
 *
 * `pinnable`: the drag carries at least one path `pinTargetFor` accepts.
 */
export type FileDropAction = { kind: 'none' } | { kind: 'pin'; index: number };

export function fileDropAction(spot: FileDragSpot, pinnable: boolean): FileDropAction {
  if (!spot.onPanel || !pinnable) {
    return { kind: 'none' };
  }
  return { kind: 'pin', index: spot.index };
}
