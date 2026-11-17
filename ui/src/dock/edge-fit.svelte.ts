import { untrack } from 'svelte';

import type { DockAxis } from '$lib/dock/edge';
import {
  awaitingMove,
  fitIconSize,
  fitRoom,
  MOVE_WAIT_MS,
  slotCounts,
  workAreaCss,
  type RoomInput,
} from '$lib/dock/fit';

import type { DockController } from './controller.svelte';
import { PANEL_LAYOUT, SLOT_GAP } from './layout';

/** The icon size the page draws at: the setting, made smaller when the dock would not fit its edge. */
export class EdgeFit {
  /** Set in the constructor; the `$derived` fields read it only later. */
  private readonly dock!: DockController;

  // --- Fitting the edge (docs/ipc/sizing-protocol.md) --------------------
  // `Settings.iconSize` is the user's size; the page draws at the fitted
  // one (`fit.ts`): smaller, when the panel at rest plus magnification's
  // growth would not fit the viewport's length along the edge. Everything
  // on the page that scales with the icon (tiles, gaps, separators, dots, the
  // magnification peak, the drag and file-drop gaps, the bitmaps, every
  // geometry report) reads `iconSize`, the fitted size. It follows the
  // viewport's length (`onwindowresize`), the items, the setting, the
  // magnification and the edge, and never the panel's measured size, so
  // drawing at it cannot change it; a drag holds it (`holdFit`).
  settingsIconSize = $derived(this.dock.view.settings?.iconSize ?? 48);
  /** The viewport's size (CSS px), followed on `resize`: its length along the edge is the fit's room. */
  viewportWidth = $state(window.innerWidth);
  viewportHeight = $state(window.innerHeight);
  /**
   * The axis the window is shaped for (`fit.ts`, `RoomInput.shapedAxis`):
   * the page's at the first settings, at a `resize` that leaves the window
   * longer along the page's axis than across it, and when `MOVE_WAIT_MS`
   * ran out. After a change of `position` to the other axis the page lays
   * itself out before Rust moves its window (docs/ipc/sizing-protocol.md,
   * step 5); until then (`awaitingMove`) the work area's length along the
   * new edge, the length the window is about to have, is the fit's room,
   * so the first reports for the new edge are made at the size it will
   * fit.
   */
  shapedAxis = $state<DockAxis | null>(null);
  room = $derived<RoomInput>({
    axis: this.dock.view.axis,
    shapedAxis: this.shapedAxis,
    viewport: { width: this.viewportWidth, height: this.viewportHeight },
    workArea: workAreaCss(this.dock.store.monitors.find((m) => m.id === this.dock.view.myMonitor)),
  });
  fitLength = $derived(fitRoom(this.room));
  waitingForMove = $derived(awaitingMove(this.room));
  fitSize = $derived(
    fitIconSize({
      iconSize: this.settingsIconSize,
      counts: slotCounts(this.dock.view.items),
      layout: PANEL_LAYOUT,
      magnify: this.dock.view.magnify,
      viewportLength: this.fitLength,
    }),
  );
  /** The size a drag in progress keeps: the one it started with; `null` with no drag. */
  heldIconSize = $state<number | null>(null);
  iconSize = $derived(this.heldIconSize ?? this.fitSize);
  /** Drawn smaller than the setting: the stage's `data-fitted`. */
  fitted = $derived(this.iconSize < this.settingsIconSize);
  slotPx = $derived(this.iconSize + SLOT_GAP);

  /**
   * What holds the fitted size (docs/ipc/sizing-protocol.md §Fitting the
   * edge): a drag-reorder from its start until its styles are cleared
   * (its rest centres, bounds and gap shifts are measured at one size),
   * and a file drag until it is over or, for a drop that pins, until it
   * has landed.
   */
  readonly fitHolds = { reorder: false, file: false };

  constructor(dock: DockController) {
    this.dock = dock;
    $effect(() => {
      if (this.dock.view.settings !== null && this.shapedAxis === null) {
        this.shapedAxis = untrack(() => this.dock.view.axis);
      }
    });
    // No move came (`MockBackend` has no window to move, or the move
    // failed): the window is as it is, and its viewport is the room again.
    $effect(() => {
      if (!this.waitingForMove) {
        return;
      }
      const timer = window.setTimeout(() => {
        this.shapedAxis = this.dock.view.axis;
      }, MOVE_WAIT_MS);
      return () => window.clearTimeout(timer);
    });
  }

  holdFit(kind: keyof EdgeFit['fitHolds'], on: boolean): void {
    this.fitHolds[kind] = on;
    if (!this.fitHolds.reorder && !this.fitHolds.file) {
      this.heldIconSize = null;
    } else if (this.heldIconSize === null) {
      this.heldIconSize = untrack(() => this.iconSize);
    }
  }
}
