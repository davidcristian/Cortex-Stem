import { watchDevicePixelRatio } from '$lib/dock/dpr';
import type { Backend } from '$lib/ipc/backend';
import type { DockStore } from '$lib/stores/dock.svelte';

import { AutoHideDriver } from './auto-hide.svelte';
import { EdgeFit } from './edge-fit.svelte';
import { FileDrag } from './file-drag.svelte';
import { FileLanding } from './file-landing.svelte';
import { DockIcons } from './icons.svelte';
import { Intents } from './intents.svelte';
import { DockKeyboard } from './keyboard.svelte';
import { Magnifier } from './magnifier.svelte';
import { PanelMeasure } from './measure.svelte';
import { PageTheme } from './page-theme.svelte';
import { ReorderPointer } from './reorder-pointer.svelte';
import { Reorder } from './reorder.svelte';
import { DockReports } from './reports.svelte';
import { Tooltip } from './tooltip.svelte';
import { DockView } from './view.svelte';

/**
 * The parts of the dock page, made once by Dock.svelte. Each part keeps the
 * state of one concern and reaches the others through this object. They are
 * made in the order their effects run in. The effects run once all of them
 * exist, except the `$effect.pre` of `DockIcons`: it runs at once and reads
 * only the view and the fit, which are made before it.
 */
export class DockController {
  backend: Backend = $derived(this.store.backend);
  /** The panel element (`bind:this` in Dock.svelte). */
  panelEl = $state<HTMLDivElement>();
  // --- Item enter/leave animations ---------------------------------------------
  // Items that are in the first snapshot are simply there; intros start with
  // the first change after that, so startup never cascades icons in.
  introsEnabled = $state(false);

  readonly view: DockView;
  readonly fit: EdgeFit;
  readonly pageTheme: PageTheme;
  readonly icons: DockIcons;
  readonly hover: Magnifier;
  readonly tooltip: Tooltip;
  readonly reorder: Reorder;
  readonly reorderPointer: ReorderPointer;
  readonly intents: Intents;
  readonly reports: DockReports;
  readonly measure: PanelMeasure;
  readonly autoHide: AutoHideDriver;
  readonly files: FileDrag;
  readonly landing: FileLanding;
  readonly keys: DockKeyboard;

  /** `store` and `demo` are Dock.svelte's props, read through so they stay reactive. */
  constructor(
    private readonly storeProp: () => DockStore,
    private readonly demoProp: () => boolean,
  ) {
    this.view = new DockView(this);
    this.fit = new EdgeFit(this);
    this.pageTheme = new PageTheme(this);
    this.icons = new DockIcons(this);
    $effect(() => {
      if (this.store.state === null || this.introsEnabled) {
        return;
      }
      const frame = requestAnimationFrame(() => {
        this.introsEnabled = true;
      });
      return () => cancelAnimationFrame(frame);
    });
    this.hover = new Magnifier(this);
    this.tooltip = new Tooltip(this);
    this.reorder = new Reorder(this);
    this.reorderPointer = new ReorderPointer(this);
    this.intents = new Intents(this);
    this.reports = new DockReports(this);
    this.measure = new PanelMeasure(this);
    this.autoHide = new AutoHideDriver(this);
    this.files = new FileDrag(this);
    this.landing = new FileLanding(this);
    this.keys = new DockKeyboard(this);
  }

  /** Created once by the page entry; it owns the backend subscription. */
  get store(): DockStore {
    return this.storeProp();
  }

  /** Browser demo (mock backend): paint a wallpaper behind the dock. */
  get demo(): boolean {
    return this.demoProp();
  }

  /** An enter or leave animation ended: the panel's rest geometry may have changed. */
  readonly onItemSettled = (): void => {
    this.measure.scheduleMeasure();
  };

  /** What the page does once mounted; the function returned undoes it. */
  mount(): () => void {
    const dark = window.matchMedia('(prefers-color-scheme: dark)');
    const motion = window.matchMedia('(prefers-reduced-motion: reduce)');
    this.view.prefersDark = dark.matches;
    this.view.reducedMotion = motion.matches;
    const stopScale = watchDevicePixelRatio(window, this.reports.onScaleChanged);
    const onDark = (): void => {
      this.view.prefersDark = dark.matches;
    };
    const onMotion = (): void => {
      this.view.reducedMotion = motion.matches;
    };
    // The tray and the toggle shortcut show a hidden dock window; shown
    // again while auto-hidden, the dock comes back (ADR-0008). With the
    // window visible and the panel slid out they ask for the panel instead
    // (`dock://show`): the same reveal, without a window round trip.
    const onVisibility = (): void => {
      if (document.visibilityState === 'visible') {
        this.autoHide.send({ type: 'show' });
      }
    };
    const onShowRequested = (): void => {
      this.autoHide.send({ type: 'show' });
    };
    const onFocusRequested = (): void => {
      void this.keys.focusEntry();
    };
    // A second press of the focus shortcut while this dock has the keyboard:
    // what Escape does. A pointer drag in progress is the
    // pointer's and stays.
    const onReleaseRequested = (): void => {
      this.intents.letGo();
    };
    dark.addEventListener('change', onDark);
    motion.addEventListener('change', onMotion);
    document.addEventListener('visibilitychange', onVisibility);
    const offShow = this.backend.on('dock://show', onShowRequested);
    const offFocus = this.backend.on('dock://focus', onFocusRequested);
    const offRelease = this.backend.on('dock://release-focus', onReleaseRequested);
    const offFileDrag = this.backend.onFileDrag(this.files.onfiledrag);
    const offUserCss = this.pageTheme.connect();
    void this.store.start();
    return () => {
      offUserCss();
      offShow();
      offFocus();
      offRelease();
      offFileDrag();
      this.files.endFileDrag(false, false);
      this.landing.finishLanding(false);
      dark.removeEventListener('change', onDark);
      motion.removeEventListener('change', onMotion);
      stopScale();
      document.removeEventListener('visibilitychange', onVisibility);
      this.tooltip.clearTooltip();
      this.hover.resetMagnification();
      if (this.hover.settleTimer !== null) {
        clearTimeout(this.hover.settleTimer);
      }
      if (this.autoHide.autoHideTimer !== null) {
        clearTimeout(this.autoHide.autoHideTimer);
      }
      if (this.autoHide.bounceHoldTimer !== null) {
        clearTimeout(this.autoHide.bounceHoldTimer);
      }
      this.reorder.cancelDrag(false);
      this.reorder.clearDragStyles();
      this.intents.toasts.clear();
      this.store.stop();
    };
  }
}
