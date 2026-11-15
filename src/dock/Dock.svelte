<script lang="ts">
  import { onMount, tick, untrack } from 'svelte';

  import DockItem, { BOUNCE_MS } from '$lib/components/DockItem.svelte';
  import {
    AUTO_HIDE_SLIDE_MS,
    autoHideActive,
    autoHideHitRegion,
    autoHideStep,
    hideDelayMs,
    INITIAL_AUTO_HIDE,
    isAutoHidden,
    onDockAt,
    revealStrip,
    revealStripOn,
    revealStripThickness,
    workAreaReachesEdge,
    type AutoHideEvent,
    type AutoHideState,
  } from '$lib/dock/autohide';
  import { watchDevicePixelRatio } from '$lib/dock/dpr';
  import { along, axisOf, centreAlong, spanAlong, type DockEdge } from '$lib/dock/edge';
  import {
    dropIndex,
    dropTargetAt,
    fileDropAction,
    fileNameOf,
    gapAnchor,
    orderWithDropped,
    pinCandidates,
    pinnedIds,
    pinTargetFor,
    recycleSubject,
    sameAnchor,
    type FileDragSpot,
    type GapAnchor,
    type ItemSpan,
    type PinCandidate,
  } from '$lib/dock/filedrop';
  import {
    edgeShift,
    hitRegionFor,
    integerRect,
    magnifiedRise,
    panelRectReport,
    rectContainsPoint,
    sameHitRegion,
    sameRect,
    sameSnap,
    snapMarginProperties,
    snapOffsets,
    type HitRegion,
    type PanelRectReport,
    type SnapMargins,
    type Viewport,
  } from '$lib/dock/geometry';
  import { iconSources, type IconSources } from '$lib/dock/icon-size';
  import { gapTransition, itemTransition } from '$lib/dock/item-transition';
  import { entryKey, nextFocusIndex, successorKey, tabStopKey } from '$lib/dock/keyboard';
  import { computeScales, maxGrowth, slotCentres, type MagnifyParams } from '$lib/dock/magnify';
  import { dockFactsFor, dockMonitor, itemsForMonitor, monitorFromSearch } from '$lib/dock/monitor';
  import {
    DRAG_START,
    dropSlack,
    exceedsMoveThreshold,
    gapShift,
    insertionIndex,
    isInsideWithSlack,
    moveItem,
    shiftTransform,
    type DropBounds,
  } from '$lib/dock/reorder';
  import { accentColor, effectiveOpacity, resolveTheme } from '$lib/dock/theme';
  import { applyUserCss, NO_USER_CSS, nextUserCss, type UserCssView } from '$lib/dock/user-css';
  import { WheelNotches } from '$lib/dock/wheel';
  import type { Backend, CssPoint, FileDragEvent } from '$lib/ipc/backend';
  import type { AppId } from '$lib/ipc/generated/AppId';
  import type { DockItem as DockItemModel } from '$lib/ipc/generated/DockItem';
  import type { DockItemKind } from '$lib/ipc/generated/DockItemKind';
  import type { IconKey } from '$lib/ipc/generated/IconKey';
  import type { Rect } from '$lib/ipc/generated/Rect';
  import type { DockStore } from '$lib/stores/dock.svelte';
  import { describeError, Toasts } from '$lib/stores/toasts.svelte';

  interface Props {
    /** Created once by the page entry; it owns the backend subscription. */
    store: DockStore;
    /** Browser demo (mock backend): paint a wallpaper behind the dock. */
    demo?: boolean;
  }

  let { store, demo = false }: Props = $props();
  const backend: Backend = $derived(store.backend);

  // Layout constants (CSS px). They are also CSS variables on the stage so
  // the stylesheet and the sizing maths cannot drift apart. A slot is exactly
  // the icon's size across the dock; the running dots hang between it and
  // the edge inside the panel's padding, which is why the padding is the
  // same all round.
  const SLOT_GAP = 8;
  const PANEL_PADDING = 8;
  const DOTS_GAP = 2;
  const DOT_SIZE = 4;
  const SEPARATOR_MARGIN = 6;
  /** Between the panel and the window's edge side (docs/ipc/sizing-protocol.md). */
  const EDGE_MARGIN = 8;
  /** Room for the tooltip and the bounce beyond a magnified icon, on a dock on `bottom` or `top`. */
  const HEADROOM = 48;
  /**
   * The same on `left` and `right`, where the title pill sits beside
   * the icon: 10 px to the pill, which wraps at `PILL_MAX_WIDTH`, plus room
   * for its shadow. Kept with tooltips off too: the failure toasts rest in
   * the same room beside the panel (docs/ipc/sizing-protocol.md §Layout on
   * each edge), and with `HEADROOM` instead they would get 63 px at the
   * default icon size.
   */
  const SIDE_HEADROOM = 240;
  /** The widest a title pill gets beside the icons of a vertical dock before it wraps (CSS px). */
  const PILL_MAX_WIDTH = 216;
  /** Room at each end for the drop shadow. */
  const SIDE_SLACK = 24;
  /**
   * How far beyond a fully magnified icon's far side (its top edge on
   * `bottom`) the hit envelope still counts as "inside" (CSS px). Small on
   * purpose: anything farther is a band where nothing is drawn but clicks
   * would still be swallowed.
   */
  const HIT_SLACK = 2;
  /**
   * The window's thickness (CSS px; Rust clamps it to the work area) while
   * an icon is being dragged, so the lifted icon can go anywhere on screen.
   */
  const DRAG_WINDOW_THICKNESS = 4096;
  /**
   * A release within this many icon sizes of the pinned row, towards or
   * away from the edge, still drops; farther cancels.
   */
  const DROP_SLACK_ICONS = 3;
  const TOOLTIP_DELAY_MS = 300;

  // --- Environment facts ----------------------------------------------------
  let prefersDark = $state(false);
  let reducedMotion = $state(false);
  /** Device pixels per CSS px, followed live (`onScaleChanged`). */
  let dpr = $state(1);

  // --- Settings-derived values -----------------------------------------------
  const settings = $derived(store.settings);

  // --- This dock's monitor ------------------------------------------------------
  // With one dock per monitor the page learns its monitor from its URL
  // (`dock.html?monitor=<id>`); the primary's page has none and takes the
  // primary monitor of the list. The monitor picks this dock's facts in
  // `DockState.docks` and, with the setting on, narrows the items to the
  // windows on it (src/lib/dock/monitor.ts, pure). Everything else about
  // the items (order, grouping, what a click does) is Rust's.
  const urlMonitor = monitorFromSearch(window.location.search);
  const myMonitor = $derived(dockMonitor(urlMonitor, store.monitors));
  // The filter applies only with a dock on every monitor: with the
  // primary's dock alone, the windows on the other monitors would be on no
  // dock at all.
  const showOnlyMine = $derived(
    (settings?.behavior.showOnlyCurrentMonitor ?? false) && settings?.monitors === 'all',
  );
  const visibleItems = $derived(itemsForMonitor(store.state?.items ?? [], myMonitor, showOnlyMine));
  const facts = $derived(dockFactsFor(store.state?.docks ?? [], myMonitor));
  // --- The edge (docs/ipc/sizing-protocol.md) ----------------------------
  // `Settings.position` alone says where the window is: the page lays out
  // for it (horizontal on `bottom` and `top`, vertical on `left` and
  // `right`) and lays out again when a `dock://settings` changes it. The
  // controllers below read `edge` and `axis` when they measure; the stage
  // carries them for the stylesheet (`data-edge`, and `data-axis` on the
  // panel for the item transitions, item-transition.ts).
  const edge = $derived<DockEdge>(settings?.position ?? 'bottom');
  const axis = $derived(axisOf(edge));
  const vertical = $derived(axis === 'vertical');
  /**
   * This dock window's edge side is the screen's: its monitor's
   * work area reaches the monitor's edge on the dock's side, no visible
   * stock taskbar (or other app bar) between them.
   */
  const atScreenEdge = $derived(
    workAreaReachesEdge(
      store.monitors.find((m) => m.id === myMonitor),
      edge,
    ),
  );
  /**
   * The item list a file drop keeps on screen while its pins land:
   * the snapshots in between would show each new item at the end of the
   * pinned group first, before the reorder moves it into the gap.
   */
  let frozenItems = $state.raw<DockItemModel[] | null>(null);
  const items = $derived(frozenItems ?? visibleItems);
  const theme = $derived(resolveTheme(settings?.theme ?? 'system', store.theme, prefersDark));
  const accent = $derived(accentColor(store.theme));
  const highContrast = $derived(store.theme?.highContrast ?? false);
  const opacity = $derived(effectiveOpacity(settings?.appearance.opacity ?? 0.72, store.theme));
  const cornerRadius = $derived(settings?.appearance.cornerRadius ?? 22);
  /** The theme preset: the stage's `data-theme`, which the preset rules below key on. */
  const preset = $derived(settings?.appearance.theme ?? 'standard');
  const iconSize = $derived(settings?.iconSize ?? 48);
  const slotPx = $derived(iconSize + SLOT_GAP);
  const magnify = $derived<MagnifyParams>({
    maxScale:
      settings?.magnification.enabled && !reducedMotion ? settings.magnification.maxScale : 1,
    rangeIcons: settings?.magnification.rangeIcons ?? 3,
  });
  // Two bitmaps per app icon: exactly the rest size in device pixels,
  // drawn 1:1, and the ladder size that covers the true magnified box. One
  // request when they coincide (magnification off, reduced motion, the top
  // of the ladder).
  const iconSizes = $derived(iconSources(iconSize, magnify.maxScale, dpr));
  const dualSources = $derived(iconSizes.magnified !== iconSizes.rest);
  /**
   * The sizes before the latest change of `iconSizes` (a scale change,
   * the icon size or the magnification settings). An item keeps
   * drawing the bitmap it has at those sizes until the one at the new sizes
   * has arrived (`iconUrlAt`), instead of showing the loading placeholder in
   * between. Taken in the effect's teardown: the sizes it captured are the
   * ones the items were drawn from.
   */
  let previousIconSizes = $state.raw<IconSources | null>(null);
  $effect.pre(() => {
    const sizes = iconSizes;
    return () => {
      previousIconSizes = sizes;
    };
  });

  /** The item's bitmap at `size`; while it loads, the one at `fallback` when there is one. */
  function iconUrlAt(
    key: IconKey,
    size: number,
    fallback: number | undefined,
  ): string | null | undefined {
    const url = store.icon(key, size);
    if (url !== undefined || fallback === undefined || fallback === size) {
      return url;
    }
    return store.icon(key, fallback);
  }
  const tooltipsEnabled = $derived(settings?.appearance.tooltips ?? true);
  const bounceEnabled = $derived((settings?.appearance.bounceOnLaunch ?? true) && !reducedMotion);
  const itemMotion = $derived(!reducedMotion);
  const showDots = $derived(settings?.behavior.runningIndicators ?? true);
  /** `behavior.scrollAction`: the wheel over an item sends nothing while it is `none`. */
  const scrollOn = $derived((settings?.behavior.scrollAction ?? 'none') !== 'none');

  // --- Theme on <html> --------------------------------------------------------
  $effect(() => {
    const root = document.documentElement;
    root.dataset.theme = theme;
    if (highContrast) {
      root.dataset.highContrast = '';
    } else {
      delete root.dataset.highContrast;
    }
    if (demo) {
      root.dataset.wallpaper = theme;
    } else {
      delete root.dataset.wallpaper;
    }
  });

  // --- The user's dock.css (docs/ipc/themes.md) --------------------------
  // One <style> element at the end of <head> (src/lib/dock/user-css.ts),
  // replaced on `dock://user-css`. The stable variables only restyle, but
  // anything else in the file may change the panel's size, so the panel is
  // measured again after a change.
  let userCss = $state.raw<UserCssView>(NO_USER_CSS);
  $effect(() => {
    if (applyUserCss(document, userCss.css)) {
      scheduleMeasure();
    }
  });
  // Its rules may key on the preset (the stage's `data-theme`) or the
  // colour mode and a contrast theme (`<html>`'s attributes,
  // docs/THEMING.md): a change of those can move the panel without
  // resizing it (a margin), which the ResizeObserver below does not see
  // (it sees a padding or a border change: it watches the border box).
  // The built-in presets keep the box.
  $effect(() => {
    void preset;
    void theme;
    void highContrast;
    untrack(scheduleMeasure);
  });

  // --- Icons ------------------------------------------------------------------
  // The rest bitmaps first: they are what shows at startup; the magnified
  // ones can arrive behind them. The snapshot's items rather than the
  // rendered ones: the icons of apps a file drop is pinning load while the
  // gap is still on screen.
  $effect(() => {
    const all = store.state?.items ?? [];
    for (const item of all) {
      if (item.iconKey !== null) {
        store.ensureIcon(item.iconKey, iconSizes.rest);
      }
    }
    if (dualSources) {
      for (const item of all) {
        if (item.iconKey !== null) {
          store.ensureIcon(item.iconKey, iconSizes.magnified);
        }
      }
    }
  });

  // --- Item enter/leave animations ---------------------------------------------
  // Items that are in the first snapshot are simply there; intros start with
  // the first change after that, so startup never cascades icons in.
  let introsEnabled = $state(false);
  $effect(() => {
    if (store.state === null || introsEnabled) {
      return;
    }
    const frame = requestAnimationFrame(() => {
      introsEnabled = true;
    });
    return () => cancelAnimationFrame(frame);
  });

  /** An enter or leave animation ended: the panel's rest geometry may have changed. */
  function onItemSettled(): void {
    scheduleMeasure();
  }

  // --- Magnification controller (imperative, size only) ----------------------
  let panelEl = $state<HTMLDivElement>();
  let settling = $state(false);
  /** The slots that take part in layout: leaving ones are excluded. */
  let slots: HTMLElement[] = [];
  let scalable: boolean[] = [];
  let centres = new Float64Array(0);
  let scales = new Float64Array(0);
  let applied = new Float64Array(0);
  /** Which slots show their magnified bitmap (`data-magnified`, DockItem.svelte). */
  let magnified: boolean[] = [];
  /** The pointer's latest position over the panel (viewport CSS px); only its coordinate along the dock counts. */
  let pointerX = 0;
  let pointerY = 0;
  let hovering = false;
  let frame = 0;
  let settleTimer: ReturnType<typeof setTimeout> | null = null;

  // Re-collect the slot elements whenever the item list changed (not per
  // frame). This runs after the DOM was updated, so a slot that is on its
  // way out already carries `data-leaving` (item-transition.ts).
  $effect(() => {
    void items;
    const panel = panelEl;
    if (!panel) {
      return;
    }
    slots = Array.from(panel.querySelectorAll<HTMLElement>('.slot:not([data-leaving])'));
    scalable = slots.map((el) => el.dataset.kind !== 'separator');
    centres = new Float64Array(slots.length);
    scales = new Float64Array(slots.length);
    // Slots keep whatever scale they last had; start from that, not from 1,
    // so a slot the pointer has left is still eased back.
    applied = Float64Array.from(slots, (el) => Number(el.style.getPropertyValue('--scale') || '1'));
    magnified = slots.map((el) => el.dataset.magnified !== undefined);
    if (hovering) {
      scheduleFrame();
    }
    // Drag bookkeeping: the snapshot after a drop lands here (clear the
    // transforms, the DOM is now in that order); a list that changed under
    // a live drag invalidates its measurements.
    const pinnedKeys = pinnedSlots().map((el) => el.dataset.key ?? '');
    if (pendingOrder !== null && sameOrder(pinnedKeys, pendingOrder)) {
      clearDragStyles();
    }
    if (drag?.started && !sameOrder(pinnedKeys, drag.keys)) {
      cancelDrag(false);
    }
  });

  function scheduleFrame(): void {
    if (frame === 0) {
      frame = requestAnimationFrame(applyMagnification);
    }
  }

  function applyMagnification(): void {
    frame = 0;
    const n = slots.length;
    // Reads first, then writes: one layout pass per frame.
    for (let i = 0; i < n; i += 1) {
      centres[i] = centreAlong(axis, slots[i]!.getBoundingClientRect());
    }
    computeScales(axis, pointerX, pointerY, centres, slotPx, magnify, scales);
    for (let i = 0; i < n; i += 1) {
      if (!scalable[i]) {
        continue;
      }
      const s = Math.round(scales[i]! * 100) / 100;
      if (s !== applied[i]) {
        applied[i] = s;
        slots[i]!.style.setProperty('--scale', String(s));
        // Same style flush as the size, so the box never shows the rest
        // bitmap larger than 1:1 (while tracking the pointer nothing eases).
        setMagnified(i, s > 1);
      }
    }
  }

  /** Switch slot `i` between its rest bitmap (1:1) and its magnified one. */
  function setMagnified(i: number, on: boolean): void {
    if (magnified[i] === on) {
      return;
    }
    magnified[i] = on;
    if (on) {
      slots[i]!.dataset.magnified = '';
    } else {
      delete slots[i]!.dataset.magnified;
    }
  }

  /** Every box is back at its rest size: the rest bitmaps can show again. */
  function showRestIcons(): void {
    for (let i = 0; i < slots.length; i += 1) {
      setMagnified(i, false);
    }
  }

  /**
   * Back to scale 1. While `settling` the boxes ease back over 120 ms, so
   * the magnified bitmaps stay until the settle timer fires; otherwise the
   * boxes are at rest in the same flush and the rest bitmaps show now.
   */
  function resetMagnification(): void {
    if (frame !== 0) {
      cancelAnimationFrame(frame);
      frame = 0;
    }
    for (let i = 0; i < slots.length; i += 1) {
      if (applied[i] !== 1) {
        applied[i] = 1;
        slots[i]!.style.setProperty('--scale', '1');
      }
    }
    if (!settling) {
      showRestIcons();
    }
  }

  function onpanelpointermove(e: PointerEvent): void {
    if (!autoHide.pointerInside) {
      // A move over the panel says where the pointer is even when no
      // `pointerenter` came (after a drag or a native menu).
      autoHideSend({ type: 'pointer', inside: true });
    }
    if (magnify.maxScale <= 1 || drag?.started) {
      return;
    }
    pointerX = e.clientX;
    pointerY = e.clientY;
    if (!hovering) {
      hovering = true;
      settling = false;
      if (settleTimer !== null) {
        clearTimeout(settleTimer);
        settleTimer = null;
      }
    }
    scheduleFrame();
  }

  function onpanelpointerenter(): void {
    autoHideSend({ type: 'pointer', inside: true });
    void backend.setDockHover?.(true).catch(report('set_dock_hover', false));
  }

  function onpanelpointerleave(): void {
    hovering = false;
    wheelNotches.reset();
    clearTooltip();
    // Ease back over 120 ms, then go idle (no timers, no transitions).
    settling = !reducedMotion && magnify.maxScale > 1;
    resetMagnification();
    if (settling) {
      settleTimer = setTimeout(() => {
        settleTimer = null;
        settling = false;
        showRestIcons();
        measurePanel();
      }, 160);
    }
    void backend.setDockHover?.(false).catch(report('set_dock_hover', false));
    autoHideSend({ type: 'pointer', inside: false });
  }

  // --- Tooltip ----------------------------------------------------------------
  let tooltipKey = $state<string | null>(null);
  let tooltipTimer: ReturnType<typeof setTimeout> | null = null;

  function onhover(item: DockItemModel | null): void {
    if (tooltipTimer !== null) {
      clearTimeout(tooltipTimer);
      tooltipTimer = null;
    }
    if (item === null || !tooltipsEnabled || drag?.started) {
      return;
    }
    if (tooltipKey !== null) {
      // Moving between icons switches the pill immediately, like macOS.
      tooltipKey = item.key;
      return;
    }
    tooltipTimer = setTimeout(() => {
      tooltipTimer = null;
      tooltipKey = item.key;
    }, TOOLTIP_DELAY_MS);
  }

  function clearTooltip(): void {
    if (tooltipTimer !== null) {
      clearTimeout(tooltipTimer);
      tooltipTimer = null;
    }
    tooltipKey = null;
  }

  // --- Drag to reorder pinned apps -------------------------------------------
  // Imperative like the magnification controller: the lifted slot follows the
  // pointer with an inline transform, the other pinned slots shift by one slot
  // to open the gap, nothing reflows. The drop sends the new order to Rust;
  // the snapshot that follows re-renders the items in that order and the
  // items effect above clears the transforms in the same flush, so the
  // picture never jumps.
  interface DragSession {
    item: DockItemModel;
    el: HTMLElement;
    pointerId: number;
    startX: number;
    startY: number;
    lastX: number;
    lastY: number;
    started: boolean;
    holdTimer: ReturnType<typeof setTimeout> | null;
    frame: number;
    /**
     * Pinned app slots in dock order, with their keys and rest centres along
     * the dock's main axis, measured at start.
     */
    pinned: HTMLElement[];
    keys: string[];
    centres: Float64Array;
    bounds: DropBounds;
    from: number;
    to: number;
    /** The viewport the drag's coordinates refer to (the window grows during a drag). */
    viewport: Viewport;
  }

  let drag: DragSession | null = null;
  /** The item rendered as lifted (`DockItem.lifted`). */
  let liftedKey = $state<string | null>(null);
  /** Pinned keys expected from the snapshot after a drop; transforms stay until it arrives. */
  let pendingOrder: string[] | null = null;
  let pendingTimer: ReturnType<typeof setTimeout> | null = null;
  /** The hit region reported for this drag is the whole viewport; restore it when the drag is over. */
  let dragHitReported = false;

  function pinnedSlots(): HTMLElement[] {
    return slots.filter((el) => el.dataset.kind === 'app' && el.dataset.pinned === 'true');
  }

  function sameOrder(a: readonly string[], b: readonly string[]): boolean {
    return a.length === b.length && a.every((key, i) => key === b[i]);
  }

  function onpress(item: DockItemModel, e: PointerEvent): void {
    if (e.button !== 0 || item.kind !== 'app' || !item.pinned) {
      return;
    }
    // A stale session (a lost pointerup) must not block the next press.
    cancelDrag(false);
    const el = slots.find((s) => s.dataset.key === item.key);
    if (!el) {
      return;
    }
    drag = {
      item,
      el,
      pointerId: e.pointerId,
      startX: e.clientX,
      startY: e.clientY,
      lastX: e.clientX,
      lastY: e.clientY,
      started: false,
      holdTimer: setTimeout(startDrag, DRAG_START.holdMs),
      frame: 0,
      pinned: [],
      keys: [],
      centres: new Float64Array(0),
      bounds: { left: 0, right: 0, top: 0, bottom: 0 },
      from: -1,
      to: -1,
      viewport: viewportNow(),
    };
  }

  function startDrag(): void {
    const d = drag;
    if (d === null || d.started) {
      return;
    }
    if (d.holdTimer !== null) {
      clearTimeout(d.holdTimer);
      d.holdTimer = null;
    }
    d.started = true;
    autoHideSend({ type: 'hold', hold: 'drag', active: true });
    // No magnification, tooltip or settle transition while dragging; the rest
    // positions measured below must be the unmagnified ones.
    leaveHoverState();
    clearDragStyles();
    try {
      d.el.setPointerCapture(d.pointerId);
    } catch {
      // The pointer is already gone; the window listeners still see the release.
    }
    d.pinned = pinnedSlots();
    d.keys = d.pinned.map((el) => el.dataset.key ?? '');
    d.from = d.pinned.indexOf(d.el);
    d.to = d.from;
    d.centres = new Float64Array(d.pinned.length);
    const bounds: DropBounds = {
      left: Infinity,
      right: -Infinity,
      top: Infinity,
      bottom: -Infinity,
    };
    const rects = d.pinned.map((el) => el.getBoundingClientRect());
    slotCentres(axis, rects, d.centres);
    rects.forEach((rect) => {
      bounds.left = Math.min(bounds.left, rect.left);
      bounds.right = Math.max(bounds.right, rect.right);
      bounds.top = Math.min(bounds.top, rect.top);
      bounds.bottom = Math.max(bounds.bottom, rect.bottom);
    });
    d.bounds = bounds;
    liftedKey = d.item.key;
    const transition = reducedMotion ? '' : 'transform 160ms ease';
    for (const el of d.pinned) {
      if (el !== d.el) {
        el.style.transition = transition;
      }
    }
    // The lifted icon may go anywhere on screen: the window grows away from
    // its edge to the work area's size for the duration of the drag (the
    // panel stays on the edge; `onwindowresize` shifts the drag's origin by
    // the growth on `bottom` and `right`) and all of it accepts clicks.
    d.viewport = viewportNow();
    growWindowForDrag();
    dragHitReported = true;
    reportHitRegion();
    applyDrag();
  }

  /** The window's thickness (its height, or its width on a vertical dock) goes to the drag's. */
  function growWindowForDrag(): void {
    if (reported.width === 0) {
      return;
    }
    reported = vertical
      ? { width: DRAG_WINDOW_THICKNESS, height: reported.height }
      : { width: reported.width, height: DRAG_WINDOW_THICKNESS };
    void backend
      .setDockContentSize(reported.width, reported.height, edge)
      .catch(report('set_dock_content_size', false));
  }

  /** The viewport's size now (CSS px). */
  function viewportNow(): Viewport {
    return { width: window.innerWidth, height: window.innerHeight };
  }

  /**
   * Forget that the pointer is over the panel: no tooltip, no magnification,
   * no settle transition, and the boxes at rest in this same flush.
   */
  function leaveHoverState(): void {
    clearTooltip();
    hovering = false;
    settling = false;
    if (settleTimer !== null) {
      clearTimeout(settleTimer);
      settleTimer = null;
    }
    resetMagnification();
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
  function onwindowresize(): void {
    const d = drag;
    if (d === null || !d.started) {
      leaveHoverState();
      scheduleMeasure();
      return;
    }
    const now = viewportNow();
    const { dx, dy } = edgeShift(edge, d.viewport, now);
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
    reportHitRegion();
    scheduleDragFrame();
  }

  function scheduleDragFrame(): void {
    if (drag !== null && drag.frame === 0) {
      drag.frame = requestAnimationFrame(applyDrag);
    }
  }

  function applyDrag(): void {
    const d = drag;
    if (d === null || !d.started) {
      return;
    }
    d.frame = 0;
    const dx = d.lastX - d.startX;
    const dy = d.lastY - d.startY;
    d.el.style.transform = `translate(${dx}px, ${dy}px)`;
    const to = insertionIndex((d.centres[d.from] ?? 0) + along(axis, dx, dy), d.centres, d.from);
    if (to === d.to) {
      return;
    }
    d.to = to;
    d.pinned.forEach((el, i) => {
      if (el === d.el) {
        return;
      }
      el.style.transform = shiftTransform(axis, gapShift(i, d.from, to, slotPx));
    });
  }

  function onwindowpointermove(e: PointerEvent): void {
    const d = drag;
    if (d === null || e.pointerId !== d.pointerId) {
      return;
    }
    d.lastX = e.clientX;
    d.lastY = e.clientY;
    if (!d.started) {
      if (exceedsMoveThreshold(e.clientX - d.startX, e.clientY - d.startY, DRAG_START.movePx)) {
        startDrag();
      }
      return;
    }
    scheduleDragFrame();
  }

  function onwindowpointerup(e: PointerEvent): void {
    const d = drag;
    if (d === null || e.pointerId !== d.pointerId) {
      return;
    }
    if (!d.started) {
      // Released before the drag began: an ordinary click, handled by the item.
      cancelDrag(false);
      return;
    }
    d.lastX = e.clientX;
    d.lastY = e.clientY;
    if (d.frame !== 0) {
      cancelAnimationFrame(d.frame);
      d.frame = 0;
    }
    applyDrag();
    const slack = dropSlack(axis, slotPx, DROP_SLACK_ICONS * iconSize);
    if (d.to !== d.from && isInsideWithSlack(d.lastX, d.lastY, d.bounds, slack.x, slack.y)) {
      drop(d);
    } else {
      cancelDrag(true);
    }
  }

  function onwindowpointercancel(e: PointerEvent): void {
    if (drag !== null && e.pointerId === drag.pointerId) {
      cancelDrag(true);
    }
  }

  function drop(d: DragSession): void {
    const order = moveItem(d.keys, d.from, d.to);
    // Land in the gap now (no transition: the snapshot may arrive within a
    // frame, and the transforms are relative to the old DOM order).
    d.el.style.transform = shiftTransform(axis, (d.centres[d.to] ?? 0) - (d.centres[d.from] ?? 0));
    liftedKey = null;
    drag = null;
    // The window shrinks back now; the pointer may never "leave".
    hovering = false;
    endDragHold(d);
    pendingOrder = order;
    pendingTimer = setTimeout(clearDragStyles, 600);
    const ids = order.map((key) => key.slice('app:'.length));
    void backend.reorderPinned(ids).catch((err: unknown) => {
      clearDragStyles();
      report('Reordering')(err);
    });
  }

  /** Put everything back; `animate` eases the lifted item home instead of snapping. */
  function cancelDrag(animate: boolean): void {
    const d = drag;
    if (d === null) {
      return;
    }
    drag = null;
    if (d.holdTimer !== null) {
      clearTimeout(d.holdTimer);
    }
    if (d.frame !== 0) {
      cancelAnimationFrame(d.frame);
    }
    if (!d.started) {
      return;
    }
    hovering = false;
    endDragHold(d);
    try {
      d.el.releasePointerCapture(d.pointerId);
    } catch {
      // Already released by the browser on pointerup.
    }
    if (!animate || reducedMotion) {
      clearDragStyles();
      return;
    }
    liftedKey = null;
    d.el.style.transition = 'transform 180ms ease';
    for (const el of d.pinned) {
      el.style.transform = '';
    }
    pendingOrder = null;
    pendingTimer = setTimeout(clearDragStyles, 220);
  }

  /** The drag is over and the DOM is in its final order: drop every inline style it set. */
  function clearDragStyles(): void {
    if (pendingTimer !== null) {
      clearTimeout(pendingTimer);
      pendingTimer = null;
    }
    pendingOrder = null;
    liftedKey = null;
    for (const el of slots) {
      el.style.transform = '';
      el.style.transition = '';
    }
    if (dragHitReported) {
      // Back to the real hit region once the DOM has settled (next frame).
      dragHitReported = false;
      scheduleMeasure();
    }
  }

  // --- Intents and failure notices ---------------------------------------------
  const toasts = new Toasts();

  /**
   * Failed commands are logged and, when the user asked for something, shown
   * as a toast (docs/architecture/error-handling.md). Housekeeping calls
   * (hover, sizing) only log: a toast would be noise the user cannot act on.
   */
  const report =
    (what: string, toast = true) =>
    (err: unknown): void => {
      console.warn(`${what} failed:`, err);
      if (toast) {
        toasts.push(`${what} failed: ${describeError(err)}`);
      }
    };

  function onprimary(item: DockItemModel): void {
    if (drag?.started) {
      // The release that ends a drag is not a click.
      return;
    }
    void backend.primaryClick(item.key).catch(report(item.title));
  }

  // The wheel over an item (docs/ipc/scrolling-over-an-item.md):
  // one listener on the panel. `wheel.ts` adds the vertical travel up per
  // item and says when a notch (100 px) is complete; each one is a
  // `scroll_item`, and Rust decides what it does (cycle the item's windows,
  // at most once per 150 ms). Nothing is counted or sent while the setting
  // is `none`, and the page's default handling is only prevented for an
  // event that sends something.
  const wheelNotches = new WheelNotches();

  function onpanelwheel(e: WheelEvent): void {
    if (!scrollOn || drag?.started) {
      wheelNotches.reset();
      return;
    }
    const slot =
      e.target instanceof Element ? e.target.closest<HTMLElement>('.slot[data-key]') : null;
    const key = slot !== null && panelEl?.contains(slot) ? (slot.dataset.key ?? null) : null;
    const notches = wheelNotches.add({
      key,
      deltaX: e.deltaX,
      deltaY: e.deltaY,
      deltaMode: e.deltaMode,
      pageHeight: window.innerHeight,
    });
    if (key === null || notches.length === 0) {
      return;
    }
    // Chromium lets only the first event of a wheel gesture block: when
    // that one sent nothing (a touchpad's first small step), the rest come
    // uncancelable, and trying would only log an intervention.
    if (e.cancelable) {
      e.preventDefault();
    }
    const title = items.find((i) => i.key === key)?.title ?? key;
    for (const direction of notches) {
      void backend.scrollItem(key, direction).catch(report(title));
    }
  }

  function onmiddle(item: DockItemModel): void {
    if (item.appId !== null) {
      void backend.launchApp(item.appId).catch(report(`Launching ${item.title}`));
    }
  }

  // Right-click: the menus are native, built and shown by Rust; the
  // UI only reports which item and where (CSS px of this window's viewport).
  // The item's own `contextmenu` handler calls `oncontext` and the event
  // bubbles on to the panel, where only the panel itself (padding between
  // the slots) opens the dock menu. Separators get nothing.
  function oncontext(item: DockItemModel, x: number, y: number): void {
    if (drag?.started) {
      return;
    }
    menuOpened();
    void backend.showItemMenu(item.key, x, y).catch(report('Menu')).finally(menuClosed);
  }

  function onpanelcontextmenu(e: MouseEvent): void {
    e.preventDefault();
    if (e.target === panelEl && !drag?.started) {
      menuOpened();
      void backend.showDockMenu(e.clientX, e.clientY).catch(report('Menu')).finally(menuClosed);
    }
  }

  // WebView2's own context menu (Back, Forward, Reload, Inspect…) must never
  // appear. The transparent stage outside the panel takes clicks while an
  // icon is being dragged and whenever click-through is off, and a
  // right-click there reached the browser's menu. Item and panel
  // menus are native and already handled; their events bubble up here after.
  function onwindowcontextmenu(e: MouseEvent): void {
    e.preventDefault();
  }

  function openSettings(): void {
    void backend.openSettings().catch(report('Opening Settings'));
  }

  /**
   * Let go of the keyboard: Escape, and a second press of the focus
   * shortcut (`dock://release-focus`). The focused item, if there is
   * one, loses the focus: its pill goes, and with auto-hide on the
   * countdown starts. Rust is asked to give the keyboard focus back
   * (`release_dock_focus`): it brings back the window that was in front
   * when the focus shortcut was pressed, and does nothing when the shortcut
   * did not bring the keyboard here. Housekeeping: a failure only logs.
   * Whether an item lost the focus.
   */
  function letGo(): boolean {
    const active = document.activeElement;
    const blurred = active instanceof HTMLElement && panelEl?.contains(active) === true;
    if (blurred) {
      active.blur();
    }
    void backend.releaseDockFocus().catch(report('release_dock_focus', false));
    return blurred;
  }

  /** Keyboard shortcuts while the dock window has focus. */
  function onwindowkeydown(e: KeyboardEvent): void {
    if (e.key === 'Escape' && drag !== null) {
      e.preventDefault();
      cancelDrag(true);
      return;
    }
    // Otherwise Escape lets go of the keyboard.
    if (e.key === 'Escape') {
      if (letGo()) {
        e.preventDefault();
      }
      return;
    }
    if (e.key === ',' && e.ctrlKey && !e.altKey && !e.metaKey && !e.shiftKey) {
      e.preventDefault();
      openSettings();
    }
  }

  // --- Sizing and hit region (docs/ipc/sizing-protocol.md, docs/ipc/hit-region.md) ---
  // Both are reported from the panel's *rest* geometry: never while the
  // pointer magnifies icons, while they ease back, or while an item is
  // animating in or out. Each report goes out only when its values changed.
  /** The panel's bounding box at rest, viewport-relative CSS px, after snapping. */
  let measured: Rect | null = null;
  /** The viewport when `measured` was taken: the one its coordinates refer to. */
  let measuredViewport: Viewport = { width: 0, height: 0 };
  /**
   * The edge `measured` was laid out for. After a position change
   * the old box describes the old layout until the next measurement, and
   * effects of the same `dock://settings` (the magnification's, the work
   * area's) can report before the edge's own effect runs: `restingPanel`
   * hides the old box from every report meanwhile.
   */
  let measuredEdge: DockEdge | null = null;
  /**
   * The inline margins that hold the panel on whole device pixels.
   * The stage centres the panel along the dock, so with an odd
   * `innerWidth − panelWidth` every bitmap inside would sit on a half pixel
   * and be smeared by the compositor; the margins cancel that (see
   * `snapOffsets`; which CSS margins they are depends on the edge,
   * `snapMarginProperties`).
   */
  let snapMargins: SnapMargins = { start: 0, edge: EDGE_MARGIN };
  let reported = { width: 0, height: 0 };
  let reportedHit: HitRegion | null = null;
  /** The resting panel of the latest `set_dock_panel_rect`, with the viewport it is relative to. */
  let reportedPanel: PanelRectReport | null = null;
  let measureFrame = 0;

  /** Measure on the next frame, at most once per frame. */
  function scheduleMeasure(): void {
    if (measureFrame === 0) {
      measureFrame = requestAnimationFrame(() => {
        measureFrame = 0;
        measurePanel();
      });
    }
  }

  /** A slot is still animating in or out (its keyframes run on the slot element itself). */
  function itemsInFlight(panel: HTMLElement): boolean {
    return (
      panel.querySelector('.slot[data-leaving]') !== null ||
      slots.some((el) => el.getAnimations().length > 0)
    );
  }

  function measurePanel(): void {
    const panel = panelEl;
    if (
      !panel ||
      hovering ||
      settling ||
      drag?.started ||
      itemsInFlight(panel) ||
      // A file drag's gap is open, or closing; `ongapoutroend` measures after it.
      panel.querySelector('.drop-gap') !== null ||
      // The auto-hide slide or fade is running; `onpaneltransitionend` measures after it.
      panel.getAnimations().length > 0
    ) {
      return;
    }
    let rect = restRect(panel);
    // Snap to whole device pixels. Only here, at rest: a magnified
    // or dragged panel is not measured at all, and the window's thickness
    // only ever changes by whole device pixels, so a snap survives Rust's
    // resize. The live ratio, not `dpr`: this runs again whenever the
    // geometry does.
    const snap = snapOffsets(rect.left, rect.top, window.devicePixelRatio || 1, snapMargins, edge);
    if (!sameSnap(snapMargins, snap)) {
      snapMargins = snap;
      applySnapMargins(panel);
      rect = restRect(panel);
    }
    measured = { x: rect.x, y: rect.y, width: rect.width, height: rect.height };
    measuredEdge = edge;
    measuredViewport = viewportNow();
    const whole = integerRect(measured);
    const resting = spanAlong(axis, { left: whole.x, top: whole.y, ...whole });
    const length = resting.end - resting.start;
    const gap = snapMargins.edge;
    if (
      stripBox === null ||
      stripBox.start !== resting.start ||
      stripBox.length !== length ||
      stripBox.gap !== gap
    ) {
      stripBox = { start: resting.start, length, gap };
    }
    reportSize();
  }

  /**
   * `measured`, when there is one for the current edge and it is not empty:
   * what every size, hit-region and panel-rect report is made from.
   */
  function restingPanel(): Rect | null {
    return measured !== null && measuredEdge === edge && measured.width !== 0 ? measured : null;
  }

  /** Write `snapMargins` to the CSS margins they stand for on this edge (`snapMarginProperties`). */
  function applySnapMargins(panel: HTMLElement): void {
    const names = snapMarginProperties(edge);
    panel.style[names.start] = `${snapMargins.start}px`;
    panel.style[names.edge] = `${snapMargins.edge}px`;
  }

  /**
   * The panel's box where it rests: `getBoundingClientRect` includes the
   * auto-hide slide, the panel's only transform (a pure translation towards
   * the edge; never measured mid-transition), so its offset is taken off
   * again. Items can come and go while the dock is hidden, and the strip and
   * the region on reveal must describe the resting panel.
   */
  function restRect(panel: HTMLElement): DOMRect {
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
   * `set_dock_content_size` (docs/ipc/sizing-protocol.md): the value
   * across the edge is the window's thickness (the height on `bottom` and
   * `top`, the width on `left` and `right`): the panel, the margin to the
   * edge, the magnified icons' rise and the room for the pill and the
   * bounce beyond them. The value along the edge is informational: the
   * panel, its magnification growth and room for the shadow at both ends.
   */
  function reportSize(): void {
    const rest = restingPanel();
    if (rest === null) {
      return;
    }
    const lengthOf = (panelLength: number): number =>
      Math.ceil(panelLength + 2 * SIDE_SLACK + maxGrowth(slotPx, magnify));
    const rise = iconSize * (magnify.maxScale - 1);
    const width = vertical
      ? Math.ceil(rest.width + snapMargins.edge + rise + SIDE_HEADROOM)
      : lengthOf(rest.width);
    const height = vertical
      ? lengthOf(rest.height)
      : Math.ceil(rest.height + snapMargins.edge + rise + HEADROOM);
    if (width !== reported.width || height !== reported.height) {
      reported = { width, height };
      void backend
        .setDockContentSize(width, height, edge)
        .catch(report('set_dock_content_size', false));
    }
    reportHitRegion();
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
  function currentHitRegion(): HitRegion | null {
    const rest = restingPanel();
    if (rest === null) {
      return null;
    }
    const viewport = viewportNow();
    if (drag?.started) {
      const { dx, dy } = edgeShift(edge, measuredViewport, viewport);
      return {
        panel: integerRect({ ...rest, x: rest.x + dx, y: rest.y + dy }),
        envelope: { x: 0, y: 0, width: viewport.width, height: viewport.height },
      };
    }
    const region = hitRegionFor(
      rest,
      // A file drag's gap lengthens the panel by one slot: the
      // window keeps taking the drag there, magnification or not.
      maxGrowth(slotPx, magnify) + (fileDrag !== null ? slotPx : 0),
      // A magnified icon's far side is iconSize × (maxScale − 1) beyond its
      // slot's, and the slot sits PANEL_PADDING inside the panel's edge.
      magnifiedRise(iconSize, magnify.maxScale, HIT_SLACK - PANEL_PADDING),
      edge,
    );
    return autoHideHitRegion(autoHide, region, viewport, atScreenEdge, edge);
  }

  function reportHitRegion(): void {
    reportDockPanel();
    const region = currentHitRegion();
    if (region === null || (reportedHit !== null && sameHitRegion(reportedHit, region))) {
      return;
    }
    reportedHit = region;
    void backend
      .setDockHitRegion(region.panel, region.envelope, edge)
      .catch(report('set_dock_hit_region', false));
  }

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
  function reportDockPanel(): void {
    const rest = restingPanel();
    if (rest === null) {
      return;
    }
    const next = panelRectReport(edge, rest, measuredViewport, viewportNow());
    if (
      reportedPanel !== null &&
      sameRect(reportedPanel.rect, next.rect) &&
      reportedPanel.viewport.width === next.viewport.width &&
      reportedPanel.viewport.height === next.viewport.height
    ) {
      return;
    }
    reportedPanel = next;
    void backend
      .setDockPanelRect(next.rect, edge, next.viewport)
      .catch(report('set_dock_panel_rect', false));
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
  function onScaleChanged(ratio: number): void {
    const previous = dpr;
    dpr = ratio;
    if (previous === ratio) {
      return;
    }
    reported = { width: 0, height: 0 };
    reportedHit = null;
    reportedPanel = null;
    scheduleMeasure();
  }

  // Settings can change the headroom and the envelope without changing the panel box.
  $effect(() => {
    void magnify;
    void iconSize;
    reportSize();
  });

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
  let laidOutEdge: DockEdge | null = null;
  $effect(() => {
    const now = edge;
    untrack(() => {
      const before = laidOutEdge;
      laidOutEdge = now;
      if (before === null || before === now) {
        return;
      }
      measured = null;
      stripBox = null;
      reported = { width: 0, height: 0 };
      reportedHit = null;
      reportedPanel = null;
      cancelDrag(false);
      clearDragStyles();
      endFileDrag(false, false);
      leaveHoverState();
      snapMargins = { start: 0, edge: EDGE_MARGIN };
      panelEl?.style.removeProperty('margin');
      scheduleMeasure();
    });
  });

  // The work area reached the screen's edge or left it (the stock taskbar
  // auto-hidden or shown): the strip, and the region with it, may
  // come or go.
  $effect(() => {
    void atScreenEdge;
    untrack(reportHitRegion);
  });

  $effect(() => {
    const panel = panelEl;
    if (!panel) {
      return;
    }
    const observer = new ResizeObserver(scheduleMeasure);
    // The border box: what is measured, and what a padding or border
    // change (dock.css) resizes without touching the content.
    observer.observe(panel, { box: 'border-box' });
    return () => {
      observer.disconnect();
      if (measureFrame !== 0) {
        cancelAnimationFrame(measureFrame);
        measureFrame = 0;
      }
    };
  });

  // --- Auto-hide (ADR-0008) ------------------------------------------------
  // The decisions are the pure machine's (src/lib/dock/autohide.ts); this is
  // its driver: it feeds it pointer, hold, timer and settings events, runs
  // its one timer, and applies a phase change to the hit region and the
  // panel's class. CSS does the slide. Nothing runs while nothing changes.
  let autoHide: AutoHideState = INITIAL_AUTO_HIDE;
  let autoHideTimer: ReturnType<typeof setTimeout> | null = null;
  /** `autoHide` for the template: set at the end of every `autoHideSend`. */
  let autoHideView = $state.raw<AutoHideState>(INITIAL_AUTO_HIDE);
  /**
   * Auto-hide applies (`autoHideActive`: the mode is `always`, or a
   * fullscreen app is in front, or the mode is `whenOverlapped` and
   * a window covers the dock): `data-autohide` is set.
   */
  const autoHideOn = $derived(autoHideActive(autoHideView));
  /** The machine's phase, for the template (`data-autohide`). */
  const autoHidePhase = $derived(autoHideView.phase);
  /** Slid out, or sliding out. */
  const autoHidden = $derived(autoHidePhase === 'hidden');
  /**
   * The reveal strip is in the DOM (`revealStripOn`): whenever auto-hide
   * applies, except in fullscreen above a visible taskbar.
   */
  const stripOn = $derived(revealStripOn(autoHideView, atScreenEdge));
  /**
   * The resting panel's whole-pixel start and length along the dock, which
   * the reveal strip shares, and the margin between it and the window's
   * edge side, which the strip fills while the dock is on screen
   * (`revealStripThickness`).
   */
  let stripBox = $state<{ start: number; length: number; gap: number } | null>(null);
  /** Native menus from this dock that have not closed yet (`show_*_menu` pending). */
  let menusOpen = 0;
  let bounceHoldTimer: ReturnType<typeof setTimeout> | null = null;
  let bounceSeen = 0;

  function autoHideSend(event: AutoHideEvent): void {
    if (!autoHideActive(autoHide) && autoHideActive(autoHideStep(autoHide, event).state)) {
      // Auto-hide comes on (the mode, a fullscreen app, an overlap). The
      // machine forgot the pointer when it last went off, and a pointer
      // resting on the panel since then has said nothing: the panel's
      // `:hover` says where it is, so the dock does not hide from under it.
      const inside = panelEl?.matches(':hover') ?? false;
      if (inside !== autoHide.pointerInside) {
        autoHideSend({ type: 'pointer', inside });
      }
    }
    const before = autoHide;
    const { state, timer } = autoHideStep(before, event);
    autoHide = state;
    if (timer !== 'none' && autoHideTimer !== null) {
      clearTimeout(autoHideTimer);
      autoHideTimer = null;
    }
    if (timer === 'arm') {
      autoHideTimer = setTimeout(() => {
        autoHideTimer = null;
        autoHideSend({ type: 'timeout' });
      }, hideDelayMs(state));
    }
    const wasHidden = isAutoHidden(before);
    const hidden = isAutoHidden(state);
    if (hidden && !wasHidden) {
      // Out: nothing magnified or pending stays behind, and from now on the
      // window takes the pointer only in the strip.
      leaveHoverState();
      reportHitRegion();
      scheduleMeasure();
    } else if (wasHidden && !hidden) {
      // Back: the normal region goes out before the slide starts (the class
      // below changes the DOM after this returns), so the panel never slides
      // in over a click-through window.
      reportHitRegion();
    } else if (
      autoHideActive(state) !== autoHideActive(before) ||
      revealStripOn(state, atScreenEdge) !== revealStripOn(before, atScreenEdge)
    ) {
      reportHitRegion();
    }
    if (hidden !== wasHidden) {
      // The app layer keeps the fact: a tray click or the toggle
      // shortcut then brings a slid-out panel back (`dock://show`) instead
      // of hiding the window. Housekeeping: a failure only logs.
      void backend.setDockAutoHidden(hidden).catch(report('set_dock_auto_hidden', false));
    }
    autoHideView = state;
  }

  /** The panel's own slide or fade ended (not a slot's transition bubbling up). */
  function onpaneltransitionend(e: TransitionEvent): void {
    if (e.target === e.currentTarget) {
      scheduleMeasure();
    }
  }

  function onstripenter(): void {
    autoHideSend({ type: 'strip' });
  }

  function onstripleave(): void {
    autoHideSend({ type: 'pointer', inside: false });
  }

  /**
   * The drag is over. No `pointerleave` may ever come (the lifted icon was
   * under the pointer, and the window shrinks back), so where the
   * pointer is is worked out from the release point: over the panel, or
   * between it and the strip, counts as inside.
   */
  function endDragHold(d: DragSession): void {
    autoHideSend({ type: 'pointer', inside: isOnDock(d.lastX, d.lastY) });
    autoHideSend({ type: 'hold', hold: 'drag', active: false });
  }

  /**
   * (`x`, `y`) is over the panel, or between it and the window's edge side
   * while the reveal strip is there to report the pointer leaving
   * (`onDockAt`; not in fullscreen above a visible taskbar): where
   * the pointer counts as on the dock after a drag that produced no pointer
   * event to say so.
   */
  function isOnDock(x: number, y: number): boolean {
    const rect = panelEl?.getBoundingClientRect();
    return (
      rect !== undefined &&
      onDockAt(rect, x, y, viewportNow(), revealStripOn(autoHide, atScreenEdge), edge)
    );
  }

  function menuOpened(): void {
    menusOpen += 1;
    if (menusOpen === 1) {
      autoHideSend({ type: 'hold', hold: 'menu', active: true });
    }
  }

  function menuClosed(): void {
    menusOpen = Math.max(0, menusOpen - 1);
    if (menusOpen === 0) {
      autoHideSend({ type: 'hold', hold: 'menu', active: false });
    }
  }

  /** An item bounces for a launch the dock started: stay up until it lands. */
  function holdForBounce(): void {
    if (bounceHoldTimer !== null) {
      clearTimeout(bounceHoldTimer);
    }
    autoHideSend({ type: 'hold', hold: 'bounce', active: true });
    bounceHoldTimer = setTimeout(() => {
      bounceHoldTimer = null;
      autoHideSend({ type: 'hold', hold: 'bounce', active: false });
    }, BOUNCE_MS);
  }

  $effect(() => {
    const mode = settings?.autoHide.mode ?? 'off';
    const delayMs = settings?.autoHide.delayMs ?? 0;
    untrack(() => autoHideSend({ type: 'configure', mode, delayMs }));
  });

  // Only a bounce that plays holds the dock (none under reduced motion or
  // with the bounce turned off).
  $effect(() => {
    const serial = store.bounceSerial;
    if (serial === bounceSeen) {
      return;
    }
    bounceSeen = serial;
    untrack(() => {
      if (bounceEnabled) {
        holdForBounce();
      }
    });
  });

  // An app asking for attention brings a hidden dock back and keeps
  // it up until no item needs attention any more, whatever the motion
  // settings: the wiggle may be off, the item still has to be seen. From the
  // snapshot's items for this monitor, not the rendered list a file drop may
  // be holding.
  const needsAttention = $derived(visibleItems.some((i) => i.needsAttention));
  $effect(() => {
    const active = needsAttention;
    untrack(() => autoHideSend({ type: 'hold', hold: 'attention', active }));
  });

  // A fullscreen app owns this dock's monitor (the platform decides,
  // the snapshot carries it per dock): the dock gets out of the way
  // whatever the auto-hide mode, and comes back when the app leaves the
  // foreground or fullscreen.
  const fullscreen = $derived(facts.fullscreen);
  $effect(() => {
    const active = fullscreen;
    untrack(() => autoHideSend({ type: 'fullscreen', active }));
  });

  // A window of another process covers this dock's resting panel
  // (the engine decides from the frames the platform reports and the panel
  // rect sent by `reportDockPanel`): with the mode `whenOverlapped` the dock
  // hides after the delay and comes back when the window is gone.
  const overlapped = $derived(facts.overlapped);
  $effect(() => {
    const active = overlapped;
    untrack(() => autoHideSend({ type: 'overlapped', active }));
  });

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
  /** Longest wait for the snapshot with the dropped apps in place; the gap closes after it anyway. */
  const LANDING_TIMEOUT_MS = 1500;

  interface FileDragSession {
    /** Every path of the drag, in its order (`fileDropAction` decides what they are good for). */
    paths: string[];
    /** The latest position (viewport CSS px). */
    x: number;
    y: number;
    frame: number;
  }

  interface Landing {
    /** The pinned order sent with `reorder_pinned`. */
    order: AppId[];
    /** The first newly pinned item: it takes the gap's place. */
    key: string | null;
    timer: ReturnType<typeof setTimeout>;
  }

  let fileDrag: FileDragSession | null = null;
  /** Where the drop gap is drawn (`gapAnchor`), or `null`. */
  let dropGap = $state.raw<GapAnchor | null>(null);
  /** The item a drop would land on instead of the gap (the Recycle Bin), highlighted; or `null`. */
  let dropTargetKey = $state<string | null>(null);
  let landing: Landing | null = null;
  /** The item that took the gap's place: it does not grow in, and the gap does not shrink away. */
  let landedKey = $state<string | null>(null);

  function onfiledrag(e: FileDragEvent): void {
    if (drag?.started) {
      // A lifted icon owns the dock (an OLE drag holds the mouse, so this
      // should never happen).
      return;
    }
    switch (e.type) {
      case 'enter':
        beginFileDrag(e.paths, e.position);
        return;
      case 'over':
        if (fileDrag !== null) {
          fileDrag.x = e.position.x;
          fileDrag.y = e.position.y;
          scheduleFileDragFrame();
        }
        return;
      case 'drop':
        dropFiles(e.paths, e.position);
        return;
      case 'leave':
        // Out of the window, or cancelled: the pointer is away.
        endFileDrag(false, false);
        return;
    }
  }

  function beginFileDrag(paths: readonly string[], at: CssPoint): void {
    // A session whose leave never came must not linger.
    endFileDrag(false, false);
    fileDrag = { paths: [...paths], x: at.x, y: at.y, frame: 0 };
    // Nothing magnified: the gap's place is found from where the slots are drawn.
    leaveHoverState();
    autoHideSend({ type: 'hold', hold: 'fileDrag', active: true });
    // The envelope makes room for the gap (`currentHitRegion`).
    reportHitRegion();
    applyFileDrag();
  }

  function scheduleFileDragFrame(): void {
    if (fileDrag !== null && fileDrag.frame === 0) {
      fileDrag.frame = requestAnimationFrame(applyFileDrag);
    }
  }

  /** Open, move or close the gap, or highlight the bin, for where the drag is now. */
  function applyFileDrag(): void {
    const d = fileDrag;
    if (d === null) {
      return;
    }
    if (d.frame !== 0) {
      cancelAnimationFrame(d.frame);
      d.frame = 0;
    }
    if (isAutoHidden(autoHide) && inRevealStrip(d.x, d.y)) {
      // Like the pointer touching the strip: the dock comes up for the drop.
      autoHideSend({ type: 'strip' });
    }
    const action = fileDropAction(fileDragSpot(d.x, d.y), d.paths);
    showGap(action.kind === 'pin' ? gapAnchor(items, action.index) : null);
    dropTargetKey = action.kind === 'recycle' ? action.key : null;
  }

  /** Over the reveal strip (ADR-0008), where a hidden dock takes the pointer; never without one. */
  function inRevealStrip(x: number, y: number): boolean {
    if (stripBox === null || !revealStripOn(autoHide, atScreenEdge)) {
      return false;
    }
    const { start, length } = stripBox;
    const resting = vertical
      ? { x: 0, y: start, width: 0, height: length }
      : { x: start, y: 0, width: length, height: 0 };
    return rectContainsPoint(revealStrip(resting, viewportNow(), edge), x, y);
  }

  /**
   * Where a file drag at (`x`, `y`) points, measured where everything is
   * drawn now (an open gap included). The item under the pointer and the
   * gap's index are separate answers (`fileDropAction`).
   */
  function fileDragSpot(x: number, y: number): FileDragSpot {
    const panel = panelEl;
    if (!panel || isAutoHidden(autoHide)) {
      return { onPanel: false, over: null, index: 0 };
    }
    const spans: ItemSpan[] = [];
    const centres: number[] = [];
    for (const el of slots) {
      const rect = el.getBoundingClientRect();
      spans.push({
        key: el.dataset.key ?? '',
        // `data-kind` is the item's `DockItem.kind` (DockItem.svelte and the separator slot).
        kind: (el.dataset.kind ?? 'separator') as DockItemKind,
        ...spanAlong(axis, rect),
      });
      if (el.dataset.kind === 'app' && el.dataset.pinned === 'true') {
        centres.push(centreAlong(axis, rect));
      }
    }
    const pos = along(axis, x, y);
    return {
      onPanel: rectContainsPoint(panel.getBoundingClientRect(), x, y),
      over: dropTargetAt(pos, spans),
      index: dropIndex(pos, centres),
    };
  }

  function showGap(anchor: GapAnchor | null): void {
    if (!sameAnchor(dropGap, anchor)) {
      dropGap = anchor;
    }
  }

  function dropFiles(paths: readonly string[], at: CssPoint): void {
    const action = fileDropAction(fileDragSpot(at.x, at.y), paths);
    // Released on the dock, the pointer is there now; no `pointerenter` will say so.
    endFileDrag(isOnDock(at.x, at.y), action.kind === 'pin');
    if (action.kind === 'recycle') {
      void recycleDropped(paths);
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
    finishLanding(false);
    showGap(gapAnchor(items, action.index));
    void pinDropped(candidates, action.index);
  }

  /**
   * The drag is over: say where the pointer is now, release the hold, and
   * close the gap unless a drop is about to land in it (`keepGap`) or still
   * landing.
   */
  function endFileDrag(pointerOnDock: boolean, keepGap: boolean): void {
    const d = fileDrag;
    if (d === null) {
      return;
    }
    fileDrag = null;
    if (d.frame !== 0) {
      cancelAnimationFrame(d.frame);
    }
    if (!keepGap && frozenItems === null) {
      showGap(null);
    }
    dropTargetKey = null;
    autoHideSend({ type: 'pointer', inside: pointerOnDock });
    autoHideSend({ type: 'hold', hold: 'fileDrag', active: false });
    reportHitRegion();
  }

  /**
   * Pin each candidate, then put the new ids at `index` in one reorder. A
   * failed pin is reported like any failed command and the others go on.
   */
  async function pinDropped(candidates: readonly PinCandidate[], index: number): Promise<void> {
    const before = pinnedIds(store.state?.items ?? []);
    frozenItems = items;
    const ids: AppId[] = [];
    for (const candidate of candidates) {
      try {
        ids.push(await backend.pinTarget(candidate.target));
      } catch (err) {
        report(`Pinning ${fileNameOf(candidate.path)}`)(err);
      }
    }
    if (ids.length === 0) {
      finishLanding(false);
      return;
    }
    const order = orderWithDropped(before, ids, index);
    const fresh = ids.find((id) => !before.includes(id));
    landing = {
      order,
      key: fresh === undefined ? null : `app:${fresh}`,
      timer: setTimeout(() => finishLanding(false), LANDING_TIMEOUT_MS),
    };
    try {
      await backend.reorderPinned(order);
    } catch (err) {
      report('Reordering')(err);
      finishLanding(false);
      return;
    }
    checkLanding();
  }

  /**
   * Send a drop on the Recycle Bin to the bin. Rust sorts the paths and the
   * shell may ask the user first, so this can take a while; a refusal is
   * reported like any failed command.
   */
  async function recycleDropped(paths: readonly string[]): Promise<void> {
    try {
      await backend.deleteToRecycleBin([...paths]);
    } catch (err) {
      report(`Moving ${recycleSubject(paths)} to the Recycle Bin`)(err);
    }
  }

  /** The snapshot shows the dropped apps in their place: render it. */
  function checkLanding(): void {
    const l = landing;
    const state = store.state;
    if (l !== null && state !== null && sameOrder(pinnedIds(state.items), l.order)) {
      finishLanding(true);
    }
  }

  /**
   * Show the snapshot again and close the gap: at once when an item takes
   * its place (`landed`), shrinking away otherwise (a failure, the timeout).
   */
  function finishLanding(landed: boolean): void {
    const l = landing;
    landing = null;
    if (l !== null) {
      clearTimeout(l.timer);
    }
    if (frozenItems === null) {
      return;
    }
    landedKey = landed && l !== null ? l.key : null;
    frozenItems = null;
    showGap(null);
    scheduleMeasure();
    if (landedKey !== null) {
      // The intro and outro decisions are taken in this update; forget the key after it.
      requestAnimationFrame(() => {
        landedKey = null;
      });
    }
    if (fileDrag !== null) {
      // Another drag came in meanwhile: its gap, in the list now shown.
      scheduleFileDragFrame();
    }
  }

  function ongapoutroend(): void {
    scheduleMeasure();
  }

  $effect(() => {
    void store.state;
    untrack(checkLanding);
  });

  // --- Keyboard -----------------------------------------------------------------
  // The toolbar is one tab stop with a roving tabindex: the item the focus
  // was on last has tabindex 0, every other one -1 (`tabStopKey`). Arrows,
  // Home and End move the focus (`nextFocusIndex`, src/lib/dock/keyboard.ts,
  // pure); Enter, Space and the menu keys are the item's own (DockItem). An
  // item with keyboard focus (`:focus-visible`) shows its title pill as
  // hover does, without magnifying, and holds auto-hide (`focus`). Left and
  // middle clicks never focus an item (DockItem prevents their mouse down);
  // a right-click may, but that focus is not `:focus-visible`, so it moves
  // the tab stop and nothing else.
  /** The item the focus was on last: the toolbar's tab stop while it is still there. */
  let focusedKey = $state<string | null>(null);
  /** The item that has keyboard focus (`:focus-visible`), or `null`. */
  let keyboardKey = $state<string | null>(null);
  const tabStop = $derived(tabStopKey(items, focusedKey));
  /**
   * The focused item's pill; the pointer's, a lifted icon and a file drag
   * over the bin come first (one pill at a time).
   */
  const focusTooltipKey = $derived(
    tooltipsEnabled && tooltipKey === null && liftedKey === null && dropTargetKey === null
      ? keyboardKey
      : null,
  );

  /** `target` is an item's slot in this panel (not a separator, not the panel itself). */
  function itemSlotOf(target: EventTarget | null): HTMLElement | null {
    if (
      !(target instanceof HTMLElement) ||
      target.dataset.key === undefined ||
      target.dataset.kind === 'separator' ||
      !panelEl?.contains(target)
    ) {
      return null;
    }
    return target;
  }

  /**
   * The options of `HTMLElement.focus` with `focusVisible` (HTML; not in
   * TypeScript's DOM types yet): `true` asks the browser to show the focus
   * as keyboard focus whatever its heuristics say.
   */
  type VisibleFocusOptions = NonNullable<Parameters<HTMLElement['focus']>[0]> & {
    focusVisible?: boolean;
  };

  /**
   * Move the focus to the item `key`. `visible` (the focus shortcut):
   * show it as keyboard focus even when the page's last input was
   * the pointer. Chromium decides `:focus-visible` for a script's `focus()`
   * by the page's last input, and the shortcut's keys never reach the page
   * (Windows takes them), so after any click on the dock the item would get
   * the focus without the ring, the pill or the auto-hide hold.
   */
  function focusItem(key: string, visible = false): void {
    focusedKey = key;
    const options: VisibleFocusOptions = visible ? { focusVisible: true } : {};
    slots.find((el) => el.dataset.key === key)?.focus(options);
  }

  function onpanelkeydown(e: KeyboardEvent): void {
    if (e.altKey || e.ctrlKey || e.metaKey || e.shiftKey || drag?.started) {
      return;
    }
    const key = itemSlotOf(e.target)?.dataset.key;
    const current = key === undefined ? -1 : items.findIndex((i) => i.key === key);
    const next = nextFocusIndex(items, current, e.key, axis);
    if (next === null) {
      return;
    }
    e.preventDefault();
    focusItem(items[next]!.key);
  }

  // The focus shortcut (docs/ipc/keyboard-entry.md): Rust has made
  // this dock window the foreground window. The keyboard lands on the
  // active app's item, else the first (`entryKey`), the way Tab would
  // bring it there: the ring, the title pill and the `focus` hold follow
  // from `focusin`. A panel still slid out (a press that crossed the slide,
  // which Rust did not see yet) comes back first: the hold keeps a dock up
  // but never reveals one. Either way the focus waits for the DOM to catch
  // up, since a panel the page has just brought back (here, or for the
  // `dock://show` before this event) is still hidden until then, and a
  // hidden panel takes no focus.
  async function focusEntry(): Promise<void> {
    if (autoHidden) {
      autoHideSend({ type: 'show' });
    }
    await tick();
    const key = entryKey(items);
    if (key !== null) {
      focusItem(key, true);
    }
  }

  function onpanelfocusin(e: FocusEvent): void {
    const slot = itemSlotOf(e.target);
    if (slot === null) {
      return;
    }
    focusedKey = slot.dataset.key ?? null;
    keyboardKey = slot.matches(':focus-visible') ? focusedKey : null;
    autoHideSend({ type: 'hold', hold: 'focus', active: keyboardKey !== null });
  }

  function onpanelfocusout(e: FocusEvent): void {
    if (itemSlotOf(e.relatedTarget) !== null) {
      // On to another item: its `focusin` follows.
      return;
    }
    const key = itemSlotOf(e.target)?.dataset.key;
    if (key !== undefined && !items.some((i) => i.key === key)) {
      // The item is being removed: the effect below moves the focus on.
      return;
    }
    releaseKeyboardFocus();
  }

  function releaseKeyboardFocus(): void {
    keyboardKey = null;
    autoHideSend({ type: 'hold', hold: 'focus', active: false });
  }

  // The item with the focus left the dock (its app's last window closed):
  // the focus moves on to its neighbour (`successorKey`), so the keyboard
  // stays in the toolbar, and the tab stop goes with it. After the slots
  // effect above, which has collected the new slots by then.
  let renderedItems: readonly DockItemModel[] = [];
  $effect(() => {
    const now = items;
    untrack(() => {
      const before = renderedItems;
      renderedItems = now;
      const key = focusedKey;
      if (key === null || now.some((i) => i.key === key)) {
        return;
      }
      const active = document.activeElement;
      const hadFocus =
        keyboardKey === key || (active instanceof HTMLElement && active.dataset.key === key);
      const next = successorKey(before, now, key);
      focusedKey = next;
      if (!hadFocus) {
        return;
      }
      const el = next === null ? undefined : slots.find((s) => s.dataset.key === next);
      if (el !== undefined) {
        el.focus();
        return;
      }
      if (active instanceof HTMLElement && panelEl?.contains(active)) {
        active.blur();
      }
      releaseKeyboardFocus();
    });
  });

  // --- Lifecycle ------------------------------------------------------------------
  onMount(() => {
    const dark = window.matchMedia('(prefers-color-scheme: dark)');
    const motion = window.matchMedia('(prefers-reduced-motion: reduce)');
    prefersDark = dark.matches;
    reducedMotion = motion.matches;
    const stopScale = watchDevicePixelRatio(window, onScaleChanged);
    const onDark = (): void => {
      prefersDark = dark.matches;
    };
    const onMotion = (): void => {
      reducedMotion = motion.matches;
    };
    // The tray and the toggle shortcut show a hidden dock window; shown
    // again while auto-hidden, the dock comes back (ADR-0008). With the
    // window visible and the panel slid out they ask for the panel instead
    // (`dock://show`): the same reveal, without a window round trip.
    const onVisibility = (): void => {
      if (document.visibilityState === 'visible') {
        autoHideSend({ type: 'show' });
      }
    };
    const onShowRequested = (): void => {
      autoHideSend({ type: 'show' });
    };
    const onFocusRequested = (): void => {
      void focusEntry();
    };
    // A second press of the focus shortcut while this dock has the keyboard:
    // what Escape does. A pointer drag in progress is the
    // pointer's and stays.
    const onReleaseRequested = (): void => {
      letGo();
    };
    dark.addEventListener('change', onDark);
    motion.addEventListener('change', onMotion);
    document.addEventListener('visibilitychange', onVisibility);
    const offShow = backend.on('dock://show', onShowRequested);
    const offFocus = backend.on('dock://focus', onFocusRequested);
    const offRelease = backend.on('dock://release-focus', onReleaseRequested);
    const offFileDrag = backend.onFileDrag(onfiledrag);
    // The user's dock.css: subscribe, then ask, like the store's
    // startup sequence; an event that arrives first wins over the answer.
    let mounted = true;
    const offUserCss = backend.on('dock://user-css', (css) => {
      userCss = nextUserCss(userCss, 'event', css);
    });
    backend.getUserCss().then(
      (css) => {
        if (mounted) {
          userCss = nextUserCss(userCss, 'fetch', css);
        }
      },
      (err: unknown) => console.error('get_user_css failed:', err),
    );
    void store.start();
    return () => {
      mounted = false;
      offUserCss();
      applyUserCss(document, null);
      offShow();
      offFocus();
      offRelease();
      offFileDrag();
      endFileDrag(false, false);
      finishLanding(false);
      dark.removeEventListener('change', onDark);
      motion.removeEventListener('change', onMotion);
      stopScale();
      document.removeEventListener('visibilitychange', onVisibility);
      clearTooltip();
      resetMagnification();
      if (settleTimer !== null) {
        clearTimeout(settleTimer);
      }
      if (autoHideTimer !== null) {
        clearTimeout(autoHideTimer);
      }
      if (bounceHoldTimer !== null) {
        clearTimeout(bounceHoldTimer);
      }
      cancelDrag(false);
      clearDragStyles();
      toasts.clear();
      store.stop();
    };
  });
</script>

{#snippet gap()}
  <!-- The drop gap: room for the files a drop will pin, with an
       outline where the first new icon will sit. -->
  <div
    class="drop-gap"
    aria-hidden="true"
    in:gapTransition|global={{ enabled: itemMotion, axis }}
    out:gapTransition|global={{ enabled: itemMotion && landedKey === null, axis }}
    onoutroend={ongapoutroend}
  >
    <div class="drop-indicator"></div>
  </div>
{/snippet}

<svelte:window
  onkeydown={onwindowkeydown}
  onpointermove={onwindowpointermove}
  onpointerup={onwindowpointerup}
  onpointercancel={onwindowpointercancel}
  onresize={onwindowresize}
  oncontextmenu={onwindowcontextmenu}
/>

<div
  class="dock-stage"
  data-edge={edge}
  data-theme={preset}
  style:--accent={accent}
  style:--panel-opacity={opacity}
  style:--radius="{cornerRadius}px"
  style:--icon="{iconSize}px"
  style:--slot-gap="{SLOT_GAP}px"
  style:--panel-padding="{PANEL_PADDING}px"
  style:--dots-gap="{DOTS_GAP}px"
  style:--dot-size="{DOT_SIZE}px"
  style:--separator-margin="{SEPARATOR_MARGIN}px"
  style:--edge-margin="{EDGE_MARGIN}px"
  style:--pill-max="{PILL_MAX_WIDTH}px"
  style:--shadow-room="{SIDE_SLACK}px"
  style:--slide="{AUTO_HIDE_SLIDE_MS}ms"
>
  {#if store.state !== null && settings !== null}
    <div
      class="dock-panel"
      class:settling
      class:high-contrast={highContrast}
      class:slid-out={autoHidden}
      bind:this={panelEl}
      role="toolbar"
      aria-label="Stem"
      aria-orientation={axis}
      tabindex="-1"
      data-ready="true"
      data-axis={axis}
      data-autohide={autoHideOn ? autoHidePhase : undefined}
      onpointermove={onpanelpointermove}
      onpointerenter={onpanelpointerenter}
      onpointerleave={onpanelpointerleave}
      onwheel={onpanelwheel}
      ontransitionend={onpaneltransitionend}
      oncontextmenu={onpanelcontextmenu}
      onkeydown={onpanelkeydown}
      onfocusin={onpanelfocusin}
      onfocusout={onpanelfocusout}
    >
      {#each items as item (item.key)}
        {#if dropGap !== null && dropGap.key === item.key && dropGap.side === 'before'}
          {@render gap()}
        {/if}
        {#if item.kind === 'separator'}
          <div
            class="slot separator"
            role="separator"
            aria-orientation={vertical ? 'horizontal' : 'vertical'}
            data-key={item.key}
            data-kind="separator"
            in:itemTransition|global={{ enabled: itemMotion && introsEnabled, axis }}
            out:itemTransition|global={{ enabled: itemMotion, axis }}
            onintroend={onItemSettled}
            onoutroend={onItemSettled}
          ></div>
        {:else}
          <DockItem
            {item}
            iconUrl={item.iconKey !== null
              ? iconUrlAt(item.iconKey, iconSizes.rest, previousIconSizes?.rest)
              : null}
            magnifiedIconUrl={item.iconKey !== null && dualSources
              ? iconUrlAt(item.iconKey, iconSizes.magnified, previousIconSizes?.magnified)
              : null}
            {showDots}
            tooltip={tooltipKey === item.key ||
              focusTooltipKey === item.key ||
              (dropTargetKey === item.key && tooltipsEnabled)}
            dropTarget={dropTargetKey === item.key}
            power={store.state.power}
            bounce={bounceEnabled && store.bounceKey === item.key ? store.bounceSerial : 0}
            animate={itemMotion}
            enter={introsEnabled && item.key !== landedKey}
            lifted={liftedKey === item.key}
            {edge}
            tabStop={tabStop === item.key}
            {onprimary}
            {onmiddle}
            {oncontext}
            {onhover}
            {onpress}
            onsettled={onItemSettled}
          />
        {/if}
        {#if dropGap !== null && dropGap.key === item.key && dropGap.side === 'after'}
          {@render gap()}
        {/if}
      {/each}
      {#if dropGap !== null && dropGap.key === null}
        {@render gap()}
      {/if}
    </div>
    {#if stripOn && stripBox !== null}
      <!-- The reveal strip (ADR-0008): the window's edge side beside the
           resting panel (its bottom on `bottom`, its left side on `left`).
           Hidden, the window takes the pointer only in its 2 px
           there; shown, it fills the margin between the panel and that side
           and counts as part of the dock, so leaving it towards the panel is
           entering the panel. None in fullscreen above a visible
           taskbar. Invisible. -->
      {@const thickness = `${revealStripThickness(autoHidePhase, stripBox.gap)}px`}
      <div
        class="reveal-strip"
        aria-hidden="true"
        style:left={vertical ? undefined : `${stripBox.start}px`}
        style:top={vertical ? `${stripBox.start}px` : undefined}
        style:width={vertical ? thickness : `${stripBox.length}px`}
        style:height={vertical ? `${stripBox.length}px` : thickness}
        onpointerenter={onstripenter}
        onpointerleave={onstripleave}
      ></div>
    {/if}
  {/if}

  <!-- Failure notices: at the far end of the window just beyond the panel's
       side away from the edge (right-aligned just above it on `bottom`),
       never in the way; while the panel is slid out they sit just inside
       the window's edge side and ride back out with it. -->
  <div class="toasts" class:at-edge={autoHidden} aria-live="polite">
    {#each toasts.items as toast (toast.id)}
      <div class="toast" role="status">{toast.message}</div>
    {/each}
  </div>
</div>

<style>
  /* The panel is anchored to the window's edge side and centred along it
     (docs/ipc/sizing-protocol.md). */
  .dock-stage {
    position: relative;
    display: flex;
    align-items: flex-end;
    justify-content: center;
    width: 100%;
    height: 100%;
    color: var(--text);
    /* The panel's thickness (icon + padding) plus its margin to the edge:
       where its side away from the edge is. */
    --panel-line: calc(var(--edge-margin) + var(--icon) + 2 * var(--panel-padding));

    /* Theme presets (docs/THEMING.md). The panel, the separators,
       the dots and the title pills read the stable `--stem-*` variables
       with these as the fallback, so the user's dock.css sets a variable
       anywhere above them and it applies; nothing here defines a
       `--stem-*` variable. `standard` (today's look) is these values, from
       the colour tokens of app.css; a contrast theme keeps them for every
       preset (its tokens are solid, `.high-contrast` drops the blur). */
    --preset-panel-fill: rgb(var(--panel-tint) / var(--panel-opacity));
    --preset-panel-blur: blur(24px) saturate(1.4);
    --preset-panel-hairline: var(--panel-border);
    --preset-panel-shadow: var(--panel-shadow);
    /* Corner radius is a setting in every preset. */
    --preset-panel-radius: var(--radius);
    --preset-separator: var(--separator);
    --preset-dot: var(--accent);
    --preset-pill-fill: var(--tooltip-bg);
    --preset-pill-text: var(--text);
    --preset-pill-border: var(--tooltip-border);
  }

  /* `solid`: the panel opaque, in the colours Windows 11 gives a flyout
     with *Transparency effects* off (#2c2c2c, #f9f9f9), with no blur, the
     flyout's stronger stroke as the hairline and a tighter shadow; the
     title pills the same. Readable over any wallpaper. */
  :global(html:not([data-high-contrast])) .dock-stage[data-theme='solid'] {
    --preset-panel-fill: rgb(44 44 44);
    --preset-panel-blur: none;
    --preset-panel-hairline: rgb(117 117 117 / 0.4);
    --preset-panel-shadow: 0 8px 24px rgb(0 0 0 / 0.42), 0 2px 6px rgb(0 0 0 / 0.28);
    --preset-separator: rgb(255 255 255 / 0.28);
    --preset-pill-fill: rgb(44 44 44);
    --preset-pill-border: rgb(117 117 117 / 0.4);
  }

  :global(html[data-theme='light']:not([data-high-contrast])) .dock-stage[data-theme='solid'] {
    --preset-panel-fill: rgb(249 249 249);
    --preset-panel-shadow: 0 8px 24px rgb(0 0 0 / 0.16), 0 2px 6px rgb(0 0 0 / 0.1);
    --preset-separator: rgb(0 0 0 / 0.22);
    --preset-pill-fill: rgb(249 249 249);
  }

  /* `minimal`: no panel at all, only the icons, the dots and the
     separators, which sit on the wallpaper and so take more contrast. The
     panel keeps its box (padding, hit region, size), so a preset change
     never moves an icon. */
  :global(html:not([data-high-contrast])) .dock-stage[data-theme='minimal'] {
    --preset-panel-fill: transparent;
    --preset-panel-blur: none;
    --preset-panel-hairline: transparent;
    --preset-panel-shadow: none;
    --preset-separator: rgb(255 255 255 / 0.5);
  }

  :global(html[data-theme='light']:not([data-high-contrast])) .dock-stage[data-theme='minimal'] {
    --preset-separator: rgb(0 0 0 / 0.4);
  }

  /* The dots and the title pills are DockItem's: they read the accent and
     the tooltip tokens, which take the variables here, for their subtree
     only (the drop outline and the Start icon keep the accent). */
  .dock-panel :global(.dots) {
    --accent: var(--stem-dot, var(--preset-dot));
  }

  .dock-panel :global(.tooltip) {
    --tooltip-bg: var(--stem-pill-fill, var(--preset-pill-fill));
    --tooltip-border: var(--preset-pill-border);
    --text: var(--stem-pill-text, var(--preset-pill-text));
  }

  .dock-stage[data-edge='top'] {
    align-items: flex-start;
  }

  .dock-stage[data-edge='left'] {
    align-items: center;
    justify-content: flex-start;
  }

  .dock-stage[data-edge='right'] {
    align-items: center;
    justify-content: flex-end;
  }

  .toasts {
    position: absolute;
    left: 8px;
    right: 8px;
    /* The panel's top edge plus a small gap. */
    bottom: calc(var(--panel-line) + 6px);
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 4px;
    pointer-events: none;
    /* Follows the panel's slide: towards the edge by the panel box
       and the margin to the edge, so the notices end 6 px inside the edge
       side while the panel is out, and back with it. A transform, like the
       panel's own slide, so nothing is laid out again. */
    transition: transform var(--slide) ease-out;
  }

  .toasts.at-edge {
    transform: translateY(var(--panel-line));
    transition: transform var(--slide) ease-in;
  }

  /* On `top`: right-aligned just below the panel, the newest nearest it. */
  .dock-stage[data-edge='top'] .toasts {
    bottom: auto;
    top: calc(var(--panel-line) + 6px);
    flex-direction: column-reverse;
  }

  .dock-stage[data-edge='top'] .toasts.at-edge {
    transform: translateY(calc(-1 * var(--panel-line)));
  }

  /* On a side edge: at the bottom end of the window, just beside the panel. */
  .dock-stage[data-edge='left'] .toasts,
  .dock-stage[data-edge='right'] .toasts {
    top: 8px;
    bottom: 8px;
    justify-content: flex-end;
  }

  .dock-stage[data-edge='left'] .toasts {
    left: calc(var(--panel-line) + 6px);
    align-items: flex-start;
  }

  .dock-stage[data-edge='left'] .toasts.at-edge {
    transform: translateX(calc(-1 * var(--panel-line)));
  }

  .dock-stage[data-edge='right'] .toasts {
    right: calc(var(--panel-line) + 6px);
  }

  .dock-stage[data-edge='right'] .toasts.at-edge {
    transform: translateX(var(--panel-line));
  }

  .toast {
    box-sizing: border-box;
    max-width: 100%;
    padding: 5px 10px 5px 22px;
    position: relative;
    border-radius: 7px;
    border: 1px solid var(--tooltip-border);
    background: var(--tooltip-bg);
    color: var(--text);
    font-size: 12px;
    font-weight: 500;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    box-shadow: 0 3px 10px rgba(0, 0, 0, 0.28);
    animation: toast-in 160ms ease-out;
  }

  .toast::before {
    content: '';
    position: absolute;
    left: 9px;
    top: 50%;
    width: 7px;
    height: 7px;
    margin-top: -3.5px;
    border-radius: 50%;
    background: #f7630c;
  }

  @keyframes toast-in {
    from {
      opacity: 0;
      transform: translateY(4px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .toast {
      animation: none;
    }

    /* Both: `.toasts.at-edge` alone would win over `.toasts` and slide. */
    .toasts,
    .toasts.at-edge {
      transition: none;
    }
  }

  /* Content-sized and centred by the stage: items animating in and out
     lengthen and shorten it without the window ever resizing
     (docs/ipc/sizing-protocol.md). A row on `bottom` and `top`, a column on
     `left` and `right`. `measurePanel` sets inline margins (the start of
     the main axis and the edge side: `margin-left` / `margin-bottom` on
     `bottom`) on top of these to hold the panel on whole device pixels. */
  .dock-panel {
    position: relative;
    display: flex;
    align-items: flex-end;
    flex: none;
    box-sizing: border-box;
    padding: var(--panel-padding);
    margin-bottom: var(--edge-margin);
    border-radius: var(--stem-panel-radius, var(--preset-panel-radius));
    background: var(--stem-panel-fill, var(--preset-panel-fill));
    -webkit-backdrop-filter: var(--stem-panel-blur, var(--preset-panel-blur));
    backdrop-filter: var(--stem-panel-blur, var(--preset-panel-blur));
    box-shadow: var(--stem-panel-shadow, var(--preset-panel-shadow));
  }

  /* Hairline border drawn inside the box so it never affects layout. */
  .dock-panel::before {
    content: '';
    position: absolute;
    inset: 0;
    border-radius: inherit;
    border: 1px solid var(--stem-panel-hairline, var(--preset-panel-hairline));
    pointer-events: none;
  }

  .dock-stage[data-edge='top'] .dock-panel {
    align-items: flex-start;
    margin-bottom: 0;
    margin-top: var(--edge-margin);
  }

  .dock-stage[data-edge='left'] .dock-panel,
  .dock-stage[data-edge='right'] .dock-panel {
    flex-direction: column;
    margin-bottom: 0;
  }

  .dock-stage[data-edge='left'] .dock-panel {
    align-items: flex-start;
    margin-left: var(--edge-margin);
  }

  .dock-stage[data-edge='right'] .dock-panel {
    align-items: flex-end;
    margin-right: var(--edge-margin);
  }

  /* Auto-hide (ADR-0008): the panel slides out past the window's
     edge side, by its own thickness plus the margin to the edge and room
     for its shadow, and back (ease-in out, ease-out in): down on `bottom`,
     up on `top`, left on `left`, right on `right`. The window
     itself never moves. Out of view it takes neither the pointer nor
     keyboard focus. Under reduced motion it fades instead. */
  .dock-panel {
    transition: transform var(--slide) ease-out;
  }

  .dock-panel.slid-out {
    visibility: hidden;
    pointer-events: none;
    transition:
      transform var(--slide) ease-in,
      visibility 0s linear var(--slide);
  }

  .dock-stage[data-edge='bottom'] .dock-panel.slid-out {
    transform: translateY(calc(100% + var(--edge-margin) + var(--shadow-room)));
  }

  .dock-stage[data-edge='top'] .dock-panel.slid-out {
    transform: translateY(calc(-100% - var(--edge-margin) - var(--shadow-room)));
  }

  .dock-stage[data-edge='left'] .dock-panel.slid-out {
    transform: translateX(calc(-100% - var(--edge-margin) - var(--shadow-room)));
  }

  .dock-stage[data-edge='right'] .dock-panel.slid-out {
    transform: translateX(calc(100% + var(--edge-margin) + var(--shadow-room)));
  }

  @media (prefers-reduced-motion: reduce) {
    .dock-panel {
      transition: opacity var(--slide) ease-out;
    }

    .dock-stage[data-edge] .dock-panel.slid-out {
      transform: none;
      opacity: 0;
      transition:
        opacity var(--slide) ease-in,
        visibility 0s linear var(--slide);
    }
  }

  /* Its start, length and thickness are set inline (`revealStripThickness`). */
  .reveal-strip {
    position: absolute;
  }

  .dock-stage[data-edge='bottom'] .reveal-strip {
    bottom: 0;
  }

  .dock-stage[data-edge='top'] .reveal-strip {
    top: 0;
  }

  .dock-stage[data-edge='left'] .reveal-strip {
    left: 0;
  }

  .dock-stage[data-edge='right'] .reveal-strip {
    right: 0;
  }

  /* A contrast theme: no blur and a 2 px line in the text colour
     (`--panel-border` is `--text` there, app.css) for every preset. Both
     are the preset's values, under the user's `--stem-panel-blur` and
     `--stem-panel-hairline` like everywhere else (docs/THEMING.md). */
  .dock-panel.high-contrast {
    --preset-panel-blur: none;
  }

  .dock-panel.high-contrast::before {
    border-width: 2px;
  }

  /* The drop gap: one slot long at rest, the icon box against its
     edge side like a slot's (its bottom on `bottom`), never a pointer
     target. */
  .drop-gap {
    position: relative;
    flex: none;
    display: flex;
    align-items: flex-end;
    justify-content: center;
    box-sizing: border-box;
    width: calc(var(--icon) + var(--slot-gap));
    height: var(--icon);
    transform-origin: 50% 100%;
    pointer-events: none;
  }

  .dock-stage[data-edge='top'] .drop-gap {
    align-items: flex-start;
    transform-origin: 50% 0;
  }

  .dock-stage[data-edge='left'] .drop-gap,
  .dock-stage[data-edge='right'] .drop-gap {
    align-items: center;
    width: var(--icon);
    height: calc(var(--icon) + var(--slot-gap));
  }

  .dock-stage[data-edge='left'] .drop-gap {
    transform-origin: 0 50%;
  }

  .dock-stage[data-edge='right'] .drop-gap {
    transform-origin: 100% 50%;
  }

  .drop-indicator {
    position: relative;
    flex: none;
    box-sizing: border-box;
    width: var(--icon);
    height: var(--icon);
    border: 2px dashed var(--accent);
    border-radius: 22%;
  }

  .drop-indicator::before {
    content: '';
    position: absolute;
    inset: 0;
    border-radius: inherit;
    background: var(--accent);
    opacity: 0.18;
  }

  .dock-panel.high-contrast .drop-indicator {
    border-color: var(--text);
  }

  /* Centred on the icons: 80 % of their height, 10 % off the bottom (off
     the edge side: a horizontal line on `left` and `right`). */
  .separator {
    flex: none;
    width: 1px;
    height: calc(var(--icon) * 0.8);
    margin: 0 var(--separator-margin) calc(var(--icon) * 0.1);
    background: var(--stem-separator, var(--preset-separator));
  }

  .dock-stage[data-edge='top'] .separator {
    margin: calc(var(--icon) * 0.1) var(--separator-margin) 0;
  }

  .dock-stage[data-edge='left'] .separator,
  .dock-stage[data-edge='right'] .separator {
    width: calc(var(--icon) * 0.8);
    height: 1px;
  }

  .dock-stage[data-edge='left'] .separator {
    margin: var(--separator-margin) 0 var(--separator-margin) calc(var(--icon) * 0.1);
  }

  .dock-stage[data-edge='right'] .separator {
    margin: var(--separator-margin) calc(var(--icon) * 0.1) var(--separator-margin) 0;
  }

  /* Playing its leave animation (item-transition.ts): still painted, no longer a target. */
  .dock-panel :global(.slot[data-leaving]) {
    pointer-events: none;
  }

  /* Ease back to rest after the pointer left; instant while tracking it. */
  .dock-panel.settling :global(.slot) {
    transition: width 120ms ease;
  }

  .dock-stage[data-edge='left'] .dock-panel.settling :global(.slot),
  .dock-stage[data-edge='right'] .dock-panel.settling :global(.slot) {
    transition: height 120ms ease;
  }

  .dock-panel.settling :global(.bouncer) {
    transition:
      width 120ms ease,
      height 120ms ease,
      transform 120ms ease,
      filter 120ms ease;
  }

  @media (prefers-reduced-motion: reduce) {
    .dock-panel.settling :global(.slot),
    .dock-panel.settling :global(.bouncer) {
      transition: none;
    }
  }

  /* The items on `top`, `left` and `right`. DockItem lays an item
     out for a dock on `bottom`: its slot as wide as the icon grows, the icon
     on the slot's bottom edge growing up, the dots below it, the title pill
     above it. Here the dock lays its items out for its own edge instead: on
     a vertical dock the slot is as wide as the icon and grows in height
     with `--scale`, the icon sits against the edge side and grows away from
     it, the dots sit between the icon and the edge and the pill on the side
     away from it; a file drop target grows away from the edge too. */
  .dock-stage[data-edge='top'] .dock-panel :global(.slot:not([data-kind='separator'])) {
    justify-content: flex-start;
    transform-origin: 50% 0;
  }

  .dock-stage[data-edge='left'] .dock-panel :global(.slot:not([data-kind='separator'])),
  .dock-stage[data-edge='right'] .dock-panel :global(.slot:not([data-kind='separator'])) {
    width: var(--icon);
    height: calc(var(--icon) * var(--scale, 1) + var(--slot-gap));
    justify-content: center;
  }

  .dock-stage[data-edge='left'] .dock-panel :global(.slot:not([data-kind='separator'])) {
    align-items: flex-start;
    transform-origin: 0 50%;
  }

  .dock-stage[data-edge='right'] .dock-panel :global(.slot:not([data-kind='separator'])) {
    align-items: flex-end;
    transform-origin: 100% 50%;
  }

  .dock-stage[data-edge='top'] .dock-panel :global(.slot .dots) {
    top: auto;
    bottom: calc(100% + var(--dots-gap));
  }

  .dock-stage[data-edge='left'] .dock-panel :global(.slot .dots),
  .dock-stage[data-edge='right'] .dock-panel :global(.slot .dots) {
    top: 0;
    bottom: 0;
    flex-direction: column;
    justify-content: center;
    width: var(--dot-size);
    height: auto;
  }

  .dock-stage[data-edge='left'] .dock-panel :global(.slot .dots) {
    left: auto;
    right: calc(100% + var(--dots-gap));
  }

  .dock-stage[data-edge='right'] .dock-panel :global(.slot .dots) {
    left: calc(100% + var(--dots-gap));
    right: auto;
  }

  .dock-stage[data-edge='left'] .dock-panel :global(.slot .dots.attention i),
  .dock-stage[data-edge='right'] .dock-panel :global(.slot .dots.attention i) {
    width: var(--dot-size);
    height: calc(var(--dot-size) * 3);
  }

  .dock-stage[data-edge='top'] .dock-panel :global(.slot .tooltip) {
    bottom: auto;
    top: calc(100% + 10px + var(--icon) * (var(--scale, 1) - 1 + var(--drop-lift, 0)));
  }

  .dock-stage[data-edge='top'] .dock-panel :global(.slot .tooltip::after) {
    top: auto;
    bottom: 100%;
    border-top-color: transparent;
    border-bottom-color: var(--tooltip-bg);
  }

  /* Beside the icon there is no window length to spare: a long title
     wraps at `--pill-max`, inside the room the window keeps for it
     (`SIDE_HEADROOM`). */
  .dock-stage[data-edge='left'] .dock-panel :global(.slot .tooltip),
  .dock-stage[data-edge='right'] .dock-panel :global(.slot .tooltip) {
    bottom: auto;
    top: 50%;
    transform: translateY(-50%);
    width: max-content;
    max-width: var(--pill-max);
    white-space: normal;
  }

  .dock-stage[data-edge='left'] .dock-panel :global(.slot .tooltip) {
    left: calc(100% + 10px + var(--icon) * (var(--scale, 1) - 1 + var(--drop-lift, 0)));
  }

  .dock-stage[data-edge='right'] .dock-panel :global(.slot .tooltip) {
    left: auto;
    right: calc(100% + 10px + var(--icon) * (var(--scale, 1) - 1 + var(--drop-lift, 0)));
  }

  .dock-stage[data-edge='left'] .dock-panel :global(.slot .tooltip::after),
  .dock-stage[data-edge='right'] .dock-panel :global(.slot .tooltip::after) {
    top: 50%;
    transform: translateY(-50%);
    border-top-color: transparent;
  }

  .dock-stage[data-edge='left'] .dock-panel :global(.slot .tooltip::after) {
    left: auto;
    right: 100%;
    border-right-color: var(--tooltip-bg);
  }

  .dock-stage[data-edge='right'] .dock-panel :global(.slot .tooltip::after) {
    left: 100%;
    border-left-color: var(--tooltip-bg);
  }

  .dock-stage[data-edge='top'] .dock-panel :global(.slot[data-drop-target] .bouncer) {
    transform: translateY(calc(50% * var(--drop-lift))) scale(calc(1 + var(--drop-lift)));
  }

  .dock-stage[data-edge='left'] .dock-panel :global(.slot[data-drop-target] .bouncer) {
    transform: translateX(calc(50% * var(--drop-lift))) scale(calc(1 + var(--drop-lift)));
  }

  .dock-stage[data-edge='right'] .dock-panel :global(.slot[data-drop-target] .bouncer) {
    transform: translateX(calc(-50% * var(--drop-lift))) scale(calc(1 + var(--drop-lift)));
  }

  /* Forced colours (WebView2 under a Windows contrast theme): the
     browser repaints backgrounds in the theme's Canvas colour, so the
     separators (1 px backgrounds) and the toast's dot would vanish. They take
     the theme's text colour instead. The panel needs nothing: its border and
     text are forced to the theme's colours, its background is opaque in high
     contrast (`effectiveOpacity`). The title pill's arrow on another edge
     takes the text colour like DockItem's own does on `bottom`. */
  @media (forced-colors: active) {
    .separator,
    .toast::before {
      forced-color-adjust: none;
      background: CanvasText;
    }

    .dock-stage[data-edge='top'] .dock-panel :global(.slot .tooltip::after) {
      border-bottom-color: CanvasText;
    }

    .dock-stage[data-edge='left'] .dock-panel :global(.slot .tooltip::after) {
      border-right-color: CanvasText;
    }

    .dock-stage[data-edge='right'] .dock-panel :global(.slot .tooltip::after) {
      border-left-color: CanvasText;
    }
  }
</style>
