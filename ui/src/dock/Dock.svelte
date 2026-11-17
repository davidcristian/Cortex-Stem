<script lang="ts">
  import { onMount } from 'svelte';

  import { itemTransition } from '$lib/dock/item-transition';
  import type { DockStore } from '$lib/stores/dock.svelte';

  import { DockController } from './controller.svelte';
  import DockStage from './DockStage.svelte';
  import DockToasts from './DockToasts.svelte';
  import DropGap from './DropGap.svelte';
  import PanelItem from './PanelItem.svelte';
  import RevealStrip from './RevealStrip.svelte';

  interface Props {
    /** Created once by the page entry; it owns the backend subscription. */
    store: DockStore;
    /** Browser demo (mock backend): paint a wallpaper behind the dock. */
    demo?: boolean;
  }

  let { store, demo = false }: Props = $props();
  const dock = new DockController(
    () => store,
    () => demo,
  );
  const dropGap = $derived(dock.files.dropGap);

  // The panel's pointer handlers stay in this file: the idle checks
  // (e2e/idle.spec.ts) find the hover's frames and settle timer by this file's name.
  function onpanelpointermove(e: PointerEvent): void {
    if (!dock.autoHide.state.pointerInside) {
      // A move over the panel says where the pointer is even when no
      // `pointerenter` came (after a drag or a native menu).
      dock.autoHide.send({ type: 'pointer', inside: true });
    }
    // Where it is, kept even while nothing magnifies: magnification turned
    // on under a resting pointer starts from there.
    dock.hover.pointerX = e.clientX;
    dock.hover.pointerY = e.clientY;
    dock.hover.pointerOver = true;
    // No hover until the window is measured at rest after a drag: the
    // hover would hold off that measurement (`restoringAfterDrag`).
    if (
      dock.view.magnify.maxScale <= 1 ||
      dock.reorder.drag?.started ||
      dock.reorder.restoringAfterDrag
    ) {
      return;
    }
    dock.hover.startHover();
    dock.hover.scheduleFrame();
  }

  function onpanelpointerleave(): void {
    dock.hover.pointerOver = false;
    dock.hover.hovering = false;
    dock.intents.wheelNotches.reset();
    // The settle below measures; the next visit may hover again.
    dock.reorder.restoringAfterDrag = false;
    dock.tooltip.clearTooltip();
    // Ease back over 120 ms, then go idle (no timers, no transitions).
    dock.hover.settling = !dock.view.reducedMotion && dock.view.magnify.maxScale > 1;
    dock.hover.resetMagnification();
    if (dock.hover.settling) {
      dock.hover.settleTimer = setTimeout(() => {
        dock.hover.settleTimer = null;
        dock.hover.settling = false;
        dock.hover.showRestIcons();
        dock.measure.measurePanel();
      }, 160);
    }
    void dock.backend.setDockHover?.(false).catch(dock.intents.report('set_dock_hover', false));
    dock.autoHide.send({ type: 'pointer', inside: false });
  }

  onMount(() => dock.mount());
</script>

