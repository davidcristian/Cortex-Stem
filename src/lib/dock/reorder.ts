/**
 * Drag-to-reorder maths: pure functions so the pointer controller in
 * `Dock.svelte` only measures, calls these, and writes transforms.
 *
 * Positions are the *rest* centres of the pinned slots, in dock order. The
 * gap is opened with transforms, so the rest positions never move during a
 * drag and every comparison below is stable.
 */

/** A press becomes a drag after `holdMs` without release, or `movePx` of travel. */
export interface DragStartRule {
  holdMs: number;
  movePx: number;
}

export const DRAG_START: DragStartRule = { holdMs: 150, movePx: 6 };

export function exceedsMoveThreshold(dx: number, dy: number, movePx: number): boolean {
  return dx * dx + dy * dy >= movePx * movePx;
}

/**
 * Index the dragged item (`from`) would take when its centre is at `x`:
 * the number of *other* items whose rest centre lies left of it. Ranges
 * over `0..n-1` for `n` items, so every position is reachable.
 */
export function insertionIndex(x: number, centres: ArrayLike<number>, from: number): number {
  let index = 0;
  for (let i = 0; i < centres.length; i += 1) {
    if (i !== from && (centres[i] ?? 0) < x) {
      index += 1;
    }
  }
  return index;
}

/**
 * Horizontal shift of the item at `index` while `from` is lifted and the gap
 * sits at `to`: the items between the two move one slot towards `from`'s
 * old place. The lifted item itself and everything outside the span stay.
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
 * Whether a release at (`x`, `y`) counts as a drop: inside `bounds` grown by
 * `slackX` / `slackY` on each side. Outside means "put it back".
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
