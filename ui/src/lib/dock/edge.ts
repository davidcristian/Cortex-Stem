/**
 * The screen edge the dock sits on and its two axes
 * (docs/ipc/sizing-protocol.md, "Layout on each edge"). Pure, so every part
 * of the dock that measures or places something along it (magnification,
 * reorder, file drops, the keyboard, auto-hide, the hit region) asks the
 * same questions the same way.
 *
 * `Settings.position` names the edge. The **main axis** runs along it: x on
 * `bottom` and `top` (a horizontal dock), y on `left` and `right` (a
 * vertical one). Items keep their order along it, first at its start (left
 * or top). The **cross axis** runs away from the edge: icons grow, title
 * pills sit and the native item menu opens on the side away from it; the
 * running dots sit between the icon and it.
 */

import type { DockPosition } from '$lib/ipc/generated/DockPosition';

/** The edge (`Settings.position`). */
export type DockEdge = DockPosition;

export const DOCK_EDGES: readonly DockEdge[] = ['bottom', 'top', 'left', 'right'];

/** `value` names an edge (a query parameter, say). */
export function isDockEdge(value: string | null | undefined): value is DockEdge {
  return typeof value === 'string' && (DOCK_EDGES as readonly string[]).includes(value);
}

/** The dock's main axis: along its edge. */
export type DockAxis = 'horizontal' | 'vertical';

export function axisOf(edge: DockEdge): DockAxis {
  return edge === 'left' || edge === 'right' ? 'vertical' : 'horizontal';
}

/** The coordinate of (`x`, `y`) along the main axis. */
export function along(axis: DockAxis, x: number, y: number): number {
  return axis === 'horizontal' ? x : y;
}

/** The coordinate of (`x`, `y`) across the main axis (towards or away from the edge). */
export function across(axis: DockAxis, x: number, y: number): number {
  return axis === 'horizontal' ? y : x;
}

/** A box as `getBoundingClientRect` gives it (a `DOMRect` is one). */
export interface Box {
  readonly left: number;
  readonly top: number;
  readonly width: number;
  readonly height: number;
}

/** Where a box starts and ends along the main axis. */
export interface Span {
  start: number;
  end: number;
}

export function spanAlong(axis: DockAxis, box: Box): Span {
  return axis === 'horizontal'
    ? { start: box.left, end: box.left + box.width }
    : { start: box.top, end: box.top + box.height };
}

export function centreAlong(axis: DockAxis, box: Box): number {
  return axis === 'horizontal' ? box.left + box.width / 2 : box.top + box.height / 2;
}

/**
 * The direction away from the edge, as a sign on the cross axis: `1` where
 * that is towards larger coordinates (`top`: down, `left`: right), `-1`
 * where it is towards smaller ones (`bottom`: up, `right`: left).
 */
export function awaySign(edge: DockEdge): 1 | -1 {
  return edge === 'top' || edge === 'left' ? 1 : -1;
}

/**
 * A CSS transform that moves something `percent` % of its own size away
 * from the edge (`translateY(-42%)` on `bottom`, `translateX(42%)` on
 * `left`): the launch bounce's keyframes on any edge.
 */
export function awayTranslate(edge: DockEdge, percent: number): string {
  const value = percent === 0 ? 0 : awaySign(edge) * percent;
  return axisOf(edge) === 'horizontal' ? `translateY(${value}%)` : `translateX(${value}%)`;
}

/**
 * Where the native item menu opens for a keyboard request (Shift+F10, the
 * menu key), CSS px of the viewport: the middle of the item's side
 * away from the edge, so a menu that has to open away from the screen edge
 * sits beside the icon instead of over it (above it on `bottom`, below on
 * `top`, right of it on `left`, left of it on `right`).
 */
export function menuAnchor(edge: DockEdge, box: Box): { x: number; y: number } {
  switch (edge) {
    case 'bottom':
      return { x: box.left + box.width / 2, y: box.top };
    case 'top':
      return { x: box.left + box.width / 2, y: box.top + box.height };
    case 'left':
      return { x: box.left + box.width, y: box.top + box.height / 2 };
    case 'right':
      return { x: box.left, y: box.top + box.height / 2 };
  }
}
