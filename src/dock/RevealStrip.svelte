<script lang="ts">
  import { revealStripThickness, type AutoHidePhase } from '$lib/dock/autohide';

  interface Props {
    /** The resting panel's start and length along the dock, and its margin to the edge side. */
    stripBox: { start: number; length: number; gap: number };
    /** The auto-hide phase: the strip's thickness follows it (`revealStripThickness`). */
    autoHidePhase: AutoHidePhase;
    vertical: boolean;
    onpointerenter: () => void;
    onpointerleave: () => void;
  }

  let { stripBox, autoHidePhase, vertical, onpointerenter, onpointerleave }: Props = $props();
  const thickness = $derived(`${revealStripThickness(autoHidePhase, stripBox.gap)}px`);
</script>

<!-- The reveal strip (ADR-0008): the window's edge side beside the
     resting panel (its bottom on `bottom`, its left side on `left`).
     Hidden, the window takes the pointer only in its 2 px
     there; shown, it fills the margin between the panel and that side
     and counts as part of the dock, so leaving it towards the panel is
     entering the panel. None in fullscreen above a visible
     taskbar. Invisible. -->
<div
  class="reveal-strip"
  aria-hidden="true"
  style:left={vertical ? undefined : `${stripBox.start}px`}
  style:top={vertical ? `${stripBox.start}px` : undefined}
  style:width={vertical ? thickness : `${stripBox.length}px`}
  style:height={vertical ? `${stripBox.length}px` : thickness}
  {onpointerenter}
  {onpointerleave}
></div>

<style>
  /* Its start, length and thickness are set inline (`revealStripThickness`). */
  .reveal-strip {
    position: absolute;
  }

  :global(.dock-stage[data-edge='bottom']) .reveal-strip {
    bottom: 0;
  }

  :global(.dock-stage[data-edge='top']) .reveal-strip {
    top: 0;
  }

  :global(.dock-stage[data-edge='left']) .reveal-strip {
    left: 0;
  }

  :global(.dock-stage[data-edge='right']) .reveal-strip {
    right: 0;
  }
</style>
