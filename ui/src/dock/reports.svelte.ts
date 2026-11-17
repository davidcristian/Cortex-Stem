import { autoHideHitRegion } from '$lib/dock/autohide';
import {
  edgeShift,
  hitRegionFor,
  integerRect,
  magnifiedRise,
  panelRectReport,
  sameHitRegion,
  sameRect,
  type HitRegion,
  type PanelRectReport,
} from '$lib/dock/geometry';
import { maxGrowth } from '$lib/dock/magnify';

import type { DockController } from './controller.svelte';
import { HEADROOM, HIT_SLACK, PANEL_PADDING, SIDE_HEADROOM, SIDE_SLACK } from './layout';
import { viewportNow } from './view.svelte';

/** What the page reports to Rust from the panel's rest geometry: its content size, hit region and rect. */
export class DockReports {
  private readonly dock: DockController;

  reported = { width: 0, height: 0 };
  reportedHit: HitRegion | null = null;
  /** The resting panel of the latest `set_dock_panel_rect`, with the viewport it is relative to. */
  reportedPanel: PanelRectReport | null = null;

  constructor(dock: DockController) {
    this.dock = dock;
    // Settings can change the headroom and the envelope without changing the panel box.
    $effect(() => {
      void this.dock.view.magnify;
      void this.dock.fit.iconSize;
      this.reportSize();
    });
  }

  /**
   * `set_dock_content_size` (docs/ipc/sizing-protocol.md): the value
   * across the edge is the window's thickness (the height on `bottom` and
   * `top`, the width on `left` and `right`): the panel, the margin to the
   * edge, the magnified icons' rise and the room for the pill and the
   * bounce beyond them. The value along the edge is informational: the
   * panel, its magnification growth and room for the shadow at both ends.
   */
  reportSize(): void {
    const rest = this.dock.measure.restingPanel();
    if (rest === null) {
      return;
    }
    const lengthOf = (panelLength: number): number =>
      Math.ceil(
        panelLength + 2 * SIDE_SLACK + maxGrowth(this.dock.fit.slotPx, this.dock.view.magnify),
      );
    const rise = this.dock.fit.iconSize * (this.dock.view.magnify.maxScale - 1);
    const width = this.dock.view.vertical
      ? Math.ceil(rest.width + this.dock.measure.snapMargins.edge + rise + SIDE_HEADROOM)
      : lengthOf(rest.width);
    const height = this.dock.view.vertical
      ? lengthOf(rest.height)
      : Math.ceil(rest.height + this.dock.measure.snapMargins.edge + rise + HEADROOM);
    if (width !== this.reported.width || height !== this.reported.height) {
      this.reported = { width, height };
      void this.dock.backend
        .setDockContentSize(width, height, this.dock.view.edge)
        .catch(this.dock.intents.report('set_dock_content_size', false));
    }
    this.reportHitRegion();
  }

  /**
   * The panel rounded outwards, and the envelope its magnified icons can
   * reach. A drag in progress makes the whole window clickable instead: the
   * lifted icon can be anywhere in it. The window grows for the drag with
   * its edge side fixed, so on `bottom` (and `right`) the panel's y (x) in
   * the current viewport is the measured one plus the growth
   * (`edgeShift`); reporting the rest one would put the panel far from its
   * real place and the tracker would turn the window click-through while
   * the icon is lifted.
   *
   * With auto-hide active (or a fullscreen app in front) a
   * hidden dock reports the reveal strip as both rectangles, and a shown one
   * an envelope that takes in the strip; with no strip (fullscreen above a
   * visible taskbar) a hidden one reports no region at all
   * (`autoHideHitRegion`).
   */
  currentHitRegion(): HitRegion | null {
    const rest = this.dock.measure.restingPanel();
    if (rest === null) {
      return null;
    }
    const viewport = viewportNow();
    if (this.dock.reorder.drag?.started) {
      const { dx, dy } = edgeShift(
        this.dock.view.edge,
        this.dock.measure.measuredViewport,
        viewport,
      );
      return {
        panel: integerRect({ ...rest, x: rest.x + dx, y: rest.y + dy }),
        envelope: { x: 0, y: 0, width: viewport.width, height: viewport.height },
      };
    }
    const region = hitRegionFor(
      rest,
      // A file drag's gap lengthens the panel by one slot: the
      // window keeps taking the drag there, magnification or not.
      maxGrowth(this.dock.fit.slotPx, this.dock.view.magnify) +
        (this.dock.files.fileDrag !== null ? this.dock.fit.slotPx : 0),
      // A magnified icon's far side is iconSize × (maxScale − 1) beyond its
      // slot's, and the slot sits PANEL_PADDING inside the panel's edge.
      magnifiedRise(
        this.dock.fit.iconSize,
        this.dock.view.magnify.maxScale,
        HIT_SLACK - PANEL_PADDING,
      ),
      this.dock.view.edge,
    );
    return autoHideHitRegion(
      this.dock.autoHide.state,
      region,
      viewport,
      this.dock.view.atScreenEdge,
      this.dock.view.edge,
    );
  }

