import { workAreaReachesEdge } from '$lib/dock/autohide';
import { axisOf, type DockEdge } from '$lib/dock/edge';
import type { Viewport } from '$lib/dock/geometry';
import type { MagnifyParams } from '$lib/dock/magnify';
import { dockFactsFor, dockMonitor, itemsForMonitor, monitorFromSearch } from '$lib/dock/monitor';
import { accentColor, effectiveOpacity, resolveTheme } from '$lib/dock/theme';
import type { DockItem as DockItemModel } from '$lib/ipc/generated/DockItem';

import type { DockController } from './controller.svelte';

/** What the dock page draws, worked out from the store, the settings and the environment. */
export class DockView {
  /** Set in the constructor; the `$derived` fields read it only later. */
  private readonly dock!: DockController;

  // --- Environment facts ----------------------------------------------------
  prefersDark = $state(false);
  reducedMotion = $state(false);
  /** Device pixels per CSS px, followed live (`onScaleChanged`). */
  dpr = $state(1);

  // --- Settings-derived values -----------------------------------------------
  settings = $derived(this.dock.store.settings);

  // --- This dock's monitor ------------------------------------------------------
  // With one dock per monitor the page learns its monitor from its URL
  // (`dock.html?monitor=<id>`); the primary's page has none and takes the
  // primary monitor of the list. The monitor picks this dock's facts in
  // `DockState.docks` and, with the setting on, narrows the items to the
  // windows on it (src/lib/dock/monitor.ts, pure). Everything else about
  // the items (order, grouping, what a click does) is Rust's.
  readonly urlMonitor = monitorFromSearch(window.location.search);
  myMonitor = $derived(dockMonitor(this.urlMonitor, this.dock.store.monitors));
  // The filter applies only with a dock on every monitor: with the
  // primary's dock alone, the windows on the other monitors would be on no
  // dock at all.
  showOnlyMine = $derived(
    (this.settings?.behavior.showOnlyCurrentMonitor ?? false) && this.settings?.monitors === 'all',
  );
  visibleItems = $derived(
    itemsForMonitor(this.dock.store.state?.items ?? [], this.myMonitor, this.showOnlyMine),
  );
  facts = $derived(dockFactsFor(this.dock.store.state?.docks ?? [], this.myMonitor));
  // --- The edge (docs/ipc/sizing-protocol.md) ----------------------------
  // `Settings.position` alone says where the window is: the page lays out
  // for it (horizontal on `bottom` and `top`, vertical on `left` and
  // `right`) and lays out again when a `dock://settings` changes it. The
  // other parts read `edge` and `axis` when they measure; the stage
  // carries them for the stylesheet (`data-edge`, and `data-axis` on the
  // panel for the item transitions, item-transition.ts).
  edge = $derived<DockEdge>(this.settings?.position ?? 'bottom');
  axis = $derived(axisOf(this.edge));
  vertical = $derived(this.axis === 'vertical');
  /**
   * This dock window's edge side is the screen's: its monitor's
   * work area reaches the monitor's edge on the dock's side, no visible
   * stock taskbar (or other app bar) between them.
   */
  atScreenEdge = $derived(
    workAreaReachesEdge(
      this.dock.store.monitors.find((m) => m.id === this.myMonitor),
      this.edge,
    ),
  );
  /**
   * The item list a file drop keeps on screen while its pins land:
   * the snapshots in between would show each new item at the end of the
   * pinned group first, before the reorder moves it into the gap.
   */
  frozenItems = $state.raw<DockItemModel[] | null>(null);
  items = $derived(this.frozenItems ?? this.visibleItems);
  theme = $derived(
    resolveTheme(this.settings?.theme ?? 'system', this.dock.store.theme, this.prefersDark),
  );
  accent = $derived(accentColor(this.dock.store.theme));
  highContrast = $derived(this.dock.store.theme?.highContrast ?? false);
  opacity = $derived(
    effectiveOpacity(this.settings?.appearance.opacity ?? 0.72, this.dock.store.theme),
  );
  cornerRadius = $derived(this.settings?.appearance.cornerRadius ?? 22);
  /** The theme preset: the stage's `data-theme`, which the preset rules of DockStage.svelte key on. */
  preset = $derived(this.settings?.appearance.theme ?? 'standard');
  magnify = $derived<MagnifyParams>({
    maxScale:
      this.settings?.magnification.enabled && !this.reducedMotion
        ? this.settings.magnification.maxScale
        : 1,
    rangeIcons: this.settings?.magnification.rangeIcons ?? 3,
  });
  tooltipsEnabled = $derived(this.settings?.appearance.tooltips ?? true);
  bounceEnabled = $derived(
    (this.settings?.appearance.bounceOnLaunch ?? true) && !this.reducedMotion,
  );
  itemMotion = $derived(!this.reducedMotion);
  showDots = $derived(this.settings?.behavior.runningIndicators ?? true);
  /**
   * `behavior.scrollAction`: the wheel over an item sends nothing
   * while it is `none`, but over the volume item (`wheelSends`).
   */
  scrollAction = $derived(this.settings?.behavior.scrollAction ?? 'none');

  constructor(dock: DockController) {
    this.dock = dock;
  }
}

/** The viewport's size now (CSS px). */
export function viewportNow(): Viewport {
  return { width: window.innerWidth, height: window.innerHeight };
}
