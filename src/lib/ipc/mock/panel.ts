/**
 * The app layer's dock windows (src-tauri/src/windows.rs): their labels, and
 * where on the screen the panel a page reports rests.
 */

import type { Viewport } from '$lib/dock/geometry';
import type { DockPosition } from '$lib/ipc/generated/DockPosition';
import type { MonitorId } from '$lib/ipc/generated/MonitorId';
import type { Rect } from '$lib/ipc/generated/Rect';

/**
 * The app layer's conversion of the UI's resting panel (CSS px of the dock
 * window's viewport) to screen pixels (`windows::panel_on_screen` with the
 * dock window where `windows::layout` puts it: flush with `edge` of the
 * work area and spanning its length along it, docs/ipc/sizing-protocol.md,
 * v1.16). The viewport's corner on the edge side is the work area's: its
 * bottom-left corner on `bottom`, its top-left corner on `top` and `left`,
 * its top-right corner on `right`. At a `scale` other than 1 the app layer
 * scales the CSS rect (rounded outwards, like a hit region) and the
 * viewport before placing them on the physical work area (`scale_rect`,
 * `panel_on_screen` in src-tauri/src/windows.rs).
 */
export function panelOnScreen(
  rect: Rect,
  workArea: Rect,
  viewport: { width: number; height: number },
  edge: DockPosition,
  scale = 1,
): Rect {
  const s = Number.isFinite(scale) && scale > 0 ? scale : 1;
  const scaled =
    s === 1
      ? { ...rect }
      : {
          x: Math.floor(rect.x * s),
          y: Math.floor(rect.y * s),
          width: Math.ceil((rect.x + rect.width) * s) - Math.floor(rect.x * s),
          height: Math.ceil((rect.y + rect.height) * s) - Math.floor(rect.y * s),
        };
  const width = s === 1 ? viewport.width : Math.round(viewport.width * s);
  const height = s === 1 ? viewport.height : Math.round(viewport.height * s);
  const originX = edge === 'right' ? workArea.x + workArea.width - width : workArea.x;
  const originY = edge === 'bottom' ? workArea.y + workArea.height - height : workArea.y;
  return { ...scaled, x: originX + scaled.x, y: originY + scaled.y };
}

/**
 * `windows::VIEWPORT_SLACK_CSS`: two viewports that differ by less than
 * this on an axis (CSS px) are the same on it (`reanchorPanel`).
 */
export const VIEWPORT_SLACK_CSS = 1;

/**
 * The app layer's re-anchoring of a reported panel rect
 * (`windows::reanchor_panel`; docs/ipc/report-provenance.md): `rect`,
 * measured in the viewport `from` for a dock on `edge`, moved to where the
 * page's layout puts it in the viewport `to`. Across the edge it keeps its
 * distance to the viewport's edge side (on `bottom` it moves by the change
 * in height, on `right` by the change in width), along the edge its offset
 * from the centre (half the change in length, rounded to the nearest px,
 * a half away from zero like Rust's `f64::round`). A difference under
 * `VIEWPORT_SLACK_CSS` on an axis moves nothing on it; the size never
 * changes.
 */
export function reanchorPanel(rect: Rect, edge: DockPosition, from: Viewport, to: Viewport): Rect {
  const change = (before: number, after: number): number => {
    const d = after - before;
    return Number.isFinite(d) && Math.abs(d) >= VIEWPORT_SLACK_CSS ? d : 0;
  };
  const dw = change(from.width, to.width);
  const dh = change(from.height, to.height);
  const shifts: Record<DockPosition, readonly [number, number]> = {
    bottom: [dw / 2, dh],
    top: [dw / 2, 0],
    left: [0, dh / 2],
    right: [dw, dh / 2],
  };
  const [dx, dy] = shifts[edge];
  // `Math.round` rounds a half up; Rust's `round` away from zero (`+ 0`: no -0).
  const round = (v: number): number => (v < 0 ? -Math.round(-v) : Math.round(v)) + 0;
  return { ...rect, x: round(rect.x + dx), y: round(rect.y + dy) };
}

/** `CssViewport::validate`: both sides positive and finite, else the Rust error. */
export function viewportError(viewport: Viewport): string | null {
  const usable = (v: number): boolean => Number.isFinite(v) && v > 0;
  return usable(viewport.width) && usable(viewport.height)
    ? null
    : `the viewport must have a positive, finite width and height: ${JSON.stringify(viewport)}`;
}

/**
 * The label of the dock window on `monitor` (`dock_label` in
 * src-tauri/src/windows.rs): `dock` for the primary monitor's, else `dock-`
 * and the id with every character outside `[A-Za-z0-9_-]` dropped.
 */
export function dockLabel(monitor: MonitorId | null, primary: MonitorId | null): string {
  if (monitor === null || monitor === primary) {
    return 'dock';
  }
  return `dock-${monitor.replace(/[^A-Za-z0-9_-]/g, '')}`;
}