  readonly reportHitRegion = (): void => {
    this.reportDockPanel();
    const region = this.currentHitRegion();
    if (region === null || (this.reportedHit !== null && sameHitRegion(this.reportedHit, region))) {
      return;
    }
    this.reportedHit = region;
    void this.dock.backend
      .setDockHitRegion(region.panel, region.envelope, this.dock.view.edge)
      .catch(this.dock.intents.report('set_dock_hit_region', false));
  };

  /**
   * Where the panel rests (docs/ipc/dock-panel.md): the measured
   * rest box in the current viewport, whatever the auto-hide phase (the
   * hidden dock's hit region is the strip, which nothing could overlap, so
   * the overlap reference is reported on its own). The window grows for a
   * drag with its edge side fixed, so on `bottom` and `right` the panel
   * shifts by the growth (`edgeShift`), like the hit region's. It
   * goes with its provenance (docs/ipc/report-provenance.md): the
   * edge the page is laid out for and the viewport the rect is relative to
   * on each axis (`panelRectReport`: the current one across the edge, the
   * measured one along it, where nothing moved the rect), which lets Rust
   * re-anchor it to a window it resized meanwhile. Sent only when the rect
   * or that viewport changed.
   */
  reportDockPanel(): void {
    const rest = this.dock.measure.restingPanel();
    if (rest === null) {
      return;
    }
    const next = panelRectReport(
      this.dock.view.edge,
      rest,
      this.dock.measure.measuredViewport,
      viewportNow(),
    );
    if (
      this.reportedPanel !== null &&
      sameRect(this.reportedPanel.rect, next.rect) &&
      this.reportedPanel.viewport.width === next.viewport.width &&
      this.reportedPanel.viewport.height === next.viewport.height
    ) {
      return;
    }
    this.reportedPanel = next;
    void this.dock.backend
      .setDockPanelRect(next.rect, this.dock.view.edge, next.viewport)
      .catch(this.dock.intents.report('set_dock_panel_rect', false));
  }

  /**
   * The device pixel ratio is `ratio` (`watchDevicePixelRatio`): at
   * mount, and again when the display scale changes or the window lands on
   * a monitor with another DPI. The icons follow through `iconSizes`; the
   * panel is measured again, which snaps it to the new device pixel grid;
   * and the content size, the hit region and the panel rect go out
   * again even where their CSS numbers did not change: Rust converts each
   * report with the window's scale factor, so a report belongs to the scale
   * it was made at, and the app layer's own re-layout on its
   * `ScaleFactorChanged` is not the only thing that ties them to the new one.
   */
  readonly onScaleChanged = (ratio: number): void => {
    const previous = this.dock.view.dpr;
    this.dock.view.dpr = ratio;
    if (previous === ratio) {
      return;
    }
    this.reported = { width: 0, height: 0 };
    this.reportedHit = null;
    this.reportedPanel = null;
    this.dock.measure.scheduleMeasure();
  };
}
