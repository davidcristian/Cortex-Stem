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

  /* Forced colours (WebView2 under a Windows contrast theme): the
     browser repaints every background in the theme's Canvas colour, which
     turns the arrow (transparent borders) into a block. It opts out and
     takes the contrast theme's text colour instead. */
  @media (forced-colors: active) {
    .tooltip::after {
      forced-color-adjust: none;
      border-top-color: CanvasText;
    }
  }
</style>
