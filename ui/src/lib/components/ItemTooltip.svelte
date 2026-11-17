<script lang="ts">
  interface Props {
    /** The pill's text: the item's title (the clock's full date and time). */
    title: string;
  }

  let { title }: Props = $props();
</script>

<div class="tooltip" role="tooltip">{title}</div>

<style>
  .tooltip {
    position: absolute;
    left: 50%;
    /* Above the icon, however far the magnification (or a drop target's
       growth) lifted its top edge. */
    bottom: calc(100% + 10px + var(--icon) * (var(--scale, 1) - 1 + var(--drop-lift, 0)));
    transform: translateX(-50%);
    padding: 4px 9px;
    border-radius: 7px;
    border: 1px solid var(--tooltip-border);
    background: var(--tooltip-bg);
    color: var(--text);
    font-size: 12px;
    font-weight: 500;
    white-space: nowrap;
    box-shadow: 0 3px 10px rgba(0, 0, 0, 0.28);
    pointer-events: none;
    z-index: 1;
  }

  .tooltip::after {
    content: '';
    position: absolute;
    left: 50%;
    top: 100%;
    transform: translateX(-50%);
    border: 5px solid transparent;
    border-top-color: var(--tooltip-bg);
  }

  /* On `top`, `left` and `right` the pill sits on the icon's side away from
     the edge (DockStage.svelte says how the dock lays its items out there). */
  :global(.dock-stage[data-edge='top'] .dock-panel .slot) .tooltip {
    bottom: auto;
    top: calc(100% + 10px + var(--icon) * (var(--scale, 1) - 1 + var(--drop-lift, 0)));
  }

  :global(.dock-stage[data-edge='top'] .dock-panel .slot) .tooltip::after {
    top: auto;
    bottom: 100%;
    border-top-color: transparent;
    border-bottom-color: var(--tooltip-bg);
  }

  /* Beside the icon there is no window length to spare: a long title
     wraps at `--pill-max`, inside the room the window keeps for it
     (`SIDE_HEADROOM`). */
  :global(.dock-stage[data-edge='left'] .dock-panel .slot) .tooltip,
  :global(.dock-stage[data-edge='right'] .dock-panel .slot) .tooltip {
    bottom: auto;
    top: 50%;
    transform: translateY(-50%);
    width: max-content;
    max-width: var(--pill-max);
    white-space: normal;
    /* Lines of even length: a 12-hour clock's title leaves no lone "AM"
       on its last line. After `white-space`, which resets it. The
       box keeps the width of the longest line greedy wrapping would have
       made, so the shorter balanced lines are centred in it. */
    text-wrap: balance;
    text-align: center;
  }

  :global(.dock-stage[data-edge='left'] .dock-panel .slot) .tooltip {
    left: calc(100% + 10px + var(--icon) * (var(--scale, 1) - 1 + var(--drop-lift, 0)));
  }

  :global(.dock-stage[data-edge='right'] .dock-panel .slot) .tooltip {
    left: auto;
    right: calc(100% + 10px + var(--icon) * (var(--scale, 1) - 1 + var(--drop-lift, 0)));
  }

  :global(.dock-stage[data-edge='left'] .dock-panel .slot) .tooltip::after,
  :global(.dock-stage[data-edge='right'] .dock-panel .slot) .tooltip::after {
    top: 50%;
    transform: translateY(-50%);
    border-top-color: transparent;
  }

  :global(.dock-stage[data-edge='left'] .dock-panel .slot) .tooltip::after {
    left: auto;
    right: 100%;
    border-right-color: var(--tooltip-bg);
  }

  :global(.dock-stage[data-edge='right'] .dock-panel .slot) .tooltip::after {
    left: 100%;
    border-left-color: var(--tooltip-bg);
  }

  /* Forced colours (WebView2 under a Windows contrast theme): the
     browser repaints every background in the theme's Canvas colour, which
     turns the arrow (transparent borders) into a block. It opts out and
     takes the contrast theme's text colour instead, on every edge. */
  @media (forced-colors: active) {
    .tooltip::after {
      forced-color-adjust: none;
      border-top-color: CanvasText;
    }

    :global(.dock-stage[data-edge='top'] .dock-panel .slot) .tooltip::after {
      border-bottom-color: CanvasText;
    }

    :global(.dock-stage[data-edge='left'] .dock-panel .slot) .tooltip::after {
      border-right-color: CanvasText;
    }

    :global(.dock-stage[data-edge='right'] .dock-panel .slot) .tooltip::after {
      border-left-color: CanvasText;
    }
  }
</style>
