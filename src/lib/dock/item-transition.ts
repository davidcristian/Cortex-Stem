/**
 * How dock items come and go: a slot grows from nothing to its rest
 * width while its icon scales up and fades in, and the reverse on the way
 * out, so a content-sized panel widens and narrows smoothly instead of
 * jumping. The transitions return `css`, so Svelte compiles them to CSS
 * keyframes that run on the compositor; nothing ticks in JavaScript.
 */

import { cubicOut } from 'svelte/easing';
import type { TransitionConfig } from 'svelte/transition';

export const ITEM_TRANSITION_MS = 220;
/** The icon's scale at the start of an intro / end of an outro. */
export const ITEM_START_SCALE = 0.6;

export interface ItemTransitionParams {
  /** `false` plays nothing (reduced motion, the first snapshot). */
  enabled: boolean;
}

/**
 * Inline CSS of an app slot at progress `t` (0 = gone, 1 = at rest). The
 * width is a `calc()` over the slot's own custom properties, so it lands on
 * exactly the stylesheet's rest width whatever the icon size and the
 * magnification scale of that slot at that moment.
 */
export function slotCss(t: number): string {
  const scale = ITEM_START_SCALE + (1 - ITEM_START_SCALE) * t;
  return (
    `width: calc((var(--icon) * var(--scale, 1) + var(--slot-gap)) * ${t}); ` +
    `opacity: ${t}; transform: scale(${scale})`
  );
}

/** Inline CSS of a separator at progress `t`: its 1 px line and its side margins. */
export function separatorCss(t: number): string {
  return (
    `width: ${t}px; margin-left: calc(var(--separator-margin) * ${t}); ` +
    `margin-right: calc(var(--separator-margin) * ${t}); opacity: ${t}`
  );
}

/**
 * Svelte transition for a `.slot` (`in:itemTransition` / `out:itemTransition`).
 * The element keeps playing after it left the item list, so the leaving one
 * is marked `data-leaving` the moment its outro starts: the dock excludes
 * such slots from magnification, drag bookkeeping and measurements. The mark
 * is cleared if the same item comes back mid-outro (Svelte resumes it).
 */
export function itemTransition(
  node: HTMLElement,
  params: ItemTransitionParams,
  options: { direction: 'in' | 'out' | 'both' },
): TransitionConfig {
  if (options.direction === 'out') {
    node.dataset.leaving = '';
  } else {
    delete node.dataset.leaving;
  }
  if (!params.enabled) {
    return { duration: 0 };
  }
  const css = node.dataset.kind === 'separator' ? separatorCss : slotCss;
  return { duration: ITEM_TRANSITION_MS, easing: cubicOut, css };
}
