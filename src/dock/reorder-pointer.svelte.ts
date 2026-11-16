import { DRAG_START, dropSlack, exceedsMoveThreshold, isInsideWithSlack } from '$lib/dock/reorder';
import type { DockItem as DockItemModel } from '$lib/ipc/generated/DockItem';

import type { DockController } from './controller.svelte';
import { DROP_SLACK_ICONS } from './layout';
import { viewportNow } from './view.svelte';

/** The pointer events of a drag to reorder: the press, the moves past the threshold, the release. */
export class ReorderPointer {
  private readonly dock: DockController;

  constructor(dock: DockController) {
    this.dock = dock;
  }

  /**
   * A left press on a pinned app may become a drag: only once the pointer
   * has travelled `DRAG_START.movePx` (`onwindowpointermove`), never by
   * holding still, so a still press of any length is a click (handled by
   * the item on release).
   */
  readonly onpress = (item: DockItemModel, e: PointerEvent): void => {
    if (e.button !== 0 || item.kind !== 'app' || !item.pinned) {
      return;
    }
    // A stale session (a lost pointerup) must not block the next press.
    this.dock.reorder.cancelDrag(false);
    const el = this.dock.hover.slots.find((s) => s.dataset.key === item.key);
    if (!el) {
      return;
    }
    this.dock.reorder.drag = {
      item,
      el,
      pointerId: e.pointerId,
      startX: e.clientX,
      startY: e.clientY,
      lastX: e.clientX,
      lastY: e.clientY,
      started: false,
      frame: 0,
      pinned: [],
      keys: [],
      centres: new Float64Array(0),
      bounds: { left: 0, right: 0, top: 0, bottom: 0 },
      from: -1,
      to: -1,
      viewport: viewportNow(),
    };
  };

  readonly onwindowpointermove = (e: PointerEvent): void => {
    const d = this.dock.reorder.drag;
    if (d === null || e.pointerId !== d.pointerId) {
      return;
    }
    d.lastX = e.clientX;
    d.lastY = e.clientY;
    if (!d.started) {
      if (exceedsMoveThreshold(e.clientX - d.startX, e.clientY - d.startY, DRAG_START.movePx)) {
        this.dock.reorder.startDrag();
      }
      return;
    }
    this.dock.reorder.scheduleDragFrame();
  };

  readonly onwindowpointerup = (e: PointerEvent): void => {
    const d = this.dock.reorder.drag;
    if (d === null || e.pointerId !== d.pointerId) {
      return;
    }
    if (!d.started) {
      // Released before the drag began: an ordinary click, handled by the item.
      this.dock.reorder.cancelDrag(false);
      return;
    }
    d.lastX = e.clientX;
    d.lastY = e.clientY;
    if (d.frame !== 0) {
      cancelAnimationFrame(d.frame);
      d.frame = 0;
    }
    this.dock.reorder.applyDrag();
    const slack = dropSlack(
      this.dock.view.axis,
      this.dock.fit.slotPx,
      DROP_SLACK_ICONS * this.dock.fit.iconSize,
    );
    if (d.to !== d.from && isInsideWithSlack(d.lastX, d.lastY, d.bounds, slack.x, slack.y)) {
      this.dock.reorder.drop(d);
    } else {
      this.dock.reorder.cancelDrag(true);
    }
  };

  readonly onwindowpointercancel = (e: PointerEvent): void => {
    if (this.dock.reorder.drag !== null && e.pointerId === this.dock.reorder.drag.pointerId) {
      this.dock.reorder.cancelDrag(true);
    }
  };
}
