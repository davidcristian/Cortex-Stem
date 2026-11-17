import { along } from '$lib/dock/edge';
import type { Viewport } from '$lib/dock/geometry';
import { slotCentres } from '$lib/dock/magnify';
import {
  gapShift,
  insertionIndex,
  moveItem,
  shiftTransform,
  type DropBounds,
} from '$lib/dock/reorder';
import type { DockItem as DockItemModel } from '$lib/ipc/generated/DockItem';

import type { DockController } from './controller.svelte';
import { DRAG_WINDOW_THICKNESS } from './layout';
import { viewportNow } from './view.svelte';

export interface DragSession {
  item: DockItemModel;
  el: HTMLElement;
  pointerId: number;
  startX: number;
  startY: number;
  lastX: number;
  lastY: number;
  started: boolean;
  frame: number;
  /**
   * Pinned app slots in dock order, with their keys and rest centres along
   * the dock's main axis, measured at start.
   */
  pinned: HTMLElement[];
  keys: string[];
  centres: Float64Array;
  bounds: DropBounds;
  from: number;
  to: number;
  /** The viewport the drag's coordinates refer to (the window grows during a drag). */
  viewport: Viewport;
}

// --- Drag to reorder pinned apps -------------------------------------------
// Imperative like the magnification controller: the lifted slot follows the
// pointer with an inline transform, the other pinned slots shift by one slot
// to open the gap, nothing reflows. The drop sends the new order to Rust;
// the snapshot that follows re-renders the items in that order and the
// slots effect of `Magnifier` clears the transforms in the same flush, so the
// picture never jumps.
export class Reorder {
  private readonly dock: DockController;

  drag: DragSession | null = null;
  /** The item rendered as lifted (`DockItem.lifted`). */
  liftedKey = $state<string | null>(null);
  /** Pinned keys expected from the snapshot after a drop; transforms stay until it arrives. */
  pendingOrder: string[] | null = null;
  pendingTimer: ReturnType<typeof setTimeout> | null = null;
  /** The hit region reported for this drag is the whole viewport; restore it when the drag is over. */
  dragHitReported = false;
  /**
   * The content size the window had before it grew for the drag, sent back
   * as soon as the drag is over (`clearDragStyles`) rather than after the
   * next measurement, which a pointer still over the panel would hold off.
   */
  sizeBeforeDrag: { width: number; height: number } | null = null;
  /**
   * The drag is over and the window has not been measured at its rest size
   * since (`measurePanel`): until then a pointer over the panel does not
   * hover (no magnification), so nothing holds off that measurement, which
   * brings the real hit region back for the shrunk window.
   */
  restoringAfterDrag = false;
  /** The viewport's thickness across the edge when the drag began, before the window grew. */
  acrossBeforeDrag = 0;

  constructor(dock: DockController) {
    this.dock = dock;
  }

  pinnedSlots(): HTMLElement[] {
    return this.dock.hover.slots.filter(
      (el) => el.dataset.kind === 'app' && el.dataset.pinned === 'true',
    );
  }

  startDrag(): void {
    const d = this.drag;
    if (d === null || d.started) {
      return;
    }
    d.started = true;
    this.restoringAfterDrag = false;
    this.dock.autoHide.send({ type: 'hold', hold: 'drag', active: true });
    // No magnification, tooltip or settle transition while dragging; the rest
    // positions measured below must be the unmagnified ones.
    this.dock.hover.leaveHoverState();
    this.clearDragStyles();
    // The drag finishes at the size it starts with.
    this.dock.fit.holdFit('reorder', true);
    try {
      d.el.setPointerCapture(d.pointerId);
    } catch {
      // The pointer is already gone; the window listeners still see the release.
    }
    d.pinned = this.pinnedSlots();
    d.keys = d.pinned.map((el) => el.dataset.key ?? '');
    d.from = d.pinned.indexOf(d.el);
    d.to = d.from;
    d.centres = new Float64Array(d.pinned.length);
    const bounds: DropBounds = {
      left: Infinity,
      right: -Infinity,
      top: Infinity,
      bottom: -Infinity,
    };
    const rects = d.pinned.map((el) => el.getBoundingClientRect());
    slotCentres(this.dock.view.axis, rects, d.centres);
    rects.forEach((rect) => {
      bounds.left = Math.min(bounds.left, rect.left);
      bounds.right = Math.max(bounds.right, rect.right);
      bounds.top = Math.min(bounds.top, rect.top);
      bounds.bottom = Math.max(bounds.bottom, rect.bottom);
    });
    d.bounds = bounds;
    this.liftedKey = d.item.key;
    const transition = this.dock.view.reducedMotion ? '' : 'transform 160ms ease';
    for (const el of d.pinned) {
      if (el !== d.el) {
        el.style.transition = transition;
      }
    }
    // The lifted icon may go anywhere on screen: the window grows away from
    // its edge to the work area's size for the duration of the drag (the
    // panel stays on the edge; `onwindowresize` shifts the drag's origin by
    // the growth on `bottom` and `right`) and all of it accepts clicks.
    d.viewport = viewportNow();
    this.acrossBeforeDrag = this.dock.view.vertical ? d.viewport.width : d.viewport.height;
    this.growWindowForDrag();
    this.dragHitReported = true;
    this.dock.reports.reportHitRegion();
    this.applyDrag();
  }

