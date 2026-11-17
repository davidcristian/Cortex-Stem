/** The overlap rule (mirror of crates/stem-core/src/overlap.rs). */

import type { RawWindow } from '$lib/ipc/generated/RawWindow';
import type { Rect } from '$lib/ipc/generated/Rect';

/** `overlap::intersects`: a shared region of positive area; touching edges do not count. */
export function rectsIntersect(a: Rect, b: Rect): boolean {
  if (a.width <= 0 || a.height <= 0 || b.width <= 0 || b.height <= 0) {
    return false;
  }
  return a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height;
}

/**
 * `overlap::overlapped`: `DockState.overlapped` for a panel resting at
 * `panel` (screen px, `null` while unknown) among `windows`: one of them is
 * visible, not minimised, not cloaked, not ours (`ownProcessId`), has a
 * frame, and that frame intersects the panel with a positive area.
 */
export function overlapped(
  panel: Rect | null,
  windows: Iterable<RawWindow>,
  ownProcessId: number | null,
): boolean {
  if (panel === null || panel.width <= 0 || panel.height <= 0) {
    return false;
  }
  for (const w of windows) {
    if (!w.isVisible || w.isMinimized || w.isCloaked || w.rect === null) {
      continue;
    }
    if (ownProcessId !== null && w.processId === ownProcessId) {
      continue;
    }
    if (rectsIntersect(panel, w.rect)) {
      return true;
    }
  }
  return false;
}
