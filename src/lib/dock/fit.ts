/**
 * Fitting the dock to its edge (docs/ipc/sizing-protocol.md §Fitting
 * the edge). Pure, so the page asks one function for the icon size it
 * draws at and the maths can be tested without a DOM.
 *
 * A dock with many items on a short edge (a left dock on a 1080p screen
 * with every fixed item on, a small monitor) can be longer than its
 * window. Like the macOS dock the page then draws it smaller instead of
 * cutting off its ends: the **fitted size** is `Settings.iconSize` when the
 * panel at rest, plus the most magnification can add to its length
 * (`maxGrowth`, the bound the page's reports already use), fits in the
 * viewport's length along the edge with `FIT_END_MARGIN` to spare at each
 * end; otherwise the largest whole CSS px size, not below
 * `MIN_FITTED_ICON`, for which that holds. At the minimum the panel may
 * still overflow.
 *
 * The answer depends only on the item counts, the layout constants, the
 * magnification settings and the viewport's length, never on a measured
 * panel, so drawing at the fitted size cannot change what it is (no layout
 * loop).
 */

import type { DockItemKind } from '$lib/ipc/generated/DockItemKind';
import type { MonitorInfo } from '$lib/ipc/generated/MonitorInfo';

import { across, along, type DockAxis } from './edge';
import { maxGrowth, type MagnifyParams } from './magnify';

/** The smallest size the fit goes down to (CSS px). */
export const MIN_FITTED_ICON = 16;

/** What the fitted panel leaves free at each end of the viewport's length (CSS px). */
export const FIT_END_MARGIN = 8;

/**
 * How long the page waits for its window to be moved after a change of
 * `position` to the other axis (ms, `awaitingMove`). Rust moves it right
 * after `dock://settings` (docs/ipc/sizing-protocol.md, step 5); when no
 * move comes in this time (`MockBackend`, which has no window to move, or a
 * move that failed), the page takes the window as shaped for the axis it
 * has.
 */
export const MOVE_WAIT_MS = 1000;

/** A width and a height (CSS px). */
export interface Extent {
  width: number;
  height: number;
}

/** What the fit's room is made from (`fitRoom`). */
export interface RoomInput {
  /** The axis the page lays out for (`Settings.position`). */
  axis: DockAxis;
  /**
   * The axis the window is shaped for: the page's at its first settings,
   * at a `resize` that leaves the window longer along the page's axis than
   * across it, and when `MOVE_WAIT_MS` ran out; `null` before the first
   * settings.
   */
  shapedAxis: DockAxis | null;
  /** The viewport (CSS px). */
  viewport: Extent;
  /** This dock's monitor's work area in CSS px (`workAreaCss`); `null` without a monitor list. */
  workArea: Extent | null;
}

/**
 * The page has switched to the other axis and its window is still shaped
 * for the old one: longer across the new axis than along it. Rust moves it
 * to the new edge after `dock://settings` (docs/ipc/sizing-protocol.md,
 * step 5), so until then the viewport's length along the new axis is the
 * old window's thickness, not the room the dock will have.
 */
export function awaitingMove({ axis, shapedAxis, viewport }: RoomInput): boolean {
  if (shapedAxis === null || shapedAxis === axis) {
    return false;
  }
  return (
    along(axis, viewport.width, viewport.height) < across(axis, viewport.width, viewport.height)
  );
}

/**
 * The fit's room: the length along the edge that the dock's window has, or
 * is about to have. That is the viewport's length along the edge, except
 * while `awaitingMove`: then it is the work area's length along the new
 * edge, the length Rust gives a dock window on any edge
 * (docs/ipc/sizing-protocol.md, steps 1 and 3), so the first reports for
 * the new edge are made at the size the window will fit, in either
 * direction (bottom → left and left → bottom). Without a monitor list
 * nothing better is known than the viewport.
 */
export function fitRoom(input: RoomInput): number {
  const { axis, viewport, workArea } = input;
  const alongNow = along(axis, viewport.width, viewport.height);
  if (!awaitingMove(input) || workArea === null) {
    return alongNow;
  }
  const alongWorkArea = along(axis, workArea.width, workArea.height);
  return alongWorkArea > 0 ? alongWorkArea : alongNow;
}

/** `monitor`'s work area in CSS px (its rect is in physical px at its `scaleFactor`); `null` without one. */
export function workAreaCss(monitor: MonitorInfo | null | undefined): Extent | null {
  if (monitor === null || monitor === undefined) {
    return null;
  }
  const scale = monitor.scaleFactor > 0 ? monitor.scaleFactor : 1;
  return { width: monitor.workArea.width / scale, height: monitor.workArea.height / scale };
}

/** The page's layout constants along the main axis (CSS px; Dock.svelte). */
export interface PanelLayout {
  /** Added to the icon's size in every item's slot. */
  slotGap: number;
  /** The panel's padding at each end. */
  panelPadding: number;
  /** One separator's length along the dock, its margins included. */
  separatorLength: number;
}

/** How many slots of each sort the panel lays out. */
export interface SlotCounts {
  /** Items with an icon (everything but separators). */
  icons: number;
  separators: number;
}

/** Count the slots of `items` (`DockState.items`, as drawn). */
export function slotCounts(items: readonly { readonly kind: DockItemKind }[]): SlotCounts {
  let separators = 0;
  for (const item of items) {
    if (item.kind === 'separator') {
      separators += 1;
    }
  }
  return { icons: items.length - separators, separators };
}

/**
 * The panel's length along the dock at `iconSize`: at rest (the padding,
 * the slots and the separators) plus the most magnification can add.
 */
export function fittedPanelLength(
  iconSize: number,
  counts: SlotCounts,
  layout: PanelLayout,
  magnify: MagnifyParams,
): number {
  const slot = iconSize + layout.slotGap;
  const rest =
    2 * layout.panelPadding + counts.icons * slot + counts.separators * layout.separatorLength;
  return rest + maxGrowth(slot, magnify);
}

export interface FitInput {
  /** `Settings.iconSize`: the user's size, the largest the fit gives. */
  iconSize: number;
  counts: SlotCounts;
  layout: PanelLayout;
  magnify: MagnifyParams;
  /** The viewport's length along the edge (its width on `bottom` and `top`, its height on a side). */
  viewportLength: number;
}

/**
 * The fitted size: `iconSize` when it fits, else the largest whole size
 * below it that does, else `MIN_FITTED_ICON` (or `iconSize`, should that
 * be smaller still). A viewport whose length is unknown (zero, negative,
 * not a number) fits anything: nothing is known to be cut off.
 */
export function fitIconSize({
  iconSize,
  counts,
  layout,
  magnify,
  viewportLength,
}: FitInput): number {
  if (!(viewportLength > 0) || !Number.isFinite(viewportLength) || !(iconSize > 0)) {
    return iconSize;
  }
  const room = viewportLength - 2 * FIT_END_MARGIN;
  // A hair of tolerance: `maxScale` arrives as an f32's decimal, and a
  // size that fits exactly must not lose a pixel to rounding noise.
  const fits = (size: number): boolean =>
    fittedPanelLength(size, counts, layout, magnify) <= room + 1e-6;
  if (fits(iconSize)) {
    return iconSize;
  }
  // The length grows with the size, so the first whole size that fits,
  // counting down, is the largest; at most ~110 steps, and only when an
  // input changed.
  for (let size = Math.ceil(iconSize) - 1; size > MIN_FITTED_ICON; size -= 1) {
    if (fits(size)) {
      return size;
    }
  }
  return Math.min(iconSize, MIN_FITTED_ICON);
}
