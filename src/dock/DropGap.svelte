<script lang="ts">
  import type { DockAxis } from '$lib/dock/edge';
  import { gapTransition } from '$lib/dock/item-transition';

  interface Props {
    /** Items animate in and out (no reduced motion). */
    itemMotion: boolean;
    axis: DockAxis;
    /** The item that took the gap's place, if one did: the gap then goes at once. */
    landedKey: string | null;
    onoutroend: () => void;
  }

  let { itemMotion, axis, landedKey, onoutroend }: Props = $props();
</script>

<!-- The drop gap: room for the files a drop will pin, with an
     outline where the first new icon will sit. -->
<div
  class="drop-gap"
  aria-hidden="true"
  in:gapTransition|global={{ enabled: itemMotion, axis }}
  out:gapTransition|global={{ enabled: itemMotion && landedKey === null, axis }}
  {onoutroend}
>
  <div class="drop-indicator"></div>
</div>

<style>
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

  :global(.dock-stage[data-edge='top']) .drop-gap {
    align-items: flex-start;
    transform-origin: 50% 0;
  }

  :global(.dock-stage[data-edge='left']) .drop-gap,
  :global(.dock-stage[data-edge='right']) .drop-gap {
    align-items: center;
    width: var(--icon);
    height: calc(var(--icon) + var(--slot-gap));
  }

  :global(.dock-stage[data-edge='left']) .drop-gap {
    transform-origin: 0 50%;
  }

  :global(.dock-stage[data-edge='right']) .drop-gap {
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

  :global(.dock-panel.high-contrast) .drop-indicator {
    border-color: var(--text);
  }
</style>
