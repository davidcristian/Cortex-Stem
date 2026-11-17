import { isAutoHidden, revealStrip, revealStripOn } from '$lib/dock/autohide';
import { along, centreAlong, spanAlong } from '$lib/dock/edge';
import {
  dropIndex,
  dropTargetAt,
  fileDropAction,
  gapAnchor,
  pinCandidates,
  pinTargetFor,
  sameAnchor,
  type FileDragSpot,
  type GapAnchor,
  type ItemSpan,
} from '$lib/dock/filedrop';
import { rectContainsPoint } from '$lib/dock/geometry';
import type { CssPoint, FileDragEvent } from '$lib/ipc/backend';
import type { DockItemKind } from '$lib/ipc/generated/DockItemKind';

import type { DockController } from './controller.svelte';
import { viewportNow } from './view.svelte';

// --- Drop files to pin ------------------------------------------------------------
// A file dragged from Explorer is an OLE drag: no pointer or HTML5 drag
// event reaches the page. Tauri reports it instead (`Backend.onFileDrag`,
// CSS px of the viewport), and only while the window takes the pointer,
// i.e. once the cursor has entered the panel (docs/ipc/hit-region.md).
// Over the panel, with something pinnable in the drag, a one-slot gap
// opens among the pinned apps where the drop would go: a real element in
// the row (`.drop-gap`) that grows and shrinks like an item, at the
// reorder gap's pace, so the panel widens around it and the neighbours
// make room. The drop pins each path (`pin_target`) and puts the new ids
// at the gap with one `reorder_pinned`; the dock keeps showing the gap
// until the snapshot with that order arrives, and the first new item then
// takes the gap's place.
//
// Over the Recycle Bin item no gap opens, whatever the drag
// carries: the item is highlighted instead, the way hover shows an item
// (its icon a little larger, its title pill), and a drop there sends
// every path to `delete_to_recycle_bin`. Its icon turns full from the
// snapshot Rust sends once the shell is done.
interface FileDragSession {
  /** Every path of the drag, in its order (`fileDropAction` decides what they are good for). */
  paths: string[];
  /** The latest position (viewport CSS px). */
  x: number;
  y: number;
  frame: number;
}

/** A file dragged from the shell over the dock: the gap it opens, or the bin it points at, and its drop. */
export class FileDrag {
  private readonly dock: DockController;

  fileDrag: FileDragSession | null = null;
  /** Where the drop gap is drawn (`gapAnchor`), or `null`. */
  dropGap = $state.raw<GapAnchor | null>(null);
  /** The item a drop would land on instead of the gap (the Recycle Bin), highlighted; or `null`. */
  dropTargetKey = $state<string | null>(null);

  constructor(dock: DockController) {
    this.dock = dock;
  }

  readonly onfiledrag = (e: FileDragEvent): void => {
    if (this.dock.reorder.drag?.started) {
      // A lifted icon owns the dock (an OLE drag holds the mouse, so this
      // should never happen).
      return;
    }
    switch (e.type) {
      case 'enter':
        this.beginFileDrag(e.paths, e.position);
        return;
      case 'over':
        if (this.fileDrag !== null) {
          this.fileDrag.x = e.position.x;
          this.fileDrag.y = e.position.y;
          this.scheduleFileDragFrame();
        }
        return;
      case 'drop':
        this.dropFiles(e.paths, e.position);
        return;
      case 'leave':
        // Out of the window, or cancelled: the pointer is away.
        this.endFileDrag(false, false);
        return;
    }
  };

  beginFileDrag(paths: readonly string[], at: CssPoint): void {
    // A session whose leave never came must not linger.
    this.endFileDrag(false, false);
    this.fileDrag = { paths: [...paths], x: at.x, y: at.y, frame: 0 };
    // The gap opens and the drop lands at the size the drag starts with.
    this.dock.fit.holdFit('file', true);
    // Nothing magnified: the gap's place is found from where the slots are drawn.
    this.dock.hover.leaveHoverState();
    this.dock.autoHide.send({ type: 'hold', hold: 'fileDrag', active: true });
    // The envelope makes room for the gap (`currentHitRegion`).
    this.dock.reports.reportHitRegion();
    this.applyFileDrag();
  }

  scheduleFileDragFrame(): void {
    if (this.fileDrag !== null && this.fileDrag.frame === 0) {
      this.fileDrag.frame = requestAnimationFrame(this.applyFileDrag);
    }
  }

