<script lang="ts">
  import { onMount } from 'svelte';

  import DockItem from '$lib/components/DockItem.svelte';
  import { computeScales, maxGrowth, type MagnifyParams } from '$lib/dock/magnify';
  import { accentColor, effectiveOpacity, resolveTheme } from '$lib/dock/theme';
  import type { Backend } from '$lib/ipc/backend';
  import type { DockItem as DockItemModel } from '$lib/ipc/generated/DockItem';
  import type { DockStore } from '$lib/stores/dock.svelte';

  interface Props {
    /** Created once by the page entry; it owns the backend subscription. */
    store: DockStore;
    /** Browser demo (mock backend): paint a wallpaper behind the dock. */
    demo?: boolean;
  }

  let { store, demo = false }: Props = $props();
  const backend: Backend = $derived(store.backend);

  // Layout constants (CSS px). The slot gap and dots area are also CSS
  // variables so the stylesheet and the sizing maths cannot drift apart.
  const SLOT_GAP = 8;
  const DOTS_AREA = 7;
  const PANEL_PADDING = 6;
  const BOTTOM_MARGIN = 8;
  /** Room for the tooltip and the bounce above a magnified icon. */
  const HEADROOM = 48;
  /** Room on each side for the drop shadow. */
  const SIDE_SLACK = 24;
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
  const requestSize = $derived(Math.max(16, Math.round(iconSize * dpr)));
  const slotPx = $derived(iconSize + SLOT_GAP);
  const magnify = $derived<MagnifyParams>({
    maxScale:
      settings?.magnification.enabled && !reducedMotion ? settings.magnification.maxScale : 1,
    rangeIcons: settings?.magnification.rangeIcons ?? 3,
  });
  const tooltipsEnabled = $derived(settings?.appearance.tooltips ?? true);
  const bounceEnabled = $derived((settings?.appearance.bounceOnLaunch ?? true) && !reducedMotion);
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

  // --- Magnification controller (imperative, transform/width only) ----------
  let panelEl = $state<HTMLDivElement>();
  let settling = $state(false);
  let slots: HTMLElement[] = [];
  let scalable: boolean[] = [];
  let centres = new Float64Array(0);
  let scales = new Float64Array(0);
  let applied = new Float64Array(0);
  let pointerX = 0;
  let hovering = false;
  let frame = 0;
  let settleTimer: ReturnType<typeof setTimeout> | null = null;

  // Re-collect the slot elements whenever the item list changed (not per frame).
  $effect(() => {
    void items;
    const panel = panelEl;
    if (!panel) {
      return;
    }
    slots = Array.from(panel.querySelectorAll<HTMLElement>('.slot'));
    scalable = slots.map((el) => el.dataset.kind !== 'separator');
    if (centres.length !== slots.length) {
      centres = new Float64Array(slots.length);
      scales = new Float64Array(slots.length);
      applied = new Float64Array(slots.length).fill(1);
    }
    if (hovering) {
      scheduleFrame();
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
    if (magnify.maxScale <= 1) {
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
    void backend.setDockHover?.(true).catch(report('set_dock_hover'));
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
    void backend.setDockHover?.(false).catch(report('set_dock_hover'));
  }

  // --- Tooltip ----------------------------------------------------------------
  let tooltipKey = $state<string | null>(null);
  let tooltipTimer: ReturnType<typeof setTimeout> | null = null;

  function onhover(item: DockItemModel | null): void {
    if (tooltipTimer !== null) {
      clearTimeout(tooltipTimer);
      tooltipTimer = null;
    }
    if (item === null || !tooltipsEnabled) {
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

  // --- Intents ------------------------------------------------------------------
  const report =
    (what: string) =>
    (err: unknown): void => {
      console.warn(`${what} failed:`, err);
    };

  function onprimary(item: DockItemModel): void {
    void backend.primaryClick(item.key).catch(report(`primary_click(${item.key})`));
  }

  function onmiddle(item: DockItemModel): void {
    if (item.appId !== null) {
      void backend.launchApp(item.appId).catch(report(`launch_app(${item.appId})`));
    }
  }

  function oncontext(item: DockItemModel, x: number, y: number): void {
    void backend.showItemMenu?.(item.key, x, y).catch(report('show_item_menu'));
  }

  function onpanelcontextmenu(e: MouseEvent): void {
    e.preventDefault();
    if (e.target === panelEl) {
      void backend.showDockMenu?.(e.clientX, e.clientY).catch(report('show_dock_menu'));
    }
  }

  // --- Sizing protocol (docs/ipc/) --------------------------------------------
  let measured = { width: 0, height: 0 };
  let reported = { width: 0, height: 0 };
  let measureFrame = 0;

  function measurePanel(): void {
    const panel = panelEl;
    if (!panel || hovering || settling) {
      return;
    }
    const rect = panel.getBoundingClientRect();
    measured = { width: rect.width, height: rect.height };
    reportSize();
  }

  function reportSize(): void {
    if (measured.width === 0) {
      return;
    }
    const width = Math.ceil(measured.width + 2 * SIDE_SLACK + maxGrowth(slotPx, magnify));
    const height = Math.ceil(
      measured.height + BOTTOM_MARGIN + iconSize * (magnify.maxScale - 1) + HEADROOM,
    );
    if (width === reported.width && height === reported.height) {
      return;
    }
    reported = { width, height };
    void backend.setDockContentSize(width, height).catch(report('set_dock_content_size'));
  }

  // Settings can change the headroom without changing the panel box.
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
    const observer = new ResizeObserver(() => {
      // At most one report per animation frame.
      if (measureFrame === 0) {
        measureFrame = requestAnimationFrame(() => {
          measureFrame = 0;
          measurePanel();
        });
      }
    });
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
      store.stop();
    };
  });
</script>

<div
  class="dock-stage"
  style:--accent={accent}
  style:--panel-opacity={opacity}
  style:--radius="{cornerRadius}px"
  style:--icon="{iconSize}px"
  style:--slot-gap="{SLOT_GAP}px"
  style:--dots-area="{DOTS_AREA}px"
  style:--panel-padding="{PANEL_PADDING}px"
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
          ></div>
        {:else}
          <DockItem
            {item}
            iconUrl={item.iconKey !== null ? store.icon(item.iconKey, requestSize) : null}
            {showDots}
            tooltip={tooltipKey === item.key}
            bounce={bounceEnabled && store.bounceKey === item.key ? store.bounceSerial : 0}
            animate={!reducedMotion}
            {onprimary}
            {onmiddle}
            {oncontext}
            {onhover}
          />
        {/if}
      {/each}
    </div>
  {/if}
</div>

<style>
  .dock-stage {
    display: flex;
    align-items: flex-end;
    justify-content: center;
    width: 100%;
    height: 100%;
    color: var(--text);
  }

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

  .separator {
    flex: none;
    width: 1px;
    height: calc(var(--icon) * 0.8);
    margin: 0 6px calc(var(--dots-area) + var(--icon) * 0.1);
    background: var(--separator);
  }

  /* Ease back to rest after the pointer left; instant while tracking it. */
  .dock-panel.settling :global(.slot) {
    transition: width 120ms ease;
  }

  .dock-panel.settling :global(.icon) {
    transition: transform 120ms ease;
  }

  @media (prefers-reduced-motion: reduce) {
    .dock-panel.settling :global(.slot),
    .dock-panel.settling :global(.icon) {
      transition: none;
    }
  }
</style>
