/**
 * The mouse wheel over the dock's items
 * (docs/ipc/scrolling-over-an-item.md): pure, so `intents.svelte.ts` only
 * finds the item under the pointer, feeds each `wheel` event in here and
 * sends a `scroll_item` for every notch that comes out. What a notch does
 * is Rust's business.
 *
 * Vertical travel is added up in CSS pixels: `deltaY` as it is for
 * `deltaMode` 0 (pixels), times 40 for 1 (lines) and times the viewport's
 * height for 2 (pages). Every 100 px is one notch, `next` for a positive
 * `deltaY` (the wheel turned towards the user, a touchpad scroll that would
 * move a page down), `previous` for a negative one; the rest is kept for
 * the next event, so a touchpad's small steps add up to notches too.
 *
 * A notch of a real wheel is not always 100 px: Chromium on Windows (so
 * WebView2) makes it Windows' *lines to scroll at a time* × 100/3 px, in
 * single precision, which is 100 only at the default of three lines. So an
 * event that travels at least 30 px on its own is one notch even when it
 * is shorter (33⅓ px at one line, 66⅔ at two), and then leaves no rest; a
 * total within half a pixel of a notch counts as one (three single-
 * precision thirds add up to just under 100). A touchpad's steps are mostly
 * smaller than 30 px and still add up.
 *
 * The rest belongs to one item and one direction: over another item, over
 * none, or for an event the other way, the count starts again. An event
 * that goes more sideways than up or down (a tilt wheel, Shift and the
 * wheel, a sideways swipe) is not a vertical scroll and counts for nothing.
 * Nor does one with `ctrlKey` set: Chromium reports a touchpad's
 * pinch as wheel events with `ctrlKey` set, whether Ctrl is held or not,
 * and Ctrl and the wheel is a zoom, not a scroll. Both leave the rest as
 * it was, and the caller leaves such an event alone (it sends nothing, so
 * it does not prevent its default either).
 *
 * Whether the wheel over an item sends anything at all is [`wheelSends`]:
 * over the volume item always (v1.30: Rust steps the volume whatever
 * `behavior.scrollAction` says), over any other item only while that
 * setting is not `none`.
 */

import type { DockItemKind } from '$lib/ipc/generated/DockItemKind';
import type { ScrollAction } from '$lib/ipc/generated/ScrollAction';
import type { ScrollDirection } from '$lib/ipc/generated/ScrollDirection';

/** Vertical travel per notch, in CSS px: one `scroll_item` each. */
export const NOTCH_PX = 100;

/**
 * An event whose own vertical travel is at least this many CSS px is one
 * notch at least: a wheel notch at Windows' one line to scroll (33⅓ px,
 * the least it can be) or more.
 */
export const NOTCH_EVENT_PX = 30;

/** Travel this close under a whole number of notches counts as that many (single-precision deltas). */
export const NOTCH_SLACK_PX = 0.5;

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
  /** `WheelEvent.ctrlKey`: a touchpad pinch, or Ctrl held with the wheel; never a scroll. */
  readonly ctrlKey: boolean;
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

/**
 * Whether the wheel over an item of `kind` (`null`: over no item) sends
 * `scroll_item` under `scrollAction`: over the volume item whatever the
 * setting, over other items only while it is not `none`, over no item
 * never.
 */
export function wheelSends(scrollAction: ScrollAction, kind: DockItemKind | null): boolean {
  if (kind === null) {
    return false;
  }
  return kind === 'volume' || scrollAction !== 'none';
}

/** Whether the event is a vertical scroll: more travel up or down than sideways. */
export function isVertical(deltaX: number, deltaY: number): boolean {
  return Math.abs(deltaY) > 0 && !(Math.abs(deltaX) > Math.abs(deltaY));
}

/** Adds wheel travel up and turns it into notches; one per dock page. */
export class WheelNotches {
  private key: string | null = null;
  /**
   * Travel since the last notch over `key`, in CSS px, in one direction
   * (−100 < rest < 100).
   */
  private rest = 0;

  /**
   * The notches `input` completes, oldest first (empty for most of a
   * touchpad's events). Over no item, or for an event that is not a
   * vertical scroll (more sideways than vertical, or with `ctrlKey` set),
   * none: the first also drops the rest, the second leaves it.
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
    if (input.ctrlKey || !isVertical(input.deltaX, input.deltaY)) {
      return [];
    }
    const px = wheelPixels(input.deltaY, input.deltaMode, input.pageHeight);
    if (px === 0) {
      return [];
    }
    // A turn the other way starts again: what was left the old way is not
    // taken off the first notch back.
    if (Math.sign(px) !== Math.sign(this.rest)) {
      this.rest = 0;
    }
    this.rest += px;
    const travel = Math.abs(this.rest);
    let whole = Math.trunc((travel + NOTCH_SLACK_PX) / NOTCH_PX);
    if (whole === 0 && Math.abs(px) >= NOTCH_EVENT_PX) {
      whole = 1;
    }
    if (whole === 0) {
      return [];
    }
    const direction: ScrollDirection = px > 0 ? 'next' : 'previous';
    this.rest = Math.sign(px) * Math.max(0, travel - whole * NOTCH_PX);
    const count = Math.min(whole, MAX_NOTCHES_PER_EVENT);
    return Array.from({ length: count }, () => direction);
  }

  /** Forget the item and the rest (the setting went off, the pointer left). */
  reset(): void {
    this.key = null;
    this.rest = 0;
  }
}
