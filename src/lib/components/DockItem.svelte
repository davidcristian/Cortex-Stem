<script lang="ts">
  import ShellIcon from '$lib/icons/ShellIcon.svelte';
  import { isShellIconKey } from '$lib/ipc/backend';
  import type { DockItem } from '$lib/ipc/generated/DockItem';

  interface Props {
    item: DockItem;
    /** Resolved app icon: a data URL, `null` when there is none, `undefined` while loading. */
    iconUrl: string | null | undefined;
    /** `behavior.runningIndicators`. */
    showDots: boolean;
    /** Show the title pill above the icon now. */
    tooltip: boolean;
    /** Changes on every `dock://item-bounce` for this item; 0 = never. */
    bounce: number;
    /** False under `prefers-reduced-motion` (no bounce, no wiggle). */
    animate: boolean;
    /** Being dragged to a new place: raised, shadowed, no dots. */
    lifted: boolean;
    onprimary: (item: DockItem) => void;
    onmiddle: (item: DockItem) => void;
    oncontext: (item: DockItem, x: number, y: number) => void;
    onhover: (item: DockItem | null) => void;
    /** A pointer went down on the item (the dock may start a drag from it). */
    onpress: (item: DockItem, e: PointerEvent) => void;
  }

  let {
    item,
    iconUrl,
    showDots,
    tooltip,
    bounce,
    animate,
    lifted,
    onprimary,
    onmiddle,
    oncontext,
    onhover,
    onpress,
  }: Props = $props();

  const shellName = $derived(
    item.iconKey !== null && isShellIconKey(item.iconKey)
      ? item.iconKey.slice('shell:'.length)
      : null,
  );
  const iconState = $derived.by((): 'shell' | 'loaded' | 'loading' | 'none' => {
    if (shellName !== null) {
      return 'shell';
    }
    if (item.iconKey === null || iconUrl === null) {
      return 'none';
    }
    return iconUrl === undefined ? 'loading' : 'loaded';
  });
  const windowCount = $derived(item.windows.length);
  const dotCount = $derived(showDots && item.kind === 'app' ? Math.min(3, windowCount) : 0);
  const allMinimised = $derived(windowCount > 0 && item.windows.every((w) => w.isMinimized));
  const label = $derived.by(() => {
    const parts = [item.title];
    if (windowCount === 1) {
      parts.push('1 window');
    } else if (windowCount > 1) {
      parts.push(`${windowCount} windows`);
    }
    if (item.isActive) {
      parts.push('active');
    }
    if (item.needsAttention) {
      parts.push('needs attention');
    }
    return parts.join(', ');
  });

  let bouncer = $state<HTMLDivElement>();

  // Bounce through the Web Animations API: it restarts cleanly when a second
  // launch happens mid-flight and composes with the magnification transform,
  // which lives on a different element.
  $effect(() => {
    const el = bouncer;
    if (bounce === 0 || !animate || !el) {
      return;
    }
    const animation = el.animate(
      [
        { transform: 'translateY(0)', easing: 'ease-out', offset: 0 },
        { transform: 'translateY(-42%)', easing: 'ease-in', offset: 0.17 },
        { transform: 'translateY(0)', easing: 'ease-out', offset: 0.34 },
        { transform: 'translateY(-22%)', easing: 'ease-in', offset: 0.5 },
        { transform: 'translateY(0)', easing: 'ease-out', offset: 0.66 },
        { transform: 'translateY(-9%)', easing: 'ease-in', offset: 0.82 },
        { transform: 'translateY(0)', offset: 1 },
      ],
      { duration: 1200 },
    );
    return () => animation.cancel();
  });

  function onpointerup(e: PointerEvent): void {
    if (e.button === 0) {
      onprimary(item);
    } else if (e.button === 1 && item.kind === 'app') {
      onmiddle(item);
    }
  }

  function onmousedown(e: MouseEvent): void {
    // No middle-click autoscroll, no focus steal on click (keyboard focus still works).
    if (e.button === 1 || e.button === 0) {
      e.preventDefault();
    }
  }

  function oncontextmenu(e: MouseEvent): void {
    e.preventDefault();
    oncontext(item, e.clientX, e.clientY);
  }

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      onprimary(item);
    }
  }
</script>

<div
  class="slot"
  role="button"
  tabindex="0"
  aria-label={label}
  aria-pressed={item.isActive ? 'true' : undefined}
  data-key={item.key}
  data-kind={item.kind}
  data-icon={iconState}
  data-active={item.isActive || undefined}
  data-pinned={item.pinned || undefined}
  data-drag={lifted ? 'lifted' : undefined}
  onpointerdown={(e) => onpress(item, e)}
  {onpointerup}
  {onmousedown}
  {oncontextmenu}
  {onkeydown}
  onpointerenter={() => onhover(item)}
  onpointerleave={() => onhover(null)}
>
  {#if tooltip}
    <div class="tooltip" role="tooltip">{item.title}</div>
  {/if}
  <div class="bouncer" bind:this={bouncer}>
    <div class="wiggler" class:attention={item.needsAttention && animate}>
      <div class="icon">
        {#if shellName !== null}
          <ShellIcon name={shellName} />
        {:else if iconState === 'loaded'}
          <img src={iconUrl} alt="" draggable="false" decoding="async" />
        {:else}
          <div class="placeholder" class:unknown={iconState === 'none'}></div>
        {/if}
      </div>
    </div>
  </div>
  <div class="dots" class:active={item.isActive} class:dim={allMinimised} aria-hidden="true">
    {#each { length: dotCount } as _, i (i)}
      <i></i>
    {/each}
  </div>
</div>

<style>
  .slot {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: flex-end;
    box-sizing: border-box;
    /* Neighbours make room for a magnified icon: the slot grows with --scale. */
    width: calc(var(--icon) * var(--scale, 1) + var(--slot-gap));
    height: calc(var(--icon) + var(--dots-area));
    flex: none;
    border-radius: 8px;
    outline: none;
    cursor: default;
    -webkit-tap-highlight-color: transparent;
  }

  .slot:focus-visible .icon {
    outline: 2px solid var(--focus-ring);
    outline-offset: 3px;
    border-radius: 22%;
  }

  .bouncer,
  .wiggler,
  .icon {
    width: var(--icon);
    height: var(--icon);
  }

  /* The lift while dragging lives on the bouncer, a different element
     from the magnification scale, so the two never fight. The dock positions
     the slot itself with an inline transform. */
  .bouncer {
    transition:
      transform 120ms ease,
      filter 120ms ease;
  }

  .slot[data-drag='lifted'] {
    z-index: 2;
  }

  .slot[data-drag='lifted'] .bouncer {
    transform: scale(1.12);
    filter: drop-shadow(0 8px 12px rgba(0, 0, 0, 0.45));
  }

  .slot[data-drag='lifted'] .dots {
    opacity: 0;
  }

  .icon {
    transform: scale(var(--scale, 1));
    transform-origin: 50% 100%;
    will-change: transform;
  }

  .icon img {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: contain;
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

  .dots {
    display: flex;
    gap: 3px;
    height: 4px;
    margin-top: calc(var(--dots-area) - 4px);
  }

  .dots i {
    display: block;
    width: 4px;
    height: 4px;
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

  .tooltip {
    position: absolute;
    left: 50%;
    /* Above the icon, however far the magnification lifted its top edge. */
    bottom: calc(100% + 10px + var(--icon) * (var(--scale, 1) - 1));
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

  @media (prefers-reduced-motion: reduce) {
    .wiggler.attention {
      animation: none;
    }

    .bouncer {
      transition: none;
    }
  }
</style>
