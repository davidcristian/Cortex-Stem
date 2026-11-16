import type { DockItem as DockItemModel } from '$lib/ipc/generated/DockItem';

import type { DockController } from './controller.svelte';
import { TOOLTIP_DELAY_MS } from './layout';

/** The pointer's title pill: shown after a short rest on an item, switched at once between items. */
export class Tooltip {
  private readonly dock: DockController;

  // --- Tooltip ----------------------------------------------------------------
  tooltipKey = $state<string | null>(null);
  tooltipTimer: ReturnType<typeof setTimeout> | null = null;

  constructor(dock: DockController) {
    this.dock = dock;
  }

  readonly onhover = (item: DockItemModel | null): void => {
    if (this.tooltipTimer !== null) {
      clearTimeout(this.tooltipTimer);
      this.tooltipTimer = null;
    }
    if (item === null || !this.dock.view.tooltipsEnabled || this.dock.reorder.drag?.started) {
      return;
    }
    if (this.tooltipKey !== null) {
      // Moving between icons switches the pill immediately, like macOS.
      this.tooltipKey = item.key;
      return;
    }
    this.tooltipTimer = setTimeout(() => {
      this.tooltipTimer = null;
      this.tooltipKey = item.key;
    }, TOOLTIP_DELAY_MS);
  };

  clearTooltip(): void {
    if (this.tooltipTimer !== null) {
      clearTimeout(this.tooltipTimer);
      this.tooltipTimer = null;
    }
    this.tooltipKey = null;
  }
}