{#snippet gap()}
  <DropGap
    itemMotion={dock.view.itemMotion}
    axis={dock.view.axis}
    landedKey={dock.landing.landedKey}
    onoutroend={dock.landing.ongapoutroend}
  />
{/snippet}

<svelte:window
  onkeydown={dock.intents.onwindowkeydown}
  onpointermove={dock.reorderPointer.onwindowpointermove}
  onpointerup={dock.reorderPointer.onwindowpointerup}
  onpointercancel={dock.reorderPointer.onwindowpointercancel}
  onresize={dock.measure.onwindowresize}
  oncontextmenu={dock.intents.onwindowcontextmenu}
/>

<DockStage
  edge={dock.view.edge}
  preset={dock.view.preset}
  fitted={dock.fit.fitted}
  accent={dock.view.accent}
  opacity={dock.view.opacity}
  cornerRadius={dock.view.cornerRadius}
  iconSize={dock.fit.iconSize}
>
  {#if store.state !== null && dock.view.settings !== null}
    <div
      class="dock-panel"
      class:settling={dock.hover.settling}
      class:high-contrast={dock.view.highContrast}
      class:slid-out={dock.autoHide.autoHidden}
      bind:this={dock.panelEl}
      role="toolbar"
      aria-label="Stem"
      aria-orientation={dock.view.axis}
      tabindex="-1"
      data-ready="true"
      data-axis={dock.view.axis}
      data-autohide={dock.autoHide.autoHideOn ? dock.autoHide.autoHidePhase : undefined}
      onpointermove={onpanelpointermove}
      onpointerenter={dock.hover.onpanelpointerenter}
      onpointerleave={onpanelpointerleave}
      onwheel={dock.intents.onpanelwheel}
      ontransitionend={dock.autoHide.onpaneltransitionend}
      oncontextmenu={dock.intents.onpanelcontextmenu}
      onkeydown={dock.keys.onpanelkeydown}
      onfocusin={dock.keys.onpanelfocusin}
      onfocusout={dock.keys.onpanelfocusout}
    >
      {#each dock.view.items as item (item.key)}
        {#if dropGap !== null && dropGap.key === item.key && dropGap.side === 'before'}
          {@render gap()}
        {/if}
        {#if item.kind === 'separator'}
          <div
            class="slot separator"
            role="separator"
            aria-orientation={dock.view.vertical ? 'horizontal' : 'vertical'}
            data-key={item.key}
            data-kind="separator"
            in:itemTransition|global={{
              enabled: dock.view.itemMotion && dock.introsEnabled,
              axis: dock.view.axis,
            }}
            out:itemTransition|global={{ enabled: dock.view.itemMotion, axis: dock.view.axis }}
            onintroend={dock.onItemSettled}
            onoutroend={dock.onItemSettled}
          ></div>
        {:else}
          <PanelItem {item} state={store.state} {dock} />
        {/if}
        {#if dropGap !== null && dropGap.key === item.key && dropGap.side === 'after'}
          {@render gap()}
        {/if}
      {/each}
      {#if dropGap !== null && dropGap.key === null}
        {@render gap()}
      {/if}
    </div>
    {#if dock.autoHide.stripOn && dock.autoHide.stripBox !== null}
      <RevealStrip
        stripBox={dock.autoHide.stripBox}
        autoHidePhase={dock.autoHide.autoHidePhase}
        vertical={dock.view.vertical}
        onpointerenter={dock.autoHide.onstripenter}
        onpointerleave={dock.autoHide.onstripleave}
      />
    {/if}
  {/if}

  <DockToasts toasts={dock.intents.toasts.items} atEdge={dock.autoHide.autoHidden} />
</DockStage>

<style>
  /* The dots and the title pills are ItemDots.svelte's and
     ItemTooltip.svelte's: they read the accent and the tooltip tokens,
     which take the variables here, for their subtree only (the drop
     outline and the Start icon keep the accent). */
  .dock-panel :global(.dots) {
    --accent: var(--stem-dot, var(--preset-dot));
  }

  .dock-panel :global(.tooltip) {
    --tooltip-bg: var(--stem-pill-fill, var(--preset-pill-fill));
    --tooltip-border: var(--preset-pill-border);
    --text: var(--stem-pill-text, var(--preset-pill-text));
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

  @media (prefers-reduced-motion: reduce) {
    .dock-panel {
      transition: opacity var(--slide) ease-out;
    }
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

  /* Centred on the icons: 80 % of their height, 10 % off the bottom (off
     the edge side: a horizontal line on `left` and `right`). */
  .separator {
    flex: none;
    width: 1px;
    height: calc(var(--icon) * 0.8);
    margin: 0 var(--separator-margin) calc(var(--icon) * 0.1);
    background: var(--stem-separator, var(--preset-separator));
  }

  /* Playing its leave animation (item-transition.ts): still painted, no longer a target. */
  .dock-panel :global(.slot[data-leaving]) {
    pointer-events: none;
  }

  /* Ease back to rest after the pointer left; instant while tracking it. */
  .dock-panel.settling :global(.slot) {
    transition: width 120ms ease;
  }

  @media (prefers-reduced-motion: reduce) {
    .dock-panel.settling :global(.slot) {
      transition: none;
    }
  }

  /* Forced colours (WebView2 under a Windows contrast theme): the
     browser repaints backgrounds in the theme's Canvas colour, so the
     separators (1 px backgrounds) would vanish. They take the theme's text
     colour instead. The panel needs nothing: its border and text are forced
     to the theme's colours, its background is opaque in high contrast
     (`effectiveOpacity`). */
  @media (forced-colors: active) {
    .separator {
      forced-color-adjust: none;
      background: CanvasText;
    }
  }
</style>
