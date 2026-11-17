import { centreAlong } from '$lib/dock/edge';
import { computeScales } from '$lib/dock/magnify';

import type { DockController } from './controller.svelte';
import { sameOrder } from './reorder.svelte';

/** Magnification under the pointer, and the hover that drives it. */
export class Magnifier {
  private readonly dock: DockController;

  // --- Magnification controller (imperative, size only) ----------------------
  settling = $state(false);
  /** The slots that take part in layout: leaving ones are excluded. */
  slots: HTMLElement[] = [];
  scalable: boolean[] = [];
  centres = new Float64Array(0);
  scales = new Float64Array(0);
  applied = new Float64Array(0);
  /** Which slots show their magnified bitmap (`data-magnified`, ItemIcon.svelte). */
  magnified: boolean[] = [];
  /** The pointer's latest position over the panel (viewport CSS px); only its coordinate along the dock counts. */
  pointerX = 0;
  pointerY = 0;
  /** A move put the pointer over the panel and no leave has come since, magnification on or not. */
  pointerOver = false;
  hovering = false;
  frame = 0;
  settleTimer: ReturnType<typeof setTimeout> | null = null;

  constructor(dock: DockController) {
    this.dock = dock;
    // Re-collect the slot elements whenever the item list changed (not per
    // frame). This runs after the DOM was updated, so a slot that is on its
    // way out already carries `data-leaving` (item-transition.ts).
    $effect(() => {
      void this.dock.view.items;
      const panel = this.dock.panelEl;
      if (!panel) {
        return;
      }
      this.slots = Array.from(panel.querySelectorAll<HTMLElement>('.slot:not([data-leaving])'));
      this.scalable = this.slots.map((el) => el.dataset.kind !== 'separator');
      this.centres = new Float64Array(this.slots.length);
      this.scales = new Float64Array(this.slots.length);
      // Slots keep whatever scale they last had; start from that, not from 1,
      // so a slot the pointer has left is still eased back.
      this.applied = Float64Array.from(this.slots, (el) =>
        Number(el.style.getPropertyValue('--scale') || '1'),
      );
      this.magnified = this.slots.map((el) => el.dataset.magnified !== undefined);
      if (this.hovering) {
        this.scheduleFrame();
      }
      // Drag bookkeeping: the snapshot after a drop lands here (clear the
      // transforms, the DOM is now in that order); a list that changed under
      // a live drag invalidates its measurements.
      const pinnedKeys = this.dock.reorder.pinnedSlots().map((el) => el.dataset.key ?? '');
      if (
        this.dock.reorder.pendingOrder !== null &&
        sameOrder(pinnedKeys, this.dock.reorder.pendingOrder)
      ) {
        this.dock.reorder.clearDragStyles();
      }
      if (this.dock.reorder.drag?.started && !sameOrder(pinnedKeys, this.dock.reorder.drag.keys)) {
        this.dock.reorder.cancelDrag(false);
      }
    });
    // A layout change under a resting pointer: a settings change (the
    // icon size, the magnification, the preset) moves and resizes the slots,
    // but no pointer event says so, and the scales would stay where the old
    // layout put them. One frame works them out again from where the pointer
    // last was (it reads the new layout); nothing repeats. Magnification
    // turned on under it starts the hover a move would have started.
    $effect(() => {
      void this.dock.view.settings;
      void this.dock.fit.slotPx;
      if (
        this.pointerOver &&
        this.dock.view.magnify.maxScale > 1 &&
        !this.dock.reorder.drag?.started &&
        !this.dock.reorder.restoringAfterDrag
      ) {
        this.startHover();
      }
      if (this.hovering) {
        this.scheduleFrame();
      }
    });
  }

  scheduleFrame(): void {
    if (this.frame === 0) {
      this.frame = requestAnimationFrame(this.applyMagnification);
    }
  }

  readonly applyMagnification = (): void => {
    this.frame = 0;
    const n = this.slots.length;
    // Reads first, then writes: one layout pass per frame.
    for (let i = 0; i < n; i += 1) {
      this.centres[i] = centreAlong(this.dock.view.axis, this.slots[i]!.getBoundingClientRect());
    }
    computeScales(
      this.dock.view.axis,
      this.pointerX,
      this.pointerY,
      this.centres,
      this.dock.fit.slotPx,
      this.dock.view.magnify,
      this.scales,
    );
    for (let i = 0; i < n; i += 1) {
      if (!this.scalable[i]) {
        continue;
      }
      const s = Math.round(this.scales[i]! * 100) / 100;
      if (s !== this.applied[i]) {
        this.applied[i] = s;
        this.slots[i]!.style.setProperty('--scale', String(s));
        // Same style flush as the size, so the box never shows the rest
        // bitmap larger than 1:1 (while tracking the pointer nothing eases).
        this.setMagnified(i, s > 1);
      }
    }
  };

  /** Switch slot `i` between its rest bitmap (1:1) and its magnified one. */
  setMagnified(i: number, on: boolean): void {
    if (this.magnified[i] === on) {
      return;
    }
    this.magnified[i] = on;
    if (on) {
      this.slots[i]!.dataset.magnified = '';
    } else {
      delete this.slots[i]!.dataset.magnified;
    }
  }

  /** Every box is back at its rest size: the rest bitmaps can show again. */
  showRestIcons(): void {
    for (let i = 0; i < this.slots.length; i += 1) {
      this.setMagnified(i, false);
    }
  }

  /**
   * Back to scale 1. While `settling` the boxes ease back over 120 ms, so
   * the magnified bitmaps stay until the settle timer fires; otherwise the
   * boxes are at rest in the same flush and the rest bitmaps show now.
   */
  resetMagnification(): void {
    if (this.frame !== 0) {
      cancelAnimationFrame(this.frame);
      this.frame = 0;
    }
    for (let i = 0; i < this.slots.length; i += 1) {
      if (this.applied[i] !== 1) {
        this.applied[i] = 1;
        this.slots[i]!.style.setProperty('--scale', '1');
      }
    }
    if (!this.settling) {
      this.showRestIcons();
    }
  }

  /** The pointer is over the panel and magnifies: no settle pending. */
  startHover(): void {
    if (!this.hovering) {
      this.hovering = true;
      this.settling = false;
      if (this.settleTimer !== null) {
        clearTimeout(this.settleTimer);
        this.settleTimer = null;
      }
    }
  }

  readonly onpanelpointerenter = (): void => {
    this.dock.autoHide.send({ type: 'pointer', inside: true });
    void this.dock.backend
      .setDockHover?.(true)
      .catch(this.dock.intents.report('set_dock_hover', false));
  };

  /**
   * Forget that the pointer is over the panel: no tooltip, no magnification,
   * no settle transition, and the boxes at rest in this same flush.
   */
  leaveHoverState(): void {
    this.dock.tooltip.clearTooltip();
    this.pointerOver = false;
    this.hovering = false;
    this.settling = false;
    if (this.settleTimer !== null) {
      clearTimeout(this.settleTimer);
      this.settleTimer = null;
    }
    this.resetMagnification();
  }
}
