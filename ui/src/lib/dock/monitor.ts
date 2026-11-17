/**
 * Which monitor a dock page belongs to, and what follows from it.
 *
 * With `monitors = all` the app opens one dock window per monitor: the
 * primary's page is `dock.html` and the others are `dock.html?monitor=<id>`
 * (docs/ipc/windows.md). A page needs its monitor for two things, both pure
 * and decided here:
 *
 * - its own facts in `DockState.docks` (`dockFactsFor`): whether a
 *   fullscreen app owns *its* monitor and whether a window covers *its*
 *   panel; a dock with no entry yet takes both as `false`;
 * - with `behavior.showOnlyCurrentMonitor` on, the item list narrowed to
 *   the windows on its monitor (`itemsForMonitor`). Ordering, grouping and
 *   click semantics stay Rust's: the filter only drops windows and the
 *   items that are left with none and are neither pinned nor fixed, and
 *   the separators that no longer separate two non-empty groups.
 */

import type { DockFacts } from '$lib/ipc/generated/DockFacts';
import type { DockItem } from '$lib/ipc/generated/DockItem';
import type { MonitorId } from '$lib/ipc/generated/MonitorId';
import type { MonitorInfo } from '$lib/ipc/generated/MonitorInfo';

/** The `monitor` query parameter of a dock page's URL, decoded; `null` when absent or empty. */
export function monitorFromSearch(search: string): MonitorId | null {
  const value = new URLSearchParams(search).get('monitor');
  return value === null || value === '' ? null : value;
}

/** The primary monitor's id (the flagged one, else the first), as the app layer picks it; `null` with no monitors. */
export function primaryMonitorId(monitors: readonly MonitorInfo[]): MonitorId | null {
  return (monitors.find((m) => m.isPrimary) ?? monitors[0])?.id ?? null;
}

/**
 * The monitor this dock page sits on: the one its URL names, else (the
 * primary's page) the primary monitor of `monitors`; `null` until the
 * monitors are known.
 */
export function dockMonitor(
  urlMonitor: MonitorId | null,
  monitors: readonly MonitorInfo[],
): MonitorId | null {
  return urlMonitor ?? primaryMonitorId(monitors);
}

/** `DockFacts` without the monitor: what the auto-hide machine takes. */
export interface OwnFacts {
  fullscreen: boolean;
  overlapped: boolean;
}

export const NO_FACTS: OwnFacts = { fullscreen: false, overlapped: false };

/**
 * This dock's entry in `DockState.docks`, by its monitor; both facts `false`
 * without one (before the engine knows the dock, or its monitor). A page
 * that cannot tell its monitor yet (`null`) takes the only entry there is,
 * if there is exactly one: the single-dock case stays right even before
 * the monitor list arrived.
 */
export function dockFactsFor(docks: readonly DockFacts[], monitor: MonitorId | null): OwnFacts {
  const own =
    monitor === null
      ? docks.length === 1
        ? docks[0]
        : undefined
      : docks.find((d) => d.monitor === monitor);
  return own === undefined ? NO_FACTS : { fullscreen: own.fullscreen, overlapped: own.overlapped };
}

/**
 * The items a dock on `monitor` shows while `behavior.showOnlyCurrentMonitor`
 * is on (`enabled`): each app item keeps only the windows on that monitor;
 * an item left with none stays only if it is pinned (or is not an app at
 * all); `isActive` and `needsAttention` follow the remaining windows (each
 * `WindowInfo` carries both flags), so an app flashing on another
 * monitor does not wiggle here. Separators that would then lead, trail or
 * double up are dropped. Off, or with the monitor unknown, the list is
 * returned as is.
 */
export function itemsForMonitor(
  items: readonly DockItem[],
  monitor: MonitorId | null,
  enabled: boolean,
): DockItem[] {
  if (!enabled || monitor === null) {
    return [...items];
  }
  const kept: DockItem[] = [];
  for (const item of items) {
    if (item.kind !== 'app') {
      kept.push(item);
      continue;
    }
    const windows = item.windows.filter((w) => w.monitor === monitor);
    if (windows.length === 0 && !item.pinned) {
      continue;
    }
    if (windows.length === item.windows.length) {
      kept.push(item);
      continue;
    }
    kept.push({
      ...item,
      windows,
      isActive: windows.some((w) => w.isActive),
      needsAttention: windows.some((w) => w.needsAttention),
    });
  }
  return collapseSeparators(kept);
}

/** Drop separators that lead, trail, or follow another separator. */
function collapseSeparators(items: readonly DockItem[]): DockItem[] {
  const out: DockItem[] = [];
  for (const item of items) {
    if (item.kind === 'separator') {
      const last = out.at(-1);
      if (last === undefined || last.kind === 'separator') {
        continue;
      }
    }
    out.push(item);
  }
  while (out.at(-1)?.kind === 'separator') {
    out.pop();
  }
  return out;
}