  /** Open, move or close the gap, or highlight the bin, for where the drag is now. */
  readonly applyFileDrag = (): void => {
    const d = this.fileDrag;
    if (d === null) {
      return;
    }
    if (d.frame !== 0) {
      cancelAnimationFrame(d.frame);
      d.frame = 0;
    }
    if (isAutoHidden(this.dock.autoHide.state) && this.inRevealStrip(d.x, d.y)) {
      // Like the pointer touching the strip: the dock comes up for the drop.
      this.dock.autoHide.send({ type: 'strip' });
    }
    const action = fileDropAction(this.fileDragSpot(d.x, d.y), d.paths);
    this.showGap(action.kind === 'pin' ? gapAnchor(this.dock.view.items, action.index) : null);
    this.dropTargetKey = action.kind === 'recycle' ? action.key : null;
  };

  /** Over the reveal strip (ADR-0008), where a hidden dock takes the pointer; never without one. */
  inRevealStrip(x: number, y: number): boolean {
    if (
      this.dock.autoHide.stripBox === null ||
      !revealStripOn(this.dock.autoHide.state, this.dock.view.atScreenEdge)
    ) {
      return false;
    }
    const { start, length } = this.dock.autoHide.stripBox;
    const resting = this.dock.view.vertical
      ? { x: 0, y: start, width: 0, height: length }
      : { x: start, y: 0, width: length, height: 0 };
    return rectContainsPoint(revealStrip(resting, viewportNow(), this.dock.view.edge), x, y);
  }

  /**
   * Where a file drag at (`x`, `y`) points, measured where everything is
   * drawn now (an open gap included). The item under the pointer and the
   * gap's index are separate answers (`fileDropAction`).
   */
  fileDragSpot(x: number, y: number): FileDragSpot {
    const panel = this.dock.panelEl;
    if (!panel || isAutoHidden(this.dock.autoHide.state)) {
      return { onPanel: false, over: null, index: 0 };
    }
    const spans: ItemSpan[] = [];
    const centres: number[] = [];
    for (const el of this.dock.hover.slots) {
      const rect = el.getBoundingClientRect();
      spans.push({
        key: el.dataset.key ?? '',
        // `data-kind` is the item's `DockItem.kind` (DockItem.svelte and the separator slot).
        kind: (el.dataset.kind ?? 'separator') as DockItemKind,
        ...spanAlong(this.dock.view.axis, rect),
      });
      if (el.dataset.kind === 'app' && el.dataset.pinned === 'true') {
        centres.push(centreAlong(this.dock.view.axis, rect));
      }
    }
    const pos = along(this.dock.view.axis, x, y);
    return {
      onPanel: rectContainsPoint(panel.getBoundingClientRect(), x, y),
      over: dropTargetAt(pos, spans),
      index: dropIndex(pos, centres),
    };
  }

  showGap(anchor: GapAnchor | null): void {
    if (!sameAnchor(this.dropGap, anchor)) {
      this.dropGap = anchor;
    }
  }

  dropFiles(paths: readonly string[], at: CssPoint): void {
    const action = fileDropAction(this.fileDragSpot(at.x, at.y), paths);
    // Released on the dock, the pointer is there now; no `pointerenter` will say so.
    this.endFileDrag(this.dock.autoHide.isOnDock(at.x, at.y), action.kind === 'pin');
    if (action.kind === 'recycle') {
      void this.dock.landing.recycleDropped(paths);
      return;
    }
    for (const path of paths) {
      if (pinTargetFor(path) === null) {
        console.info(`drop: ${path} is not a shortcut or a program; not pinned`);
      }
    }
    if (action.kind !== 'pin') {
      return;
    }
    const candidates = pinCandidates(paths);
    // A drop still landing shows the truth first; this one starts from it.
    this.dock.landing.finishLanding(false);
    this.showGap(gapAnchor(this.dock.view.items, action.index));
    void this.dock.landing.pinDropped(candidates, action.index);
  }

  /**
   * The drag is over: say where the pointer is now, release the hold, and
   * close the gap unless a drop is about to land in it (`keepGap`) or still
   * landing.
   */
  endFileDrag(pointerOnDock: boolean, keepGap: boolean): void {
    const d = this.fileDrag;
    if (d === null) {
      return;
    }
    this.fileDrag = null;
    if (d.frame !== 0) {
      cancelAnimationFrame(d.frame);
    }
    if (!keepGap && this.dock.view.frozenItems === null) {
      this.showGap(null);
      // Nothing is landing: the fit may follow again (`finishLanding` otherwise).
      this.dock.fit.holdFit('file', false);
    }
    this.dropTargetKey = null;
    this.dock.autoHide.send({ type: 'pointer', inside: pointerOnDock });
    this.dock.autoHide.send({ type: 'hold', hold: 'fileDrag', active: false });
    this.dock.reports.reportHitRegion();
  }
}
