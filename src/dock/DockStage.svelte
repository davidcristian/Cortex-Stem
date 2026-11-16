<script lang="ts">
  import type { Snippet } from 'svelte';

  import { AUTO_HIDE_SLIDE_MS } from '$lib/dock/autohide';
  import type { DockEdge } from '$lib/dock/edge';
  import type { ThemePreset } from '$lib/ipc/generated/ThemePreset';

  import {
    DOT_SIZE,
    DOTS_GAP,
    EDGE_MARGIN,
    PANEL_PADDING,
    PILL_MAX_WIDTH,
    SEPARATOR_MARGIN,
    SIDE_SLACK,
    SLOT_GAP,
  } from './layout';

  interface Props {
    edge: DockEdge;
    /** The theme preset (`appearance.theme`). */
    preset: ThemePreset;
    /** Drawn smaller than the icon size setting (`EdgeFit.fitted`). */
    fitted: boolean;
    accent: string;
    opacity: number;
    cornerRadius: number;
    /** The icon size the page draws at (`EdgeFit.iconSize`). */
    iconSize: number;
    children: Snippet;
  }

  let { edge, preset, fitted, accent, opacity, cornerRadius, iconSize, children }: Props = $props();
</script>

<div
  class="dock-stage"
  data-edge={edge}
  data-theme={preset}
  data-fitted={fitted ? '' : undefined}
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
  {@render children()}
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
     never moves an icon. Stem's own neutral tiles (Task View, Show
     Desktop, Quick Settings, Notification Center, the volume, the battery,
     the clock)
     are a light wash over the panel elsewhere; with no panel under them
     they take a nearly opaque tile in the panel's tint, so their text and
     lines read over any wallpaper (`--panel-tint` follows the colour
     mode). */
  :global(html:not([data-high-contrast])) .dock-stage[data-theme='minimal'] {
    --preset-panel-fill: transparent;
    --preset-panel-blur: none;
    --preset-panel-hairline: transparent;
    --preset-panel-shadow: none;
    --preset-separator: rgb(255 255 255 / 0.5);
    --tile-neutral: rgb(var(--panel-tint) / 0.85);
  }

  :global(html[data-theme='light']:not([data-high-contrast])) .dock-stage[data-theme='minimal'] {
    --preset-separator: rgb(0 0 0 / 0.4);
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

  .dock-stage[data-edge='top'] :global(.dock-panel) {
    align-items: flex-start;
    margin-bottom: 0;
    margin-top: var(--edge-margin);
  }

  .dock-stage[data-edge='left'] :global(.dock-panel),
  .dock-stage[data-edge='right'] :global(.dock-panel) {
    flex-direction: column;
    margin-bottom: 0;
  }

  .dock-stage[data-edge='left'] :global(.dock-panel) {
    align-items: flex-start;
    margin-left: var(--edge-margin);
  }

  .dock-stage[data-edge='right'] :global(.dock-panel) {
    align-items: flex-end;
    margin-right: var(--edge-margin);
  }

  /* The auto-hide slide on each edge (Dock.svelte has the slide itself). */
  .dock-stage[data-edge='bottom'] :global(.dock-panel.slid-out) {
    transform: translateY(calc(100% + var(--edge-margin) + var(--shadow-room)));
  }

  .dock-stage[data-edge='top'] :global(.dock-panel.slid-out) {
    transform: translateY(calc(-100% - var(--edge-margin) - var(--shadow-room)));
  }

  .dock-stage[data-edge='left'] :global(.dock-panel.slid-out) {
    transform: translateX(calc(-100% - var(--edge-margin) - var(--shadow-room)));
  }

  .dock-stage[data-edge='right'] :global(.dock-panel.slid-out) {
    transform: translateX(calc(100% + var(--edge-margin) + var(--shadow-room)));
  }

  @media (prefers-reduced-motion: reduce) {
    .dock-stage[data-edge] :global(.dock-panel.slid-out) {
      transform: none;
      opacity: 0;
      transition:
        opacity var(--slide) ease-in,
        visibility 0s linear var(--slide);
    }
  }

  .dock-stage[data-edge='top'] :global(.separator) {
    margin: calc(var(--icon) * 0.1) var(--separator-margin) 0;
  }

  .dock-stage[data-edge='left'] :global(.separator),
  .dock-stage[data-edge='right'] :global(.separator) {
    width: calc(var(--icon) * 0.8);
    height: 1px;
  }

  .dock-stage[data-edge='left'] :global(.separator) {
    margin: var(--separator-margin) 0 var(--separator-margin) calc(var(--icon) * 0.1);
  }

  .dock-stage[data-edge='right'] :global(.separator) {
    margin: var(--separator-margin) calc(var(--icon) * 0.1) var(--separator-margin) 0;
  }

  .dock-stage[data-edge='left'] :global(.dock-panel.settling .slot),
  .dock-stage[data-edge='right'] :global(.dock-panel.settling .slot) {
    transition: height 120ms ease;
  }

  /* The items on `top`, `left` and `right`. DockItem lays an item
     out for a dock on `bottom`: its slot as wide as the icon grows, the icon
     on the slot's bottom edge growing up, the dots below it, the title pill
     above it. Here the dock lays its items out for its own edge instead: on
     a vertical dock the slot is as wide as the icon and grows in height
     with `--scale`, the icon sits against the edge side and grows away from
     it, the dots sit between the icon and the edge and the pill on the side
     away from it; a file drop target grows away from the edge too. The
     rules for the dots, the pill and the drop target are in ItemDots.svelte,
     ItemTooltip.svelte and ItemBouncer.svelte. */
  .dock-stage[data-edge='top'] :global(.dock-panel .slot:not([data-kind='separator'])) {
    justify-content: flex-start;
    transform-origin: 50% 0;
  }

  .dock-stage[data-edge='left'] :global(.dock-panel .slot:not([data-kind='separator'])),
  .dock-stage[data-edge='right'] :global(.dock-panel .slot:not([data-kind='separator'])) {
    width: var(--icon);
    height: calc(var(--icon) * var(--scale, 1) + var(--slot-gap));
    justify-content: center;
  }

  .dock-stage[data-edge='left'] :global(.dock-panel .slot:not([data-kind='separator'])) {
    align-items: flex-start;
    transform-origin: 0 50%;
  }

  .dock-stage[data-edge='right'] :global(.dock-panel .slot:not([data-kind='separator'])) {
    align-items: flex-end;
    transform-origin: 100% 50%;
  }
</style>
