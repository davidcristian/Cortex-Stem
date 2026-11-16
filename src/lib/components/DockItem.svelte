<script lang="ts" module>
  /** Length of the launch bounce; the dock stays up for it under auto-hide. */
  export const BOUNCE_MS = 1200;
</script>

<script lang="ts">
  import { untrack } from 'svelte';

  import { awayTranslate, axisOf, menuAnchor, type DockEdge } from '$lib/dock/edge';
  import { itemTransition } from '$lib/dock/item-transition';
  import { clockText, msUntilNextMinute } from '$lib/icons/faces';
  import ShellIcon from '$lib/icons/ShellIcon.svelte';
  import { isShellIconKey } from '$lib/ipc/backend';
  import type { ClockFormat } from '$lib/ipc/generated/ClockFormat';
  import type { DockItem } from '$lib/ipc/generated/DockItem';
  import type { PowerStatus } from '$lib/ipc/generated/PowerStatus';
  import type { VolumeStatus } from '$lib/ipc/generated/VolumeStatus';

  interface Props {
    item: DockItem;
    /**
     * Resolved app icon at exactly its rest size in device pixels: a data
     * URL, `null` when there is none, `undefined` while loading.
     */
    iconUrl: string | null | undefined;
    /**
     * The ladder-size bitmap shown while the box is magnified:
     * `null` when the rest bitmap serves both (the sizes coincide, or
     * magnification is off), `undefined` while loading. Both images stay in
     * the DOM; the dock's magnification controller switches them by setting
     * `data-magnified` on the slot, so a switch is a style change, not a load.
     */
    magnifiedIconUrl: string | null | undefined;
    /** `behavior.runningIndicators`. */
    showDots: boolean;
    /** Show the title pill above the icon now. */
    tooltip: boolean;
    /** Changes on every `dock://item-bounce` for this item; 0 = never. */
    bounce: number;
    /**
     * False under `prefers-reduced-motion` (no bounce, no wiggle, no
     * enter/leave). The attention wiggle follows only this: the
     * launch bounce has its own setting (`appearance.bounceOnLaunch`).
     */
    animate: boolean;
    /**
     * Play the enter animation if this item appears now. Off until the dock
     * has rendered its first snapshot, so startup does not cascade icons in.
     */
    enter: boolean;
    /** Being dragged to a new place: raised, shadowed, no dots. */
    lifted: boolean;
    /**
     * The screen edge the dock sits on: the launch bounce moves away
     * from it, the attention wiggle along the dock, and the keyboard's menu
     * opens on the item's side away from it. Default `bottom`.
     */
    edge?: DockEdge;
    /**
     * The toolbar's one tab stop (a roving tabindex): `tabindex` 0
     * for this item, -1 for the others. The dock moves it with the focus.
     */
    tabStop: boolean;
    /**
     * A file drag points at this item and a drop would land on it
     * (the Recycle Bin): the icon grows a little, as under the pointer. A
     * transform only, so nothing reflows while the drag moves.
     */
    dropTarget: boolean;
    /**
     * `DockState.power`: the battery item draws its face from it.
     * `null` (the default) before the page has a state.
     */
    power?: PowerStatus | null;
    /**
     * `DockState.volume`: the volume item draws its face from it;
     * `null` (the default) without an output device or before the page has
     * a state.
     */
    volume?: VolumeStatus | null;
    /**
     * `DockState.clockFormat`: the clock item formats its face, its
     * title pill and its accessible name in it. `null` (the default) before
     * the page has a state: the page's own locale.
     */
    clockFormat?: ClockFormat | null;
    onprimary: (item: DockItem) => void;
    onmiddle: (item: DockItem) => void;
    /**
     * Open the item's native menu at (`x`, `y`), CSS px of the viewport: the
     * pointer for a right-click; for Shift+F10 and the context menu key,
     * the middle of the item's side away from the edge (`menuAnchor`:
     * its horizontal centre and top edge on `bottom`).
     */
    oncontext: (item: DockItem, x: number, y: number) => void;
    onhover: (item: DockItem | null) => void;
    /** A pointer went down on the item (the dock may start a drag from it). */
    onpress: (item: DockItem, e: PointerEvent) => void;
    /** An enter or leave animation of this slot has finished (the panel is at rest again). */
    onsettled: () => void;
  }

  let {
    item,
    iconUrl,
    magnifiedIconUrl,
    showDots,
    tooltip,
    bounce,
    animate,
    enter,
    lifted,
    edge = 'bottom',
    tabStop,
    dropTarget,
    power = null,
    volume = null,
    clockFormat = null,
    onprimary,
    onmiddle,
    oncontext,
    onhover,
    onpress,
    onsettled,
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
  /** A second, larger bitmap exists for magnification; `data-magnified` swaps to it. */
  const dual = $derived(iconState === 'loaded' && typeof magnifiedIconUrl === 'string');
  const windowCount = $derived(item.windows.length);

  /**
   * The clock item reads the page's own clock and writes it in the
   * user's regional format (`clockFormat`; a new one is applied at once,
   * without waiting for the minute): `now` is set when the item mounts, at
   * each minute boundary by one `setTimeout`, and when the page becomes
   * visible again (after a sleep the timer may be late). Only a clock item
   * runs the timer, and only while it is mounted.
   */
  const isClock = $derived(item.kind === 'clock');
  let now = $state<Date | null>(null);
  $effect(() => {
    if (!isClock) {
      return;
    }
    let timer: ReturnType<typeof setTimeout> | undefined;
    const tick = (): void => {
      const current = new Date();
      now = current;
      timer = setTimeout(tick, msUntilNextMinute(current));
    };
    const onVisibility = (): void => {
      if (document.visibilityState === 'visible') {
        clearTimeout(timer);
        tick();
      }
    };
    tick();
    document.addEventListener('visibilitychange', onVisibility);
    return () => {
      clearTimeout(timer);
      document.removeEventListener('visibilitychange', onVisibility);
      now = null;
    };
  });
  // Every `dock://state` brings a new `clockFormat` object; the face is
  // rebuilt only when one of its two values changes.
  const clockLocale = $derived(clockFormat?.locale ?? null);
  const clockHour12 = $derived(clockFormat?.hour12 ?? null);
  const clock = $derived(
    isClock && now !== null ? clockText(now, { locale: clockLocale, hour12: clockHour12 }) : null,
  );
  /** The title pill's text and the accessible name's start: the clock's is the full date and time. */
  const title = $derived(clock?.full ?? item.title);
  /**
   * An app asking for attention also gets a still cue: its dots give
   * way to one short pill in the attention colour, shown even with the
   * running indicators off. The wiggle alone would leave nothing to see under
   * reduced motion, and the pill differs from the dots by its shape, so it
   * reads whatever the accent colour is.
   */
  const attention = $derived(item.kind === 'app' && item.needsAttention);
  const dotCount = $derived.by(() => {
    if (item.kind !== 'app') {
      return 0;
    }
    if (attention) {
      return 1;
    }
    return showDots ? Math.min(3, windowCount) : 0;
  });
  const allMinimised = $derived(windowCount > 0 && item.windows.every((w) => w.isMinimized));
  const label = $derived.by(() => {
    const parts = [title];
    if (windowCount === 1) {
      parts.push('1 window');
    } else if (windowCount > 1) {
      parts.push(`${windowCount} windows`);
    }
    if (allMinimised) {
      parts.push('minimised');
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

  function onkeydown(e: KeyboardEvent & { currentTarget: HTMLElement }): void {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      onprimary(item);
      return;
    }
    // The keyboard's way to the menu: Shift+F10 or the context menu
    // key, at the middle of the slot's side away from the edge (there is no
    // pointer position; its top edge on `bottom`), so a menu that has
    // to open away from the screen edge sits just beside the icon instead of
    // over it. `preventDefault` keeps Chromium from also sending its own
    // `contextmenu` for Shift+F10 (it does so for an unhandled key down).
    const shiftF10 = e.key === 'F10' && e.shiftKey && !e.ctrlKey && !e.altKey && !e.metaKey;
    if (shiftF10 || e.key === 'ContextMenu') {
      e.preventDefault();
      const { x, y } = menuAnchor(edge, e.currentTarget.getBoundingClientRect());
      oncontext(item, x, y);
    }
  }

  function onkeyup(e: KeyboardEvent): void {
    // On Windows Chromium sends the context menu key's `contextmenu` on the
    // key's release, unless the release is handled: the menu is already open
    // from the key down, and must not be asked for twice.
    if (e.key === 'ContextMenu') {
      e.preventDefault();
    }
  }
</script>

<!-- `in:` and `out:` rather than `transition:` so the function learns the
     direction and can mark the slot as leaving (see item-transition.ts).
     `|global`: the dock's item list decides when this slot comes and goes,
     and an `{#if}` sits between that `{#each}` and this element, which a
     local transition would not play through. -->
<div
  class="slot"
  role="button"
  tabindex={tabStop ? 0 : -1}
  aria-label={label}
  aria-pressed={item.isActive ? 'true' : undefined}
  data-key={item.key}
  data-kind={item.kind}
  data-icon={iconState}
  data-active={item.isActive || undefined}
  data-attention={attention || undefined}
  data-pinned={item.pinned || undefined}
  data-drag={lifted ? 'lifted' : undefined}
  data-drop-target={dropTarget || undefined}
  in:itemTransition|global={{ enabled: animate && enter }}
  out:itemTransition|global={{ enabled: animate }}
  onintroend={onsettled}
  onoutroend={onsettled}
  onpointerdown={(e) => onpress(item, e)}
  {onpointerup}
  {onmousedown}
  {oncontextmenu}
  {onkeydown}
  {onkeyup}
  onpointerenter={() => onhover(item)}
  onpointerleave={() => onhover(null)}
>
  {#if tooltip}
    <div class="tooltip" role="tooltip">{title}</div>
  {/if}
  <div class="bouncer" bind:this={bouncer}>
    <div
      class="wiggler"
      class:attention={attention && animate}
      class:vertical={axisOf(edge) === 'vertical'}
    >
      <div class="icon" class:dual>
        {#if shellName !== null}
          <ShellIcon name={shellName} {clock} {power} {volume} />
        {:else if iconState === 'loaded'}
          <!-- `decoding="sync"`: the hidden one of the pair is decoded in the
               paint that first shows it, so switching never leaves a blank frame. -->
          <img class="rest" src={iconUrl} alt="" draggable="false" decoding="sync" />
          {#if dual}
            <img
              class="magnified"
              src={magnifiedIconUrl}
              alt=""
              draggable="false"
              decoding="sync"
            />
          {/if}
        {:else}
          <div class="placeholder" class:unknown={iconState === 'none'}></div>
        {/if}
      </div>
    </div>
  </div>
  <div
    class="dots"
    class:active={item.isActive}
    class:dim={allMinimised}
    class:attention
    aria-hidden="true"
  >
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
    /* The icon sits on the slot's bottom edge and grows upwards out of it. */
    justify-content: flex-end;
    box-sizing: border-box;
    /* Neighbours make room for a magnified icon: the slot grows with --scale. */
    width: calc(var(--icon) * var(--scale, 1) + var(--slot-gap));
    height: var(--icon);
    flex: none;
    border-radius: 8px;
    outline: none;
    cursor: default;
    -webkit-tap-highlight-color: transparent;
    /* The enter/leave keyframes scale the whole slot from its bottom centre. */
    transform-origin: 50% 100%;
  }

  .slot:focus-visible .icon {
    outline: 2px solid var(--focus-ring);
    outline-offset: 3px;
    border-radius: 22%;
  }

  /* True-size magnification: the box is laid out at its magnified
     size, so the bitmap inside is resampled by the image decoder at that size
     instead of a composited layer being stretched by a transform. */
  .bouncer {
    flex: none;
    width: calc(var(--icon) * var(--scale, 1));
    height: calc(var(--icon) * var(--scale, 1));
  }

  .wiggler,
  .icon {
    width: 100%;
    height: 100%;
  }

  /* The lift while dragging lives on the bouncer, a different element
     from the slot the dock moves with an inline transform, so the two never
     fight. */
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

  /* A file drop would land here (the Recycle Bin): the icon grows
     from its bottom edge like a magnified one, by a transform on the
     bouncer so the slot keeps its size and nothing moves under the drag.
     Scaled about the centre and raised by half the growth, so the bottom
     stays put without changing `transform-origin`, which would make the
     icon jump when the highlight eases away. `--drop-lift` also raises the
     tooltip above the grown icon. */
  .slot[data-drop-target] {
    --drop-lift: 0.2;
  }

  .slot[data-drop-target] .bouncer {
    transform: translateY(calc(-50% * var(--drop-lift))) scale(calc(1 + var(--drop-lift)));
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

  .slot:global([data-magnified]) .icon.dual img.rest {
    display: none;
  }

  .slot:global([data-magnified]) .icon.dual img.magnified {
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

  @media (prefers-reduced-motion: reduce) {
    .wiggler.attention,
    .wiggler.attention.vertical {
      animation: none;
    }

    .bouncer {
      transition: none;
    }
  }

  /* Forced colours (WebView2 under a Windows contrast theme): the
     browser repaints every background in the theme's Canvas colour, which
     erases the dots and turns the tooltip's arrow (transparent borders) into
     a block. These parts opt out and take the contrast theme's own system
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

    .tooltip::after {
      forced-color-adjust: none;
      border-top-color: CanvasText;
    }

    .placeholder {
      outline: 1px solid CanvasText;
      outline-offset: -1px;
    }
  }
</style>
