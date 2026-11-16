<script lang="ts" module>
  /** Length of the launch bounce; the dock stays up for it under auto-hide. */
  export const BOUNCE_MS = 1200;
</script>

<script lang="ts">
  import { untrack, type Snippet } from 'svelte';

  import { awayTranslate, type DockEdge } from '$lib/dock/edge';

  interface Props {
    /** DockItem's `bounce`: changes on every launch bounce of the item; 0 = never. */
    bounce: number;
    /** DockItem's `animate`: false under `prefers-reduced-motion`. */
    animate: boolean;
    /** The screen edge the dock sits on: the bounce moves away from it. */
    edge: DockEdge;
    children: Snippet;
  }

  let { bounce, animate, edge, children }: Props = $props();

  let bouncer = $state<HTMLDivElement>();

  // The bounce value this item has played or was mounted with. A bounce is
  // an event, not a state: only a value that changes plays, so an item that
  // mounts later under the key of the last bounce (an app opened again, a
  // window moved to another monitor's dock) or *Bounce on launch* turned
  // off and on again does not replay it.
  let seenBounce = untrack(() => bounce);

  // Bounce through the Web Animations API: it restarts cleanly when a second
  // launch happens mid-flight and composes with the magnification, which
  // sizes the same element rather than transforming it. Away from the edge;
  // the edge is read untracked, so moving the dock does not replay
  // the last bounce.
  $effect(() => {
    const el = bouncer;
    const serial = bounce;
    if (serial === 0 || serial === seenBounce) {
      return;
    }
    seenBounce = serial;
    if (!animate || !el) {
      return;
    }
    const away = (percent: number): string => untrack(() => awayTranslate(edge, percent));
    const animation = el.animate(
      [
        { transform: away(0), easing: 'ease-out', offset: 0 },
        { transform: away(42), easing: 'ease-in', offset: 0.17 },
        { transform: away(0), easing: 'ease-out', offset: 0.34 },
        { transform: away(22), easing: 'ease-in', offset: 0.5 },
        { transform: away(0), easing: 'ease-out', offset: 0.66 },
        { transform: away(9), easing: 'ease-in', offset: 0.82 },
        { transform: away(0), offset: 1 },
      ],
      { duration: BOUNCE_MS },
    );
    return () => animation.cancel();
  });
</script>

<div class="bouncer" bind:this={bouncer}>
  {@render children()}
</div>

<style>
  /* True-size magnification: the box is laid out at its magnified
     size, so the bitmap inside is resampled by the image decoder at that size
     instead of a composited layer being stretched by a transform. */
  .bouncer {
    flex: none;
    width: calc(var(--icon) * var(--scale, 1));
    height: calc(var(--icon) * var(--scale, 1));
  }

  /* The lift while dragging lives on the bouncer, a different element
     from the slot the dock moves with an inline transform, so the two never
     fight. */
  .bouncer {
    transition:
      transform 120ms ease,
      filter 120ms ease;
  }

  :global(.slot[data-drag='lifted']) .bouncer {
    transform: scale(1.12);
    filter: drop-shadow(0 8px 12px rgba(0, 0, 0, 0.45));
  }

  :global(.slot[data-drop-target]) .bouncer {
    transform: translateY(calc(-50% * var(--drop-lift))) scale(calc(1 + var(--drop-lift)));
  }

  @media (prefers-reduced-motion: reduce) {
    .bouncer {
      transition: none;
    }
  }
</style>
