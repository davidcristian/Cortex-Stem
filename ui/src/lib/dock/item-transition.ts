/**
 * How dock items come and go: a slot grows from nothing to its rest
 * length while its icon scales up and fades in, and the reverse on the way
 * out, so a content-sized panel lengthens and shortens smoothly instead of
 * jumping. The transitions return `css`, so Svelte compiles them to CSS
 * keyframes that run on the compositor; nothing ticks in JavaScript.
 *
 * The length is along the dock's main axis: the width on a
 * horizontal dock, the height on a vertical one.
 */

import { cubicOut } from 'svelte/easing';
import type { TransitionConfig } from 'svelte/transition';

import type { DockAxis } from './edge';

export const ITEM_TRANSITION_MS = 220;
/** The icon's scale at the start of an intro / end of an outro. */
export const ITEM_START_SCALE = 0.6;

export interface ItemTransitionParams {
  /** `false` plays nothing (reduced motion, the first snapshot). */
  enabled: boolean;
  /**
   * The dock's main axis. Absent: the nearest `[data-axis]` ancestor's
   * (`axisOfNode`; the dock panel carries it), so a slot whose component
   * does not pass it still grows along the right axis.
   */
  axis?: DockAxis;
}

/**
 * The main axis of the dock `node` is in: the `data-axis` of its nearest
 * ancestor that has one (the dock panel), `horizontal` when there is none.
 */
export function axisOfNode(node: Element): DockAxis {
  const owner = node.closest<HTMLElement>('[data-axis]');
  return owner?.dataset.axis === 'vertical' ? 'vertical' : 'horizontal';
}

/**
 * Inline CSS of an app slot at progress `t` (0 = gone, 1 = at rest). The
 * length is a `calc()` over the slot's own custom properties, so it lands
 * on exactly the stylesheet's rest length whatever the icon size and the
 * magnification scale of that slot at that moment.
 */
export function slotCss(t: number, axis: DockAxis = 'horizontal'): string {
  const scale = ITEM_START_SCALE + (1 - ITEM_START_SCALE) * t;
  const length = axis === 'horizontal' ? 'width' : 'height';
  return (
    `${length}: calc((var(--icon) * var(--scale, 1) + var(--slot-gap)) * ${t}); ` +
    `opacity: ${t}; transform: scale(${scale})`
  );
}

/** Inline CSS of a separator at progress `t`: its 1 px line and its margins along the dock. */
export function separatorCss(t: number, axis: DockAxis = 'horizontal'): string {
  const [length, before, after] =
    axis === 'horizontal'
      ? ['width', 'margin-left', 'margin-right']
      : ['height', 'margin-top', 'margin-bottom'];
  return (
    `${length}: ${t}px; ${before}: calc(var(--separator-margin) * ${t}); ` +
    `${after}: calc(var(--separator-margin) * ${t}); opacity: ${t}`
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
  const axis = params.axis ?? axisOfNode(node);
  const css = node.dataset.kind === 'separator' ? separatorCss : slotCss;
  return { duration: ITEM_TRANSITION_MS, easing: cubicOut, css: (t) => css(t, axis) };
}

/** The drop gap opens and closes at the pace of the reorder gap (`transform 160ms`). */
export const GAP_TRANSITION_MS = 160;

/**
 * Inline CSS of the drop gap at progress `t`: it grows and shrinks like an
 * item does, to exactly one slot at rest (the gap is never magnified).
 */
export function gapCss(t: number, axis: DockAxis = 'horizontal'): string {
  const scale = ITEM_START_SCALE + (1 - ITEM_START_SCALE) * t;
  const length = axis === 'horizontal' ? 'width' : 'height';
  return (
    `${length}: calc((var(--icon) + var(--slot-gap)) * ${t}); ` +
    `opacity: ${t}; transform: scale(${scale})`
  );
}

/** Svelte transition for the `.drop-gap` element (`in:` and `out:`). */
export function gapTransition(node: HTMLElement, params: ItemTransitionParams): TransitionConfig {
  if (!params.enabled) {
    return { duration: 0 };
  }
  const axis = params.axis ?? axisOfNode(node);
  return { duration: GAP_TRANSITION_MS, easing: cubicOut, css: (t) => gapCss(t, axis) };
}
