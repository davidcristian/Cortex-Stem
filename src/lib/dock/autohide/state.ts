/** The auto-hide machine's state, its events and the facts read from a state (`../autohide.ts`). */

import type { AutoHideMode } from '$lib/ipc/generated/AutoHideMode';

/** The slide (or, under reduced motion, the fade) in each direction, ms (ADR-0008). */
export const AUTO_HIDE_SLIDE_MS = 200;
/** `autoHide.delayMs` is clamped to this by Rust; the machine clamps again defensively. */
export const MAX_AUTO_HIDE_DELAY_MS = 5000;

/**
 * - `shown`: on screen, nothing counting down.
 * - `pending`: on screen, the pointer is away and the delay timer runs.
 * - `hidden`: slid out (or sliding out); only the reveal strip takes the pointer.
 */
export type AutoHidePhase = 'shown' | 'pending' | 'hidden';

/**
 * What keeps the dock on screen whatever the pointer does. `fileDrag`: a
 * file from the shell is being dragged over the dock window, an OLE
 * drag that the page sees through Tauri's events, never as pointer events.
 * `attention`: at least one item has `needsAttention`; the only hold
 * that also reveals a hidden dock when it begins (`REVEALING_HOLDS`).
 * `focus`: an item has keyboard focus: the dock must not slide away
 * from under it. Like `menu` it never reveals (a hidden panel cannot take
 * focus, so it can only begin while the dock is shown).
 */
export type AutoHideHold = 'drag' | 'menu' | 'bounce' | 'fileDrag' | 'attention' | 'focus';

/**
 * Holds whose start brings a hidden dock back. The others only keep a shown
 * dock up: a bounce for a launch from elsewhere, say, should not pop the dock
 * up, but an app asking for the user's attention should be seen.
 */
export const REVEALING_HOLDS: ReadonlySet<AutoHideHold> = new Set<AutoHideHold>(['attention']);

export interface AutoHideState {
  /** `autoHide.mode` is `always`. */
  readonly enabled: boolean;
  /** `autoHide.mode` is `whenOverlapped`: the dock hides while `overlapped`. */
  readonly dodges: boolean;
  /** `autoHide.delayMs`, clamped to 0..=5000 and whole. */
  readonly delayMs: number;
  /** `DockState.fullscreen`: a fullscreen app owns the screen; the dock hides with no delay. */
  readonly fullscreen: boolean;
  /** `DockState.overlapped`: a window covers the resting panel. Only `dodges` acts on it. */
  readonly overlapped: boolean;
  readonly phase: AutoHidePhase;
  /**
   * The last pointer event put the pointer over the panel or the reveal
   * strip. Cleared when auto-hide stops applying (see the module comment).
   */
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
  /** `DockState.fullscreen` changed. Idempotent. */
  | { readonly type: 'fullscreen'; readonly active: boolean }
  /** `DockState.overlapped` changed. Idempotent. */
  | { readonly type: 'overlapped'; readonly active: boolean }
  /** The timer armed on entering `pending` fired. */
  | { readonly type: 'timeout' };

/**
 * What to do with the one delay timer: `arm` (clearing any running one
 * first) for `hideDelayMs(state)`, `clear` it, or leave it alone (`none`).
 */
export type TimerCommand = 'none' | 'arm' | 'clear';

export interface AutoHideStep {
  readonly state: AutoHideState;
  readonly timer: TimerCommand;
}

/** Auto-hide off, the dock on screen, the pointer assumed away until an event says otherwise. */
export const INITIAL_AUTO_HIDE: AutoHideState = {
  enabled: false,
  dodges: false,
  delayMs: 0,
  fullscreen: false,
  overlapped: false,
  phase: 'shown',
  pointerInside: false,
  holds: {
    drag: false,
    menu: false,
    bounce: false,
    fileDrag: false,
    attention: false,
    focus: false,
  },
};

/** The mode hides whenever the pointer is away. */
export function autoHideEnabled(mode: AutoHideMode): boolean {
  return mode === 'always';
}

/** The mode hides only while a window overlaps the dock. */
export function autoHideDodges(mode: AutoHideMode): boolean {
  return mode === 'whenOverlapped';
}

/**
 * The dock hides in `state`: the mode is `always`, or a fullscreen app owns
 * the screen, or the mode is `whenOverlapped` and a window covers
 * the dock. What decides whether the reveal strip is in the DOM and
 * the hit region follows the phase.
 */
export function autoHideActive(state: AutoHideState): boolean {
  return state.enabled || state.fullscreen || (state.dodges && state.overlapped);
}

/**
 * Only a fullscreen app makes the machine active: the mode is not
 * `always`. An overlap under `whenOverlapped` does not count while
 * fullscreen: the fullscreen window covers the whole monitor, so it is the
 * overlap, and the dock's monitor has nothing else in front.
 */
export function onlyFullscreenHides(state: AutoHideState): boolean {
  return state.fullscreen && !state.enabled;
}

/** How long the timer an `arm` asks for runs: no delay at all while fullscreen. */
export function hideDelayMs(state: AutoHideState): number {
  return state.fullscreen ? 0 : state.delayMs;
}

/** Whether the dock is out of view (or on its way out) in `state`. */
export function isAutoHidden(state: AutoHideState): boolean {
  return state.phase === 'hidden';
}
