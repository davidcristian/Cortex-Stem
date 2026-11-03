<script lang="ts">
  import { onMount } from 'svelte';

  import DockItem from '$lib/components/DockItem.svelte';
  import { hitRegionFor, magnifiedRise, sameHitRegion, type HitRegion } from '$lib/dock/geometry';
  import { iconRequestSize } from '$lib/dock/icon-size';
  import { itemTransition } from '$lib/dock/item-transition';
  import { computeScales, maxGrowth, type MagnifyParams } from '$lib/dock/magnify';
  import {
    DRAG_START,
    exceedsMoveThreshold,
    gapShift,
    insertionIndex,
    isInsideWithSlack,
    moveItem,
    type DropBounds,
  } from '$lib/dock/reorder';
  import { accentColor, effectiveOpacity, resolveTheme } from '$lib/dock/theme';
  import type { Backend } from '$lib/ipc/backend';
  import type { DockItem as DockItemModel } from '$lib/ipc/generated/DockItem';
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
  // the icon's height; the running dots hang below it inside the panel's
  // bottom padding, which is why the padding is the same all round.
  const SLOT_GAP = 8;
  const PANEL_PADDING = 8;
  const DOTS_GAP = 2;
  const DOT_SIZE = 4;
  const SEPARATOR_MARGIN = 6;
  const BOTTOM_MARGIN = 8;
  /** Room for the tooltip and the bounce above a magnified icon. */
  const HEADROOM = 48;
  /** Room on each side for the drop shadow. */
  const SIDE_SLACK = 24;
  /**
   * How far above a fully magnified icon's top edge the hit envelope still
   * counts as "inside" (CSS px). Small on purpose: anything higher is a band
   * where nothing is drawn but clicks would still be swallowed.
   */
  const HIT_SLACK = 2;
  /**
   * The window's height (CSS px; Rust clamps it to the work area) while an
   * icon is being dragged, so the lifted icon can go anywhere on screen.
   */
  const DRAG_WINDOW_HEIGHT = 4096;
  /** A release within this many icon sizes above or below the pinned row still drops; farther cancels. */
  const DROP_SLACK_ICONS = 3;
  const TOOLTIP_DELAY_MS = 300;

  // --- Environment facts ----------------------------------------------------
  let prefersDark = $state(false);
  let reducedMotion = $state(false);
  let dpr = $state(1);

  // --- Settings-derived values -----------------------------------------------
  const settings = $derived(store.settings);
  const items = $derived(store.state?.items ?? []);
  const theme = $derived(resolveTheme(settings?.theme ?? 'system', store.theme, prefersDark));
  const accent = $derived(accentColor(store.theme));
  const highContrast = $derived(store.theme?.highContrast ?? false);
  const opacity = $derived(effectiveOpacity(settings?.appearance.opacity ?? 0.72, store.theme));
  const cornerRadius = $derived(settings?.appearance.cornerRadius ?? 22);
  const iconSize = $derived(settings?.iconSize ?? 48);
  const slotPx = $derived(iconSize + SLOT_GAP);
  const magnify = $derived<MagnifyParams>({
    maxScale:
      settings?.magnification.enabled && !reducedMotion ? settings.magnification.maxScale : 1,
    rangeIcons: settings?.magnification.rangeIcons ?? 3,
  });
  // Icons render at their true magnified size, so the source must cover it.
  const requestSize = $derived(iconRequestSize(iconSize, magnify.maxScale, dpr));
  const tooltipsEnabled = $derived(settings?.appearance.tooltips ?? true);
  const bounceEnabled = $derived((settings?.appearance.bounceOnLaunch ?? true) && !reducedMotion);
  const itemMotion = $derived(!reducedMotion);
  const showDots = $derived(settings?.behavior.runningIndicators ?? true);

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

  // --- Icons ------------------------------------------------------------------
  $effect(() => {
    for (const item of items) {
      if (item.iconKey !== null) {
        store.ensureIcon(item.iconKey, requestSize);
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

  // --- Magnification controller (imperative, width only) ----------------------
  let panelEl = $state<HTMLDivElement>();
  let settling = $state(false);
  /** The slots that take part in layout: leaving ones are excluded. */
  let slots: HTMLElement[] = [];
  let scalable: boolean[] = [];
  let centres = new Float64Array(0);
  let scales = new Float64Array(0);
  let applied = new Float64Array(0);
  let pointerX = 0;
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
      const rect = slots[i]!.getBoundingClientRect();
      centres[i] = rect.left + rect.width / 2;
    }
    computeScales(pointerX, centres, slotPx, magnify, scales);
    for (let i = 0; i < n; i += 1) {
      if (!scalable[i]) {
        continue;
      }
      const s = Math.round(scales[i]! * 100) / 100;
      if (s !== applied[i]) {
        applied[i] = s;
        slots[i]!.style.setProperty('--scale', String(s));
      }
    }
  }

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
  }

  function onpanelpointermove(e: PointerEvent): void {
    if (magnify.maxScale <= 1 || drag?.started) {
      return;
    }
    pointerX = e.clientX;
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
    void backend.setDockHover?.(true).catch(report('set_dock_hover', false));
  }

  function onpanelpointerleave(): void {
    hovering = false;
    clearTooltip();
    // Ease back over 120 ms, then go idle (no timers, no transitions).
    settling = !reducedMotion && magnify.maxScale > 1;
    resetMagnification();
    if (settling) {
      settleTimer = setTimeout(() => {
        settleTimer = null;
        settling = false;
        measurePanel();
      }, 160);
    }
    void backend.setDockHover?.(false).catch(report('set_dock_hover', false));
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
    /** Pinned app slots in dock order, with their keys and rest centres, measured at start. */
    pinned: HTMLElement[];
    keys: string[];
    centres: Float64Array;
    bounds: DropBounds;
    from: number;
    to: number;
    /** `window.innerHeight` the drag's y coordinates refer to (the window grows during a drag). */
    viewportHeight: number;
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
      viewportHeight: window.innerHeight,
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
    // No magnification, tooltip or settle transition while dragging; the rest
    // positions measured below must be the unmagnified ones.
    clearTooltip();
    hovering = false;
    settling = false;
    if (settleTimer !== null) {
      clearTimeout(settleTimer);
      settleTimer = null;
    }
    resetMagnification();
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
    d.pinned.forEach((el, i) => {
      const rect = el.getBoundingClientRect();
      d.centres[i] = rect.left + rect.width / 2;
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
    // The lifted icon may go anywhere on screen: the window grows to the
    // work area's height for the duration of the drag (the panel stays at
    // the bottom; `onwindowresize` shifts the drag's y origin by the growth)
    // and all of it accepts clicks.
    d.viewportHeight = window.innerHeight;
    growWindowForDrag();
    dragHitReported = true;
    reportHitRegion();
    applyDrag();
  }

  function growWindowForDrag(): void {
    if (reported.width === 0) {
      return;
    }
    reported = { width: reported.width, height: DRAG_WINDOW_HEIGHT };
    void backend
      .setDockContentSize(reported.width, DRAG_WINDOW_HEIGHT)
      .catch(report('set_dock_content_size', false));
  }

  /**
   * The window grew (or shrank back) during a drag. The viewport's origin
   * moved, so every stored y coordinate shifts by the change to keep
   * pointing at the same screen spot, and the whole-window envelope is
   * reported at its new size.
   */
  function onwindowresize(): void {
    const d = drag;
    if (d === null || !d.started) {
      return;
    }
    const delta = window.innerHeight - d.viewportHeight;
    if (delta === 0) {
      return;
    }
    d.viewportHeight = window.innerHeight;
    d.startY += delta;
    d.lastY += delta;
    d.bounds = { ...d.bounds, top: d.bounds.top + delta, bottom: d.bounds.bottom + delta };
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
    const to = insertionIndex((d.centres[d.from] ?? 0) + dx, d.centres, d.from);
    if (to === d.to) {
      return;
    }
    d.to = to;
    d.pinned.forEach((el, i) => {
      if (el === d.el) {
        return;
      }
      const shift = gapShift(i, d.from, to, slotPx);
      el.style.transform = shift === 0 ? '' : `translateX(${shift}px)`;
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
    if (
      d.to !== d.from &&
      isInsideWithSlack(d.lastX, d.lastY, d.bounds, slotPx, DROP_SLACK_ICONS * iconSize)
    ) {
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
    d.el.style.transform = `translateX(${(d.centres[d.to] ?? 0) - (d.centres[d.from] ?? 0)}px)`;
    liftedKey = null;
    drag = null;
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
    void backend.showItemMenu(item.key, x, y).catch(report('Menu'));
  }

  function onpanelcontextmenu(e: MouseEvent): void {
    e.preventDefault();
    if (e.target === panelEl && !drag?.started) {
      void backend.showDockMenu(e.clientX, e.clientY).catch(report('Menu'));
    }
  }

  function openSettings(): void {
    void backend.openSettings().catch(report('Opening Settings'));
  }

  /** Keyboard shortcuts while the dock window has focus. */
  function onwindowkeydown(e: KeyboardEvent): void {
    if (e.key === 'Escape' && drag !== null) {
      e.preventDefault();
      cancelDrag(true);
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
  /** The panel's bounding box at rest, viewport-relative CSS px (fractional). */
  let measured: Rect | null = null;
  let reported = { width: 0, height: 0 };
  let reportedHit: HitRegion | null = null;
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
    if (!panel || hovering || settling || drag?.started || itemsInFlight(panel)) {
      return;
    }
    const rect = panel.getBoundingClientRect();
    measured = { x: rect.x, y: rect.y, width: rect.width, height: rect.height };
    reportSize();
  }

  function reportSize(): void {
    if (measured === null || measured.width === 0) {
      return;
    }
    const width = Math.ceil(measured.width + 2 * SIDE_SLACK + maxGrowth(slotPx, magnify));
    const height = Math.ceil(
      measured.height + BOTTOM_MARGIN + iconSize * (magnify.maxScale - 1) + HEADROOM,
    );
    if (width !== reported.width || height !== reported.height) {
      reported = { width, height };
      void backend.setDockContentSize(width, height).catch(report('set_dock_content_size', false));
    }
    reportHitRegion();
  }

  /**
   * The panel rounded outwards, and the envelope its magnified icons can
   * reach. A drag in progress makes the whole window clickable instead: the
   * lifted icon can be anywhere in it.
   */
  function currentHitRegion(): HitRegion | null {
    if (measured === null || measured.width === 0) {
      return null;
    }
    const region = hitRegionFor(
      measured,
      maxGrowth(slotPx, magnify),
      // A magnified icon's top is iconSize × (maxScale − 1) above its slot's
      // top edge, and the slot sits PANEL_PADDING below the panel's top edge.
      magnifiedRise(iconSize, magnify.maxScale, HIT_SLACK - PANEL_PADDING),
    );
    if (drag?.started) {
      return {
        panel: region.panel,
        envelope: { x: 0, y: 0, width: window.innerWidth, height: window.innerHeight },
      };
    }
    return region;
  }

  function reportHitRegion(): void {
    const region = currentHitRegion();
    if (region === null || (reportedHit !== null && sameHitRegion(reportedHit, region))) {
      return;
    }
    reportedHit = region;
    void backend
      .setDockHitRegion(region.panel, region.envelope)
      .catch(report('set_dock_hit_region', false));
  }

  // Settings can change the headroom and the envelope without changing the panel box.
  $effect(() => {
    void magnify;
    void iconSize;
    reportSize();
  });

  $effect(() => {
    const panel = panelEl;
    if (!panel) {
      return;
    }
    const observer = new ResizeObserver(scheduleMeasure);
    observer.observe(panel);
    return () => {
      observer.disconnect();
      if (measureFrame !== 0) {
        cancelAnimationFrame(measureFrame);
        measureFrame = 0;
      }
    };
  });

  // --- Lifecycle ------------------------------------------------------------------
  onMount(() => {
    const dark = window.matchMedia('(prefers-color-scheme: dark)');
    const motion = window.matchMedia('(prefers-reduced-motion: reduce)');
    prefersDark = dark.matches;
    reducedMotion = motion.matches;
    dpr = window.devicePixelRatio || 1;
    const onDark = (): void => {
      prefersDark = dark.matches;
    };
    const onMotion = (): void => {
      reducedMotion = motion.matches;
    };
    dark.addEventListener('change', onDark);
    motion.addEventListener('change', onMotion);
    void store.start();
    return () => {
      dark.removeEventListener('change', onDark);
      motion.removeEventListener('change', onMotion);
      clearTooltip();
      resetMagnification();
      if (settleTimer !== null) {
        clearTimeout(settleTimer);
      }
      cancelDrag(false);
      clearDragStyles();
      toasts.clear();
      store.stop();
    };
  });
</script>

<svelte:window
  onkeydown={onwindowkeydown}
  onpointermove={onwindowpointermove}
  onpointerup={onwindowpointerup}
  onpointercancel={onwindowpointercancel}
  onresize={onwindowresize}
/>

<div
  class="dock-stage"
  style:--accent={accent}
  style:--panel-opacity={opacity}
  style:--radius="{cornerRadius}px"
  style:--icon="{iconSize}px"
  style:--slot-gap="{SLOT_GAP}px"
  style:--panel-padding="{PANEL_PADDING}px"
  style:--dots-gap="{DOTS_GAP}px"
  style:--dot-size="{DOT_SIZE}px"
  style:--separator-margin="{SEPARATOR_MARGIN}px"
  style:--bottom-margin="{BOTTOM_MARGIN}px"
>
  {#if store.state !== null && settings !== null}
    <div
      class="dock-panel"
      class:settling
      class:high-contrast={highContrast}
      bind:this={panelEl}
      role="toolbar"
      aria-label="Stem"
      aria-orientation="horizontal"
      tabindex="-1"
      data-ready="true"
      onpointermove={onpanelpointermove}
      onpointerenter={onpanelpointerenter}
      onpointerleave={onpanelpointerleave}
      oncontextmenu={onpanelcontextmenu}
    >
      {#each items as item (item.key)}
        {#if item.kind === 'separator'}
          <div
            class="slot separator"
            role="separator"
            aria-orientation="vertical"
            data-key={item.key}
            data-kind="separator"
            in:itemTransition|global={{ enabled: itemMotion && introsEnabled }}
            out:itemTransition|global={{ enabled: itemMotion }}
            onintroend={onItemSettled}
            onoutroend={onItemSettled}
          ></div>
        {:else}
          <DockItem
            {item}
            iconUrl={item.iconKey !== null ? store.icon(item.iconKey, requestSize) : null}
            {showDots}
            tooltip={tooltipKey === item.key}
            bounce={bounceEnabled && store.bounceKey === item.key ? store.bounceSerial : 0}
            animate={itemMotion}
            enter={introsEnabled}
            lifted={liftedKey === item.key}
            {onprimary}
            {onmiddle}
            {oncontext}
            {onhover}
            {onpress}
            onsettled={onItemSettled}
          />
        {/if}
      {/each}
    </div>
  {/if}

  <!-- Failure notices: right-aligned just above the panel, never in the way. -->
  <div class="toasts" aria-live="polite">
    {#each toasts.items as toast (toast.id)}
      <div class="toast" role="status">{toast.message}</div>
    {/each}
  </div>
</div>

<style>
  .dock-stage {
    position: relative;
    display: flex;
    align-items: flex-end;
    justify-content: center;
    width: 100%;
    height: 100%;
    color: var(--text);
  }

  .toasts {
    position: absolute;
    left: 8px;
    right: 8px;
    /* The panel's top edge plus a small gap; the panel box is icon + padding. */
    bottom: calc(var(--bottom-margin) + var(--icon) + 2 * var(--panel-padding) + 6px);
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 4px;
    pointer-events: none;
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
  }

  /* Content-sized and centred by the stage: items animating in and out
     widen and narrow it without the window ever resizing (docs/ipc/sizing-protocol.md). */
  .dock-panel {
    position: relative;
    display: flex;
    align-items: flex-end;
    flex: none;
    box-sizing: border-box;
    padding: var(--panel-padding);
    margin-bottom: var(--bottom-margin);
    border-radius: var(--radius);
    background: rgb(var(--panel-tint) / var(--panel-opacity));
    -webkit-backdrop-filter: blur(24px) saturate(1.4);
    backdrop-filter: blur(24px) saturate(1.4);
    box-shadow: var(--panel-shadow);
  }

  /* Hairline border drawn inside the box so it never affects layout. */
  .dock-panel::before {
    content: '';
    position: absolute;
    inset: 0;
    border-radius: inherit;
    border: 1px solid var(--panel-border);
    pointer-events: none;
  }

  .dock-panel.high-contrast {
    -webkit-backdrop-filter: none;
    backdrop-filter: none;
  }

  .dock-panel.high-contrast::before {
    border: 2px solid var(--text);
  }

  /* Centred on the icons: 80 % of their height, 10 % off the bottom. */
  .separator {
    flex: none;
    width: 1px;
    height: calc(var(--icon) * 0.8);
    margin: 0 var(--separator-margin) calc(var(--icon) * 0.1);
    background: var(--separator);
  }

  /* Playing its leave animation (item-transition.ts): still painted, no longer a target. */
  .dock-panel :global(.slot[data-leaving]) {
    pointer-events: none;
  }

  /* Ease back to rest after the pointer left; instant while tracking it. */
  .dock-panel.settling :global(.slot) {
    transition: width 120ms ease;
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
</style>
