/**
 * Auto-hide (ADR-0008): when the dock slides out of view and when it
 * comes back. Pure presentation logic, like the magnification maths:
 * `Dock.svelte` reports what happened (the pointer is over the panel or the
 * reveal strip or not, a hold such as a drag began or ended, the delay timer
 * fired, the settings changed) and applies what comes back: the phase to
 * render, and whether to arm or clear its one timer.
 *
 * ```text
 *   shown ──pointer out, nothing held──▶ pending ──delayMs──▶ hidden
 *     ▲                                    │                    │
 *     └─────pointer back, or a hold────────┘                    │
 *     └────────────────────strip touched────────────────────────┘
 * ```
 *
 * Only `autoHide.mode = always` hides. `off` never does, and neither does
 * `whenOverlapped` until the platform can report window geometry (ADR-0008);
 * switching away from `always` reveals at once. A hold (an icon lifted for
 * a drag, a native context menu open, an item bouncing for a launch the dock
 * started) keeps the dock up while it lasts but never reveals a hidden one.
 * Once hidden, the reveal strip brings the dock back, and so does the dock
 * window being shown on request (ADR-0008: the tray and the toggle shortcut
 * still show it).
 */

import type { AutoHideMode } from '$lib/ipc/generated/AutoHideMode';
import type { Rect } from '$lib/ipc/generated/Rect';

import type { HitRegion } from './geometry';

/** The slide (or, under reduced motion, the fade) in each direction, ms (ADR-0008). */
export const AUTO_HIDE_SLIDE_MS = 200;
/** Height of the reveal strip along the dock window's bottom edge, CSS px (ADR-0008). */
export const REVEAL_STRIP_PX = 2;
/** `autoHide.delayMs` is clamped to this by Rust; the machine clamps again defensively. */
export const MAX_AUTO_HIDE_DELAY_MS = 5000;

/**
 * - `shown`: on screen, nothing counting down.
 * - `pending`: on screen, the pointer is away and the delay timer runs.
 * - `hidden`: slid out (or sliding out); only the reveal strip takes the pointer.
 */
export type AutoHidePhase = 'shown' | 'pending' | 'hidden';

/** What keeps the dock on screen whatever the pointer does. */
export type AutoHideHold = 'drag' | 'menu' | 'bounce';

export interface AutoHideState {
  /** `autoHide.mode` is `always`. */
  readonly enabled: boolean;
  /** `autoHide.delayMs`, clamped to 0..=5000 and whole. */
  readonly delayMs: number;
  readonly phase: AutoHidePhase;
  /** The last pointer event put the pointer over the panel or the reveal strip. */
  readonly pointerInside: boolean;
  readonly holds: Readonly<Record<AutoHideHold, boolean>>;
}

export type AutoHideEvent =
  /** The settings arrived or changed. */
  | { readonly type: 'configure'; readonly mode: AutoHideMode; readonly delayMs: number }
  /** The pointer entered (`true`) or left (`false`) the panel or the reveal strip. */
  | { readonly type: 'pointer'; readonly inside: boolean }
  /** The pointer reached the reveal strip: reveal, and the pointer counts as inside. */
  | { readonly type: 'strip' }
  /**
   * The dock window was shown on request (the tray or the toggle shortcut
   * hid and showed it). A hidden dock comes back and stays until the
   * pointer has been over it and left, as if the strip had been touched.
   */
  | { readonly type: 'show' }
  /** A hold began (`active`) or ended. Idempotent. */
  | { readonly type: 'hold'; readonly hold: AutoHideHold; readonly active: boolean }
  /** The timer armed on entering `pending` fired. */
  | { readonly type: 'timeout' };

/**
 * What to do with the one delay timer: `arm` (clearing any running one
 * first) for `state.delayMs`, `clear` it, or leave it alone (`none`).
 */
export type TimerCommand = 'none' | 'arm' | 'clear';

export interface AutoHideStep {
  readonly state: AutoHideState;
  readonly timer: TimerCommand;
}

/** Auto-hide off, the dock on screen, the pointer assumed away until an event says otherwise. */
export const INITIAL_AUTO_HIDE: AutoHideState = {
  enabled: false,
  delayMs: 0,
  phase: 'shown',
  pointerInside: false,
  holds: { drag: false, menu: false, bounce: false },
};

/** Auto-hide applies to this mode now (ADR-0008: `whenOverlapped` waits for window geometry). */
export function autoHideEnabled(mode: AutoHideMode): boolean {
  return mode === 'always';
}

function clampDelay(ms: number): number {
  if (!Number.isFinite(ms)) {
    return 0;
  }
  return Math.min(MAX_AUTO_HIDE_DELAY_MS, Math.max(0, Math.round(ms)));
}

