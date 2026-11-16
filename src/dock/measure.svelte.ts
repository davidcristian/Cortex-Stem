import { untrack } from 'svelte';

import { spanAlong, type DockEdge } from '$lib/dock/edge';
import {
  edgeShift,
  integerRect,
  sameSnap,
  snapMarginProperties,
  snapOffsets,
  type SnapMargins,
  type Viewport,
} from '$lib/dock/geometry';
import type { Rect } from '$lib/ipc/generated/Rect';

import type { DockController } from './controller.svelte';
import { EDGE_MARGIN } from './layout';
import { viewportNow } from './view.svelte';

/** The panel's rest geometry, measured when nothing moves it, and what a resize does to it. */
export class PanelMeasure {
  private readonly dock: DockController;

  // --- Sizing and hit region (docs/ipc/sizing-protocol.md, docs/ipc/hit-region.md) ---
  // Both are reported from the panel's *rest* geometry: never while the
  // pointer magnifies icons, while they ease back, or while an item is
  // animating in or out. Each report goes out only when its values changed.
  /** The panel's bounding box at rest, viewport-relative CSS px, after snapping. */
  measured: Rect | null = null;
  /** The viewport when `measured` was taken: the one its coordinates refer to. */
  measuredViewport: Viewport = { width: 0, height: 0 };
  /**
   * The edge `measured` was laid out for. After a position change
   * the old box describes the old layout until the next measurement, and
   * effects of the same `dock://settings` (the magnification's, the work
   * area's) can report before the edge's own effect runs: `restingPanel`
   * hides the old box from every report meanwhile.
   */
  measuredEdge: DockEdge | null = null;
  /**
   * The inline margins that hold the panel on whole device pixels.
   * The stage centres the panel along the dock, so with an odd
   * `innerWidth − panelWidth` every bitmap inside would sit on a half pixel
   * and be smeared by the compositor; the margins cancel that (see
   * `snapOffsets`; which CSS margins they are depends on the edge,
   * `snapMarginProperties`).
   */
  snapMargins: SnapMargins = { start: 0, edge: EDGE_MARGIN };
  measureFrame = 0;
  laidOutEdge: DockEdge | null = null;

  constructor(dock: DockController) {
    this.dock = dock;
    // The edge changed (a `dock://settings` with another position): the
    // stylesheet has already laid the panel out for it (`data-edge`). The
    // measurement of the old edge goes first (`restingPanel` already ignores
    // it): ending a file drag reports the hit region, and so does a drag's end
    // when it moves the auto-hide phase, and none of those may send the old
    // edge's rest box. Then whatever was going on along the old axis stops,
    // the snap margins of the old edge go (they are other CSS margins now),
    // and everything is measured and reported again for the new one; Rust
    // moves the window meanwhile and keeps its old thickness until the next
    // `set_dock_content_size` (docs/ipc/sizing-protocol.md, step 5).
    $effect(() => {
      const now = this.dock.view.edge;
      untrack(() => {
        const before = this.laidOutEdge;
        this.laidOutEdge = now;
        if (before === null || before === now) {
          return;
        }
        this.measured = null;
        this.dock.autoHide.stripBox = null;
        this.dock.reports.reported = { width: 0, height: 0 };
        this.dock.reports.reportedHit = null;
        this.dock.reports.reportedPanel = null;
        this.dock.reorder.cancelDrag(false);
        this.dock.reorder.clearDragStyles();
        this.dock.files.endFileDrag(false, false);
        this.dock.hover.leaveHoverState();
        this.snapMargins = { start: 0, edge: EDGE_MARGIN };
        this.dock.panelEl?.style.removeProperty('margin');
        this.scheduleMeasure();
      });
    });
    // The work area reached the screen's edge or left it (the stock taskbar
    // auto-hidden or shown): the strip, and the region with it, may
    // come or go.
    $effect(() => {
      void this.dock.view.atScreenEdge;
      untrack(this.dock.reports.reportHitRegion);
    });
    $effect(() => {
      const panel = this.dock.panelEl;
      if (!panel) {
        return;
      }
      const observer = new ResizeObserver(this.scheduleMeasure);
      // The border box: what is measured, and what a padding or border
      // change (dock.css) resizes without touching the content.
      observer.observe(panel, { box: 'border-box' });
      return () => {
        observer.disconnect();
        if (this.measureFrame !== 0) {
          cancelAnimationFrame(this.measureFrame);
          this.measureFrame = 0;
        }
      };
    });
  }

  /** Measure on the next frame, at most once per frame. */
  readonly scheduleMeasure = (): void => {
    if (this.measureFrame === 0) {
      this.measureFrame = requestAnimationFrame(() => {
        this.measureFrame = 0;
        this.measurePanel();
      });
    }
  };

  /** A slot is still animating in or out (its keyframes run on the slot element itself). */
  itemsInFlight(panel: HTMLElement): boolean {
    return (
      panel.querySelector('.slot[data-leaving]') !== null ||
      this.dock.hover.slots.some((el) => el.getAnimations().length > 0)
    );
  }

