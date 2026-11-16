<script lang="ts">
  interface Props {
    /** How many to draw: up to three dots, or one pill while the app needs attention. */
    count: number;
    /** The app is in front. */
    active: boolean;
    /** Every window of the app is minimised. */
    dim: boolean;
    /** The app needs attention: one pill in the attention colour. */
    attention: boolean;
  }

  let { count, active, dim, attention }: Props = $props();
</script>

<div class="dots" class:active class:dim class:attention aria-hidden="true">
  {#each { length: count } as _, i (i)}
    <i></i>
  {/each}
</div>

<style>
  :global(.slot[data-drag='lifted']) .dots {
    opacity: 0;
  }

  /* Running indicators live in the panel's bottom padding, under the icon,
     so the slot itself is exactly the icon's height and the panel's padding
     reads the same above and below. */
  .dots {
    position: absolute;
    left: 0;
    right: 0;
    top: calc(100% + var(--dots-gap));
    display: flex;
    justify-content: center;
    gap: 3px;
    height: var(--dot-size);
    pointer-events: none;
  }

  .dots i {
    display: block;
    width: var(--dot-size);
    height: var(--dot-size);
    border-radius: 50%;
    background: var(--accent);
    opacity: 0.72;
  }

  .dots.dim i {
    opacity: 0.35;
  }

  .dots.active i {
    opacity: 1;
    box-shadow: 0 0 4px var(--accent);
  }

  /* Needs attention: a still cue next to the wiggle, the only one
     under reduced motion: one pill, three dots wide, in the attention colour.
     Wins over `dim`: minimised apps flash too. */
  .dots.attention i {
    width: calc(var(--dot-size) * 3);
    border-radius: calc(var(--dot-size) / 2);
    background: var(--attention);
    opacity: 1;
  }

  /* On `top`, `left` and `right` the dots sit between the icon and the edge
     (DockStage.svelte says how the dock lays its items out there). */
  :global(.dock-stage[data-edge='top'] .dock-panel .slot) .dots {
    top: auto;
    bottom: calc(100% + var(--dots-gap));
  }

  :global(.dock-stage[data-edge='left'] .dock-panel .slot) .dots,
  :global(.dock-stage[data-edge='right'] .dock-panel .slot) .dots {
    top: 0;
    bottom: 0;
    flex-direction: column;
    justify-content: center;
    width: var(--dot-size);
    height: auto;
  }

  :global(.dock-stage[data-edge='left'] .dock-panel .slot) .dots {
    left: auto;
    right: calc(100% + var(--dots-gap));
  }

  :global(.dock-stage[data-edge='right'] .dock-panel .slot) .dots {
    left: calc(100% + var(--dots-gap));
    right: auto;
  }

  :global(.dock-stage[data-edge='left'] .dock-panel .slot) .dots.attention i,
  :global(.dock-stage[data-edge='right'] .dock-panel .slot) .dots.attention i {
    width: var(--dot-size);
    height: calc(var(--dot-size) * 3);
  }

  /* Forced colours (WebView2 under a Windows contrast theme): the
     browser repaints every background in the theme's Canvas colour, which
     erases the dots. They opt out and take the contrast theme's own system
     colours instead. */
  @media (forced-colors: active) {
    .dots i {
      forced-color-adjust: none;
      background: CanvasText;
    }

    .dots.active i,
    .dots.attention i {
      background: Highlight;
      box-shadow: none;
    }
  }
</style>