function held(state: AutoHideState): boolean {
  return state.holds.drag || state.holds.menu || state.holds.bounce;
}

function wantsHide(state: AutoHideState): boolean {
  return state.enabled && !state.pointerInside && !held(state);
}

/**
 * The phase `next` (fields already updated, phase still the previous one)
 * should be in. `hidden` is sticky while auto-hide is on: only `strip`
 * leaves it, by setting the phase to `shown` before calling this. `restart`
 * re-arms a running countdown (the delay changed).
 */
function settle(next: AutoHideState, restart = false): AutoHideStep {
  const wasPending = next.phase === 'pending';
  if (!next.enabled) {
    return { state: { ...next, phase: 'shown' }, timer: wasPending ? 'clear' : 'none' };
  }
  if (next.phase === 'hidden') {
    return { state: next, timer: 'none' };
  }
  if (!wantsHide(next)) {
    return { state: { ...next, phase: 'shown' }, timer: wasPending ? 'clear' : 'none' };
  }
  if (wasPending && !restart) {
    return { state: next, timer: 'none' };
  }
  if (next.delayMs === 0) {
    return { state: { ...next, phase: 'hidden' }, timer: wasPending ? 'clear' : 'none' };
  }
  return { state: { ...next, phase: 'pending' }, timer: 'arm' };
}

/** One transition. Never mutates `state`. */
export function autoHideStep(state: AutoHideState, event: AutoHideEvent): AutoHideStep {
  switch (event.type) {
    case 'configure': {
      const enabled = autoHideEnabled(event.mode);
      const delayMs = clampDelay(event.delayMs);
      const restart = enabled !== state.enabled || delayMs !== state.delayMs;
      return settle({ ...state, enabled, delayMs }, restart);
    }
    case 'pointer':
      return settle({ ...state, pointerInside: event.inside });
    case 'strip':
      if (!state.enabled) {
        return { state, timer: 'none' };
      }
      return settle({
        ...state,
        pointerInside: true,
        phase: state.phase === 'hidden' ? 'shown' : state.phase,
      });
    case 'show':
      if (!state.enabled || state.phase !== 'hidden') {
        return { state, timer: 'none' };
      }
      return settle({ ...state, pointerInside: true, phase: 'shown' });
    case 'hold':
      return settle({ ...state, holds: { ...state.holds, [event.hold]: event.active } });
    case 'timeout':
      if (state.phase !== 'pending') {
        // A timer that fired after it stopped mattering.
        return { state, timer: 'none' };
      }
      if (!wantsHide(state)) {
        return { state: { ...state, phase: 'shown' }, timer: 'none' };
      }
      return { state: { ...state, phase: 'hidden' }, timer: 'none' };
  }
}

/** Whether the dock is out of view (or on its way out) in `state`. */
export function isAutoHidden(state: AutoHideState): boolean {
  return state.phase === 'hidden';
}

/**
 * The reveal strip (ADR-0008): the bottom `REVEAL_STRIP_PX` CSS px of a
 * viewport `viewportHeight` px tall, exactly as wide as the resting panel
 * `panel` (whole CSS px, as `integerRect` gives it) and in line with it.
 */
export function revealStrip(panel: Rect, viewportHeight: number): Rect {
  const height = Math.max(0, Math.min(REVEAL_STRIP_PX, viewportHeight));
  return { x: panel.x, y: viewportHeight - height, width: panel.width, height };
}

/** While hidden the strip is both rectangles: the window takes the pointer only there. */
export function hiddenHitRegion(panel: Rect, viewportHeight: number): HitRegion {
  const strip = revealStrip(panel, viewportHeight);
  return { panel: strip, envelope: { ...strip } };
}

/** The smallest rect containing both. */
function union(a: Rect, b: Rect): Rect {
  const x = Math.min(a.x, b.x);
  const y = Math.min(a.y, b.y);
  return {
    x,
    y,
    width: Math.max(a.x + a.width, b.x + b.width) - x,
    height: Math.max(a.y + a.height, b.y + b.height) - y,
  };
}

/**
 * The region while auto-hide is on and the dock is shown: `region` with the
 * envelope grown to take in the reveal strip below the panel. A pointer that
 * revealed the dock from the strip, or came down from the panel to the
 * screen edge, so stays tracked (the window keeps taking it) instead of the
 * window turning click-through under it; the panel, where the window starts
 * taking the pointer, is unchanged.
 */
export function shownHitRegion(region: HitRegion, viewportHeight: number): HitRegion {
  return {
    panel: region.panel,
    envelope: union(region.envelope, revealStrip(region.panel, viewportHeight)),
  };
}
