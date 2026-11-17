/**
 * Drag-to-reorder maths: pure functions so the pointer controller in
 * `reorder.svelte.ts` and `reorder-pointer.svelte.ts` only measures, calls
 * these, and writes transforms.
 *
 * Positions are the *rest* centres of the pinned slots, in dock order, along
 * the dock's main axis (`edge.ts`: x on a horizontal dock, y on a
 * vertical one). The gap is opened with transforms, so the rest positions
 * never move during a drag and every comparison below is stable.
 */

import type { DockAxis } from './edge';

/**
 * A press becomes a drag after `movePx` of travel, never by holding still:
 * a still press of any length is a click (a mouse click often lasts 100 to
 * 200 ms, a touchpad's longer).
 */
export interface DragStartRule {
  movePx: number;
}

export const DRAG_START: DragStartRule = { movePx: 6 };

export function exceedsMoveThreshold(dx: number, dy: number, movePx: number): boolean {
  return dx * dx + dy * dy >= movePx * movePx;
}

/**
 * Index the dragged item (`from`) would take when its centre is at `pos`
 * along the main axis: the number of *other* items whose rest centre lies
 * before it (left of it, or above it on a vertical dock). Ranges over
 * `0..n-1` for `n` items, so every position is reachable. With `from = -1`
 * nothing is lifted (a file dropped from outside the dock, `filedrop.ts`)
 * and it ranges over `0..=n`.
 */
export function insertionIndex(pos: number, centres: ArrayLike<number>, from: number): number {
  let index = 0;
  for (let i = 0; i < centres.length; i += 1) {
    if (i !== from && (centres[i] ?? 0) < pos) {
      index += 1;
    }
  }
  return index;
}

/**
 * Shift along the main axis of the item at `index` while `from` is lifted
 * and the gap sits at `to`: the items between the two move one slot towards
 * `from`'s old place. The lifted item itself and everything outside the
 * span stay.
 */
export function gapShift(index: number, from: number, to: number, slotPx: number): number {
  if (index === from) {
    return 0;
  }
  if (from < to && index > from && index <= to) {
    return -slotPx;
  }
  if (to < from && index >= to && index < from) {
    return slotPx;
  }
  return 0;
}

/**
 * The inline transform that shifts a slot by `px` along `axis`
 * (`translateX` on a horizontal dock, `translateY` on a vertical one); no
 * transform at all for no shift.
 */
export function shiftTransform(axis: DockAxis, px: number): string {
  if (px === 0) {
    return '';
  }
  return axis === 'horizontal' ? `translateX(${px}px)` : `translateY(${px}px)`;
}

/** The list with the item at `from` moved to `to`. Returns a copy. */
export function moveItem<T>(list: readonly T[], from: number, to: number): T[] {
  const next = [...list];
  if (from < 0 || from >= next.length || to < 0 || to >= next.length || from === to) {
    return next;
  }
  const [item] = next.splice(from, 1);
  next.splice(to, 0, item as T);
  return next;
}

/** Axis-aligned box in CSS pixels (viewport coordinates). */
export interface DropBounds {
  left: number;
  right: number;
  top: number;
  bottom: number;
}

/**
 * The slack of `isInsideWithSlack` for a dock whose main axis is `axis`:
 * `alongPx` at the row's ends, `acrossPx` towards and away from the edge.
 */
export function dropSlack(
  axis: DockAxis,
  alongPx: number,
  acrossPx: number,
): { x: number; y: number } {
  return axis === 'horizontal' ? { x: alongPx, y: acrossPx } : { x: acrossPx, y: alongPx };
}

/**
 * Whether a release at (`x`, `y`) counts as a drop: inside `bounds` grown by
 * `slackX` / `slackY` on each side (`dropSlack`). Outside means "put it back".
 */
export function isInsideWithSlack(
  x: number,
  y: number,
  bounds: DropBounds,
  slackX: number,
  slackY: number,
): boolean {
  return (
    x >= bounds.left - slackX &&
    x <= bounds.right + slackX &&
    y >= bounds.top - slackY &&
    y <= bounds.bottom + slackY
  );
}