  measurePanel(): void {
    const panel = this.dock.panelEl;
    if (
      !panel ||
      this.dock.hover.hovering ||
      this.dock.hover.settling ||
      this.dock.reorder.drag?.started ||
      this.itemsInFlight(panel) ||
      // A file drag's gap is open, or closing; `ongapoutroend` measures after it.
      panel.querySelector('.drop-gap') !== null ||
      // The auto-hide slide or fade is running; `onpaneltransitionend` measures after it.
      panel.getAnimations().length > 0
    ) {
      return;
    }
    let rect = this.restRect(panel);
    // Snap to whole device pixels. Only here, at rest: a magnified
    // or dragged panel is not measured at all, and the window's thickness
    // only ever changes by whole device pixels, so a snap survives Rust's
    // resize. The live ratio, not `dpr`: this runs again whenever the
    // geometry does.
    const snap = snapOffsets(
      rect.left,
      rect.top,
      window.devicePixelRatio || 1,
      this.snapMargins,
      this.dock.view.edge,
    );
    if (!sameSnap(this.snapMargins, snap)) {
      this.snapMargins = snap;
      this.applySnapMargins(panel);
      rect = this.restRect(panel);
    }
    this.measured = { x: rect.x, y: rect.y, width: rect.width, height: rect.height };
    this.measuredEdge = this.dock.view.edge;
    this.measuredViewport = viewportNow();
    const whole = integerRect(this.measured);
    const resting = spanAlong(this.dock.view.axis, { left: whole.x, top: whole.y, ...whole });
    const length = resting.end - resting.start;
    const gap = this.snapMargins.edge;
    if (
      this.dock.autoHide.stripBox === null ||
      this.dock.autoHide.stripBox.start !== resting.start ||
      this.dock.autoHide.stripBox.length !== length ||
      this.dock.autoHide.stripBox.gap !== gap
    ) {
      this.dock.autoHide.stripBox = { start: resting.start, length, gap };
    }
    this.dock.reports.reportSize();
    // Measured in a window that is back to the thickness it had before the
    // drag grew it: the pointer may hover again.
    const across = this.dock.view.vertical ? window.innerWidth : window.innerHeight;
    if (this.dock.reorder.restoringAfterDrag && across <= this.dock.reorder.acrossBeforeDrag + 1) {
      this.dock.reorder.restoringAfterDrag = false;
    }
  }

  /**
   * `measured`, when there is one for the current edge and it is not empty:
   * what every size, hit-region and panel-rect report is made from.
   */
  restingPanel(): Rect | null {
    return this.measured !== null &&
      this.measuredEdge === this.dock.view.edge &&
      this.measured.width !== 0
      ? this.measured
      : null;
  }

  /** Write `snapMargins` to the CSS margins they stand for on this edge (`snapMarginProperties`). */
  applySnapMargins(panel: HTMLElement): void {
    const names = snapMarginProperties(this.dock.view.edge);
    panel.style[names.start] = `${this.snapMargins.start}px`;
    panel.style[names.edge] = `${this.snapMargins.edge}px`;
  }

  /**
   * The panel's box where it rests: `getBoundingClientRect` includes the
   * auto-hide slide, the panel's only transform (a pure translation towards
   * the edge; never measured mid-transition), so its offset is taken off
   * again. Items can come and go while the dock is hidden, and the strip and
   * the region on reveal must describe the resting panel.
   */
  restRect(panel: HTMLElement): DOMRect {
    const rect = panel.getBoundingClientRect();
    const transform = getComputedStyle(panel).transform;
    if (transform === '' || transform === 'none') {
      return rect;
    }
    const { m41: dx, m42: dy } = new DOMMatrixReadOnly(transform);
    return dx === 0 && dy === 0
      ? rect
      : new DOMRect(rect.x - dx, rect.y - dy, rect.width, rect.height);
  }

  /**
   * The viewport changed size. During a drag the window grew away
   * from its edge to the work area's size, or shrank back: on `bottom` and
   * `right` its origin moved (`edgeShift`), so every stored coordinate
   * shifts by the change to keep pointing at the same screen spot, and the
   * hit region is reported again for the new viewport (the panel shifted,
   * the envelope the whole window). The shift is always across the dock,
   * so the rest centres along it stay as they are.
   *
   * With no drag in progress the old measurement describes the wrong
   * coordinate space (the window shrank back after a drop, or moved to
   * another monitor), so the rest geometry is measured again. `hovering`
   * is dropped first: when the window turns click-through under a pointer
   * that is still over the panel no `pointerleave` ever arrives, and a
   * stale `hovering` would stop every later measurement for good.
   */
  readonly onwindowresize = (): void => {
    // The fit's room; a drag holds the size whatever it is.
    this.dock.fit.viewportWidth = window.innerWidth;
    this.dock.fit.viewportHeight = window.innerHeight;
    // Shaped for the page's axis, the move done; a resize that leaves the
    // window longer across than along (a thickness set before the move)
    // does not end the wait for it.
    if (
      (this.dock.view.vertical ? this.dock.fit.viewportHeight : this.dock.fit.viewportWidth) >=
      (this.dock.view.vertical ? this.dock.fit.viewportWidth : this.dock.fit.viewportHeight)
    ) {
      this.dock.fit.shapedAxis = this.dock.view.axis;
    }
    const d = this.dock.reorder.drag;
    if (d === null || !d.started) {
      this.dock.hover.leaveHoverState();
      this.scheduleMeasure();
      return;
    }
    const now = viewportNow();
    const { dx, dy } = edgeShift(this.dock.view.edge, d.viewport, now);
    d.viewport = now;
    if (dx !== 0 || dy !== 0) {
      d.startX += dx;
      d.lastX += dx;
      d.startY += dy;
      d.lastY += dy;
      d.bounds = {
        left: d.bounds.left + dx,
        right: d.bounds.right + dx,
        top: d.bounds.top + dy,
        bottom: d.bounds.bottom + dy,
      };
    }
    this.dock.reports.reportHitRegion();
    this.dock.reorder.scheduleDragFrame();
  };
}
