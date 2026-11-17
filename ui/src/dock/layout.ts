// Layout constants (CSS px). They are also CSS variables on the stage so
// the stylesheet and the sizing maths cannot drift apart. A slot is exactly
// the icon's size across the dock; the running dots hang between it and
// the edge inside the panel's padding, which is why the padding is the
// same all round.

import type { PanelLayout } from '$lib/dock/fit';

export const SLOT_GAP = 8;
export const PANEL_PADDING = 8;
export const DOTS_GAP = 2;
export const DOT_SIZE = 4;
export const SEPARATOR_MARGIN = 6;
/** The constants above as the fit (`fit.ts`) reads them: a separator is 1 px plus its margins. */
export const PANEL_LAYOUT: PanelLayout = {
  slotGap: SLOT_GAP,
  panelPadding: PANEL_PADDING,
  separatorLength: 1 + 2 * SEPARATOR_MARGIN,
};
/** Between the panel and the window's edge side (docs/ipc/sizing-protocol.md). */
export const EDGE_MARGIN = 8;
/** Room for the tooltip and the bounce beyond a magnified icon, on a dock on `bottom` or `top`. */
export const HEADROOM = 48;
/**
 * The same on `left` and `right`, where the title pill sits beside
 * the icon: 10 px to the pill, which wraps at `PILL_MAX_WIDTH`, plus room
 * for its shadow. Kept with tooltips off too: the failure toasts rest in
 * the same room beside the panel (docs/ipc/sizing-protocol.md §Layout on
 * each edge), and with `HEADROOM` instead they would get 63 px at the
 * default icon size.
 */
export const SIDE_HEADROOM = 240;
/** The widest a title pill gets beside the icons of a vertical dock before it wraps (CSS px). */
export const PILL_MAX_WIDTH = 216;
/** Room at each end for the drop shadow. */
export const SIDE_SLACK = 24;
/**
 * How far beyond a fully magnified icon's far side (its top edge on
 * `bottom`) the hit envelope still counts as "inside" (CSS px). Small on
 * purpose: anything farther is a band where nothing is drawn but clicks
 * would still be swallowed.
 */
export const HIT_SLACK = 2;
/**
 * The window's thickness (CSS px; Rust clamps it to the work area) while
 * an icon is being dragged, so the lifted icon can go anywhere on screen.
 */
export const DRAG_WINDOW_THICKNESS = 4096;
/**
 * A release within this many icon sizes of the pinned row, towards or
 * away from the edge, still drops; farther cancels.
 */
export const DROP_SLACK_ICONS = 3;
export const TOOLTIP_DELAY_MS = 300;
