<script lang="ts">
  import type { Toast } from '$lib/stores/toasts.svelte';

  interface Props {
    /** The failure notices, oldest first (`Toasts.items`). */
    toasts: readonly Toast[];
    /** The panel is slid out (`AutoHideDriver.autoHidden`). */
    atEdge: boolean;
  }

  let { toasts, atEdge }: Props = $props();
</script>

<!-- Failure notices: at the far end of the window just beyond the panel's
     side away from the edge (right-aligned just above it on `bottom`),
     never in the way; while the panel is slid out they sit just inside
     the window's edge side and ride back out with it. -->
<div class="toasts" class:at-edge={atEdge} aria-live="polite">
  {#each toasts as toast (toast.id)}
    <div class="toast" role="status">{toast.message}</div>
  {/each}
</div>

<style>
  .toasts {
    position: absolute;
    left: 8px;
    right: 8px;
    /* The panel's top edge plus a small gap. */
    bottom: calc(var(--panel-line) + 6px);
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 4px;
    pointer-events: none;
    /* Follows the panel's slide: towards the edge by the panel box
       and the margin to the edge, so the notices end 6 px inside the edge
       side while the panel is out, and back with it. A transform, like the
       panel's own slide, so nothing is laid out again. */
    transition: transform var(--slide) ease-out;
  }

  .toasts.at-edge {
    transform: translateY(var(--panel-line));
    transition: transform var(--slide) ease-in;
  }

  /* On `top`: right-aligned just below the panel, the newest nearest it. */
  :global(.dock-stage[data-edge='top']) .toasts {
    bottom: auto;
    top: calc(var(--panel-line) + 6px);
    flex-direction: column-reverse;
  }

  :global(.dock-stage[data-edge='top']) .toasts.at-edge {
    transform: translateY(calc(-1 * var(--panel-line)));
  }

  /* On a side edge: at the bottom end of the window, just beside the panel. */
  :global(.dock-stage[data-edge='left']) .toasts,
  :global(.dock-stage[data-edge='right']) .toasts {
    top: 8px;
    bottom: 8px;
    justify-content: flex-end;
  }

  :global(.dock-stage[data-edge='left']) .toasts {
    left: calc(var(--panel-line) + 6px);
    align-items: flex-start;
  }

  :global(.dock-stage[data-edge='left']) .toasts.at-edge {
    transform: translateX(calc(-1 * var(--panel-line)));
  }

  :global(.dock-stage[data-edge='right']) .toasts {
    right: calc(var(--panel-line) + 6px);
  }

  :global(.dock-stage[data-edge='right']) .toasts.at-edge {
    transform: translateX(var(--panel-line));
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

    /* Both: `.toasts.at-edge` alone would win over `.toasts` and slide. */
    .toasts,
    .toasts.at-edge {
      transition: none;
    }
  }

  /* Forced colours (WebView2 under a Windows contrast theme): the
     browser repaints backgrounds in the theme's Canvas colour, so the
     toast's dot would vanish. It takes the theme's text colour instead. */
  @media (forced-colors: active) {
    .toast::before {
      forced-color-adjust: none;
      background: CanvasText;
    }
  }
</style>
