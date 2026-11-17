<script lang="ts">
  import type { ClockText } from '$lib/icons/faces';
  import ShellIcon from '$lib/icons/ShellIcon.svelte';
  import type { PowerStatus } from '$lib/ipc/generated/PowerStatus';
  import type { VolumeStatus } from '$lib/ipc/generated/VolumeStatus';

  interface Props {
    /** The `shell:` icon's name without its prefix, or `null` for an app icon. */
    shellName: string | null;
    /** What there is to draw (DockItem's `data-icon`). */
    iconState: 'shell' | 'loaded' | 'loading' | 'none';
    /** DockItem's `iconUrl`: the bitmap at rest. */
    iconUrl: string | null | undefined;
    /** DockItem's `magnifiedIconUrl`: the bitmap while the box is magnified. */
    magnifiedIconUrl: string | null | undefined;
    /** The clock face's text (`shell:clock`). */
    clock: ClockText | null;
    power: PowerStatus | null;
    volume: VolumeStatus | null;
    /** The attention wiggle plays. */
    attention: boolean;
    /** The dock is vertical: the wiggle runs along it. */
    vertical: boolean;
  }

  let {
    shellName,
    iconState,
    iconUrl,
    magnifiedIconUrl,
    clock,
    power,
    volume,
    attention,
    vertical,
  }: Props = $props();

  /** A second, larger bitmap exists for magnification; `data-magnified` swaps to it. */
  const dual = $derived(iconState === 'loaded' && typeof magnifiedIconUrl === 'string');
</script>

<div class="wiggler" class:attention class:vertical>
  <div class="icon" class:dual>
    {#if shellName !== null}
      <ShellIcon name={shellName} {clock} {power} {volume} />
    {:else if iconState === 'loaded'}
      <!-- `decoding="sync"`: the hidden one of the pair is decoded in the
           paint that first shows it, so switching never leaves a blank frame. -->
      <img class="rest" src={iconUrl} alt="" draggable="false" decoding="sync" />
      {#if dual}
        <img class="magnified" src={magnifiedIconUrl} alt="" draggable="false" decoding="sync" />
      {/if}
    {:else}
      <div class="placeholder" class:unknown={iconState === 'none'}></div>
    {/if}
  </div>
</div>

<style>
  :global(.slot:focus-visible) .icon {
    outline: 2px solid var(--focus-ring);
    outline-offset: 3px;
    border-radius: 22%;
  }

  .wiggler,
  .icon {
    width: 100%;
    height: 100%;
  }

  .icon img {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: contain;
  }

  /* Two bitmaps: the exact-size one at rest, drawn 1:1; the
     ladder-size one while the box is magnified. The dock sets
     `data-magnified` on the slot imperatively (with `--scale`, in the same
     style flush), which is why the attribute is `:global` here. */
  .icon img.magnified {
    display: none;
  }

  :global(.slot[data-magnified]) .icon.dual img.rest {
    display: none;
  }

  :global(.slot[data-magnified]) .icon.dual img.magnified {
    display: block;
  }

  .placeholder {
    width: 100%;
    height: 100%;
    border-radius: 22%;
    background: var(--placeholder);
  }

  .placeholder.unknown {
    background:
      linear-gradient(135deg, transparent 46%, var(--separator) 46% 54%, transparent 54%),
      var(--placeholder);
  }

  .wiggler.attention {
    animation: wiggle 2s ease-in-out infinite;
  }

  /* Along a vertical dock. */
  .wiggler.attention.vertical {
    animation-name: wiggle-vertical;
  }

  @keyframes wiggle {
    0%,
    30%,
    100% {
      transform: translateX(0);
    }
    5% {
      transform: translateX(-4px);
    }
    10% {
      transform: translateX(4px);
    }
    15% {
      transform: translateX(-3px);
    }
    20% {
      transform: translateX(3px);
    }
    25% {
      transform: translateX(-1px);
    }
  }

  @keyframes wiggle-vertical {
    0%,
    30%,
    100% {
      transform: translateY(0);
    }
    5% {
      transform: translateY(-4px);
    }
    10% {
      transform: translateY(4px);
    }
    15% {
      transform: translateY(-3px);
    }
    20% {
      transform: translateY(3px);
    }
    25% {
      transform: translateY(-1px);
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .wiggler.attention,
    .wiggler.attention.vertical {
      animation: none;
    }
  }

  /* Forced colours (WebView2 under a Windows contrast theme): the
     placeholder takes an outline in the contrast theme's text colour. */
  @media (forced-colors: active) {
    .placeholder {
      outline: 1px solid CanvasText;
      outline-offset: -1px;
    }
  }
</style>