  /** The window's thickness (its height, or its width on a vertical dock) goes to the drag's. */
  growWindowForDrag(): void {
    if (this.dock.reports.reported.width === 0) {
      return;
    }
    this.sizeBeforeDrag = this.dock.reports.reported;
    this.dock.reports.reported = this.dock.view.vertical
      ? { width: DRAG_WINDOW_THICKNESS, height: this.dock.reports.reported.height }
      : { width: this.dock.reports.reported.width, height: DRAG_WINDOW_THICKNESS };
    void this.dock.backend
      .setDockContentSize(
        this.dock.reports.reported.width,
        this.dock.reports.reported.height,
        this.dock.view.edge,
      )
      .catch(this.dock.intents.report('set_dock_content_size', false));
  }

  scheduleDragFrame(): void {
    if (this.drag !== null && this.drag.frame === 0) {
      this.drag.frame = requestAnimationFrame(this.applyDrag);
    }
  }

  readonly applyDrag = (): void => {
    const d = this.drag;
    if (d === null || !d.started) {
      return;
    }
    d.frame = 0;
    const dx = d.lastX - d.startX;
    const dy = d.lastY - d.startY;
    d.el.style.transform = `translate(${dx}px, ${dy}px)`;
    const to = insertionIndex(
      (d.centres[d.from] ?? 0) + along(this.dock.view.axis, dx, dy),
      d.centres,
      d.from,
    );
    if (to === d.to) {
      return;
    }
    d.to = to;
    d.pinned.forEach((el, i) => {
      if (el === d.el) {
        return;
      }
      el.style.transform = shiftTransform(
        this.dock.view.axis,
        gapShift(i, d.from, to, this.dock.fit.slotPx),
      );
    });
  };

  drop(d: DragSession): void {
    const order = moveItem(d.keys, d.from, d.to);
    // Land in the gap now (no transition: the snapshot may arrive within a
    // frame, and the transforms are relative to the old DOM order).
    d.el.style.transform = shiftTransform(
      this.dock.view.axis,
      (d.centres[d.to] ?? 0) - (d.centres[d.from] ?? 0),
    );
    this.liftedKey = null;
    this.drag = null;
    // The window shrinks back now; the pointer may never "leave".
    this.dock.hover.hovering = false;
    this.restoringAfterDrag = true;
    this.dock.autoHide.endDragHold(d);
    this.pendingOrder = order;
    this.pendingTimer = setTimeout(this.clearDragStyles, 600);
    const ids = order.map((key) => key.slice('app:'.length));
    void this.dock.backend.reorderPinned(ids).catch((err: unknown) => {
      this.clearDragStyles();
      this.dock.intents.report('Reordering')(err);
    });
  }

  /** Put everything back; `animate` eases the lifted item home instead of snapping. */
  cancelDrag(animate: boolean): void {
    const d = this.drag;
    if (d === null) {
      return;
    }
    this.drag = null;
    if (d.frame !== 0) {
      cancelAnimationFrame(d.frame);
    }
    if (!d.started) {
      return;
    }
    this.dock.hover.hovering = false;
    this.restoringAfterDrag = true;
    this.dock.autoHide.endDragHold(d);
    try {
      d.el.releasePointerCapture(d.pointerId);
    } catch {
      // Already released by the browser on pointerup.
    }
    if (!animate || this.dock.view.reducedMotion) {
      this.clearDragStyles();
      return;
    }
    this.liftedKey = null;
    d.el.style.transition = 'transform 180ms ease';
    for (const el of d.pinned) {
      el.style.transform = '';
    }
    this.pendingOrder = null;
    this.pendingTimer = setTimeout(this.clearDragStyles, 220);
  }

  /** The drag is over and the DOM is in its final order: drop every inline style it set. */
  readonly clearDragStyles = (): void => {
    if (this.pendingTimer !== null) {
      clearTimeout(this.pendingTimer);
      this.pendingTimer = null;
    }
    this.pendingOrder = null;
    this.liftedKey = null;
    for (const el of this.dock.hover.slots) {
      el.style.transform = '';
      el.style.transition = '';
    }
    // No transform left from the drag: the fit may follow again.
    this.dock.fit.holdFit('reorder', false);
    if (this.dragHitReported) {
      this.dragHitReported = false;
      // The window goes back to the size it had before the drag at once: a
      // pointer that stays over the panel would otherwise hold off the
      // measurement that shrinks it, and the tall window would take clicks
      // above the dock until the pointer left.
      const rest = this.sizeBeforeDrag;
      this.sizeBeforeDrag = null;
      if (
        rest !== null &&
        (rest.width !== this.dock.reports.reported.width ||
          rest.height !== this.dock.reports.reported.height)
      ) {
        this.dock.reports.reported = rest;
        void this.dock.backend
          .setDockContentSize(rest.width, rest.height, this.dock.view.edge)
          .catch(this.dock.intents.report('set_dock_content_size', false));
      }
      // Back to the real hit region once the DOM has settled (next frame).
      this.dock.measure.scheduleMeasure();
    }
  };
}

export function sameOrder(a: readonly string[], b: readonly string[]): boolean {
  return a.length === b.length && a.every((key, i) => key === b[i]);
}
