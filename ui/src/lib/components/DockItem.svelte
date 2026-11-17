<script lang="ts" module>
  export { BOUNCE_MS } from './ItemBouncer.svelte';
</script>

<script lang="ts">
  import { axisOf, menuAnchor } from '$lib/dock/edge';
  import { itemTransition } from '$lib/dock/item-transition';
  import { clockText, msUntilNextMinute } from '$lib/icons/faces';
  import { isShellIconKey } from '$lib/ipc/backend';

  import type { DockItemProps } from './dock-item';
  import ItemBouncer from './ItemBouncer.svelte';
  import ItemDots from './ItemDots.svelte';
  import ItemIcon from './ItemIcon.svelte';
  import ItemTooltip from './ItemTooltip.svelte';

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
  }: DockItemProps = $props();

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
    <ItemTooltip {title} />
  {/if}
  <ItemBouncer {bounce} {animate} {edge}>
    <ItemIcon
      {shellName}
      {iconState}
      {iconUrl}
      {magnifiedIconUrl}
      {clock}
      {power}
      {volume}
      attention={attention && animate}
      vertical={axisOf(edge) === 'vertical'}
    />
  </ItemBouncer>
  <ItemDots count={dotCount} active={item.isActive} dim={allMinimised} {attention} />
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

  .slot[data-drag='lifted'] {
    z-index: 2;
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
</style>
