/**
 * The mouse wheel over the dock's items
 * (docs/ipc/scrolling-over-an-item.md): pure, so `Dock.svelte` only finds
 * the item under the pointer, feeds each `wheel` event in here and sends a
 * `scroll_item` for every notch that comes out. What a notch does is Rust's
 * business.
 *
 * Vertical travel is added up in CSS pixels: `deltaY` as it is for
 * `deltaMode` 0 (pixels), times 40 for 1 (lines) and times the viewport's
 * height for 2 (pages). Every 100 px is one notch, `next` for travel towards
 * the user (a positive `deltaY`: the wheel turned down, two fingers drawn
 * towards you on a touchpad), `previous` away; the rest is kept for the
 * next event, so a touchpad's small steps add up to notches too. The rest
 * belongs to one item: over another item, or over none, the count starts
 * again. An event that goes more sideways than up or down (a tilt wheel,
 * Shift and the wheel, a sideways swipe) is not a vertical scroll and
 * counts for nothing.
 */

import type { ScrollDirection } from '$lib/ipc/generated/ScrollDirection';

/** Vertical travel per notch, in CSS px: one `scroll_item` each. */
export const NOTCH_PX = 100;

/** CSS px per line for `WheelEvent.DOM_DELTA_LINE`. */
export const LINE_PX = 40;

/**
 * The most notches one event yields: a page of travel on a tall viewport,
 * or a device that reports nonsense, sends no more than this (the service
 * lets one activation through per 150 ms anyway).
 */
export const MAX_NOTCHES_PER_EVENT = 10;

/** `WheelEvent.deltaMode` values (`DOM_DELTA_*`; the constants are not available in node). */
const DELTA_LINE = 1;
const DELTA_PAGE = 2;

/** What the accumulator needs of a `WheelEvent`. */
export interface WheelInput {
  /** The item under the pointer (`data-key` of its slot), or `null` over none. */
  readonly key: string | null;
  readonly deltaX: number;
  readonly deltaY: number;
  readonly deltaMode: number;
  /** The viewport's height in CSS px: one page of `deltaMode` 2. */
  readonly pageHeight: number;
}

/**
 * `deltaY` in CSS px for `deltaMode`: as it is for pixels (and for a mode
 * this does not know), × `LINE_PX` for lines, × `pageHeight` for pages.
 * Anything not finite, a page height that is not positive included, is 0.
 */
export function wheelPixels(deltaY: number, deltaMode: number, pageHeight: number): number {
  let px = deltaY;
  if (deltaMode === DELTA_LINE) {
    px = deltaY * LINE_PX;
  } else if (deltaMode === DELTA_PAGE) {
    px = pageHeight > 0 ? deltaY * pageHeight : 0;
  }
  return Number.isFinite(px) ? px : 0;
}

/** Whether the event is a vertical scroll: more travel up or down than sideways. */
export function isVertical(deltaX: number, deltaY: number): boolean {
  return Math.abs(deltaY) > 0 && !(Math.abs(deltaX) > Math.abs(deltaY));
}

/** Adds wheel travel up and turns it into notches; one per dock page. */
export class WheelNotches {
  private key: string | null = null;
  /** Travel since the last notch over `key`, in CSS px (−100 < rest < 100). */
  private rest = 0;

  /**
   * The notches `input` completes, oldest first (empty for most of a
   * touchpad's events). Over no item, or for an event that is not a
   * vertical scroll, none: the first also drops the rest, the second
   * leaves it.
   */
  add(input: WheelInput): ScrollDirection[] {
    if (input.key === null) {
      this.reset();
      return [];
    }
    if (input.key !== this.key) {
      this.key = input.key;
      this.rest = 0;
    }
    if (!isVertical(input.deltaX, input.deltaY)) {
      return [];
    }
    this.rest += wheelPixels(input.deltaY, input.deltaMode, input.pageHeight);
    const whole = Math.trunc(this.rest / NOTCH_PX);
    if (whole === 0) {
      return [];
    }
    this.rest -= whole * NOTCH_PX;
    const count = Math.min(Math.abs(whole), MAX_NOTCHES_PER_EVENT);
    return Array.from({ length: count }, () => (whole > 0 ? 'next' : 'previous'));
  }

  /** Forget the item and the rest (the setting went off, the pointer left). */
  reset(): void {
    this.key = null;
    this.rest = 0;
  }
}
