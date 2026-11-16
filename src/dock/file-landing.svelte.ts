import { untrack } from 'svelte';

import {
  fileNameOf,
  orderWithDropped,
  pinnedIds,
  recycleSubject,
  type PinCandidate,
} from '$lib/dock/filedrop';
import type { AppId } from '$lib/ipc/generated/AppId';

import type { DockController } from './controller.svelte';
import { sameOrder } from './reorder.svelte';

/** Longest wait for the snapshot with the dropped apps in place; the gap closes after it anyway. */
const LANDING_TIMEOUT_MS = 1500;

interface Landing {
  /** The pinned order sent with `reorder_pinned`. */
  order: AppId[];
  /** The first newly pinned item: it takes the gap's place. */
  key: string | null;
  timer: ReturnType<typeof setTimeout>;
}

/** A drop that pins: the pins and the reorder, and the gap kept until the snapshot shows them. */
export class FileLanding {
  private readonly dock: DockController;

  landing: Landing | null = null;
  /** The item that took the gap's place: it does not grow in, and the gap does not shrink away. */
  landedKey = $state<string | null>(null);

  constructor(dock: DockController) {
    this.dock = dock;
    $effect(() => {
      void this.dock.store.state;
      untrack(this.checkLanding);
    });
  }

  /**
   * Pin each candidate, then put the new ids at `index` in one reorder. A
   * failed pin is reported like any failed command and the others go on.
   */
  async pinDropped(candidates: readonly PinCandidate[], index: number): Promise<void> {
    const before = pinnedIds(this.dock.store.state?.items ?? []);
    this.dock.view.frozenItems = this.dock.view.items;
    // Held until `finishLanding`, also after an earlier landing it cut short.
    this.dock.fit.holdFit('file', true);
    const ids: AppId[] = [];
    for (const candidate of candidates) {
      try {
        ids.push(await this.dock.backend.pinTarget(candidate.target));
      } catch (err) {
        this.dock.intents.report(`Pinning ${fileNameOf(candidate.path)}`)(err);
      }
    }
    if (ids.length === 0) {
      this.finishLanding(false);
      return;
    }
    const order = orderWithDropped(before, ids, index);
    const fresh = ids.find((id) => !before.includes(id));
    this.landing = {
      order,
      key: fresh === undefined ? null : `app:${fresh}`,
      timer: setTimeout(() => this.finishLanding(false), LANDING_TIMEOUT_MS),
    };
    try {
      await this.dock.backend.reorderPinned(order);
    } catch (err) {
      this.dock.intents.report('Reordering')(err);
      this.finishLanding(false);
      return;
    }
    this.checkLanding();
  }

  /**
   * Send a drop on the Recycle Bin to the bin. Rust sorts the paths and the
   * shell may ask the user first, so this can take a while; a refusal is
   * reported like any failed command.
   */
  async recycleDropped(paths: readonly string[]): Promise<void> {
    try {
      await this.dock.backend.deleteToRecycleBin([...paths]);
    } catch (err) {
      this.dock.intents.report(`Moving ${recycleSubject(paths)} to the Recycle Bin`)(err);
    }
  }

  /** The snapshot shows the dropped apps in their place: render it. */
  readonly checkLanding = (): void => {
    const l = this.landing;
    const state = this.dock.store.state;
    if (l !== null && state !== null && sameOrder(pinnedIds(state.items), l.order)) {
      this.finishLanding(true);
    }
  };

  /**
   * Show the snapshot again and close the gap: at once when an item takes
   * its place (`landed`), shrinking away otherwise (a failure, the timeout).
   */
  finishLanding(landed: boolean): void {
    const l = this.landing;
    this.landing = null;
    if (l !== null) {
      clearTimeout(l.timer);
    }
    if (this.dock.view.frozenItems === null) {
      return;
    }
    this.landedKey = landed && l !== null ? l.key : null;
    this.dock.view.frozenItems = null;
    this.dock.files.showGap(null);
    if (this.dock.files.fileDrag === null) {
      this.dock.fit.holdFit('file', false);
    }
    this.dock.measure.scheduleMeasure();
    if (this.landedKey !== null) {
      // The intro and outro decisions are taken in this update; forget the key after it.
      requestAnimationFrame(() => {
        this.landedKey = null;
      });
    }
    if (this.dock.files.fileDrag !== null) {
      // Another drag came in meanwhile: its gap, in the list now shown.
      this.dock.files.scheduleFileDragFrame();
    }
  }

  readonly ongapoutroend = (): void => {
    this.dock.measure.scheduleMeasure();
  };
}
