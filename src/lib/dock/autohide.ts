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
 *     └──────────strip touched, or an item needs attention──────┘
 * ```
 *
 * `autoHide.mode = always` hides whenever the pointer is away; `off` never
 * does; `whenOverlapped` hides only while `DockState.overlapped`
 * says a window covers the resting panel (see below). Switching to a mode
 * that does not hide right now reveals at once. A hold (an icon lifted for
 * a drag, a native context menu open, an item bouncing for a launch the dock
 * started, a file from the shell being dragged over the dock, an item with
 * keyboard focus) keeps the dock up while it lasts but never reveals a
 * hidden one.
 * Once hidden, the reveal strip brings the dock back, and so does the dock
 * window being shown on request (ADR-0008: the tray and the toggle shortcut
 * still show it).
 * The one hold that also reveals is `attention`: an item whose app
 * flashed its taskbar button brings a hidden dock back and keeps it up until
 * no item needs attention any more (the window was activated), like the
 * stock taskbar on auto-hide.
 *
 * Fullscreen: while `DockState.fullscreen` says a fullscreen app
 * owns the screen (a game, a browser after F11, a video, a slideshow), the
 * machine behaves as if the mode were `always` with no delay, whatever the
 * mode is: the `fullscreen` event hides at once with the pointer away; with
 * the pointer over the dock it stays until the pointer leaves, and then goes
 * through `pending` with a 0 ms timer (so a strip-leave and a panel-enter in
 * one dispatch cannot hide it). The strip, `show` and the holds work as
 * usual. When fullscreen ends, a mode other than `always` shows the dock
 * again at once; `always` takes over through its normal delay, and a dock
 * that is hidden stays hidden. `enabled` keeps meaning "the mode is
 * `always`"; `autoHideActive` is what the UI reads for the strip and the
 * hit region.
 *
 * Fullscreen above a visible stock taskbar: the dock window sits on
 * its edge of the work area (ADR-0008; `Settings.position`), which
 * with the stock taskbar (or another app bar) on that edge is about 48 px
 * inside the screen's, so a strip there would be a line across the
 * fullscreen picture that flashes the dock over it whenever the pointer
 * crosses it. While only fullscreen makes the machine active
 * (`onlyFullscreenHides`) and the work area stops short of the monitor on
 * the dock's edge (`workAreaReachesEdge`), there is no strip
 * (`revealStripOn`) and the hidden dock takes no pointer at all
 * (`autoHideHitRegion`): the tray, the toggle shortcut (`show`) and
 * `attention` still bring it back. With the taskbar auto-hidden (Windows'
 * own setting, or Stem's) or on another edge, the work area reaches
 * the dock's edge and the strip works as above.
 *
 * Every edge: the panel slides out towards the dock's edge, the
 * reveal strip lies along the window's edge side (the bottom 2 px on
 * `bottom`, the leftmost 2 px on `left`, and so on), and the band between
 * the panel and that side is the strip's while the dock is on screen. The
 * machine itself knows nothing about edges; only the rectangles below do.
 *
 * When overlapped: with the mode `whenOverlapped` (`dodges`), the
 * `overlapped` event carries `DockState.overlapped`, the engine's verdict
 * that a window of another process covers the resting panel. While it is
 * set the machine is active exactly as with `always`: the dock hides after
 * `delayMs` once the pointer is away (never at once, so a window dragged
 * past does not make it jump), the strip, `show` and the holds work as
 * usual. When it clears the dock comes back at once (a countdown is
 * dropped), unless `always` or fullscreen keeps the machine active.
 * `fullscreen` and `overlapped` are independent facts; `autoHideActive` is
 * "the mode is `always`, or fullscreen, or the mode is `whenOverlapped` and
 * a window overlaps".
 *
 * A countdown of 0 ms also goes through `pending` and a 0 ms timer, so two
 * events of one dispatch (a strip-leave followed by a panel-enter) can
 * never hide the dock between them, whatever the mode. For that to hold
 * when the pointer comes in from the strip slowly, the strip on screen fills
 * the whole margin between the panel and the window's edge side
 * (`revealStripThickness`): there is no gap between the two where the
 * pointer would count as away.
 *
 * The machine forgets the pointer (`pointerInside` back to `false`) when
 * auto-hide stops applying (the mode, the end of fullscreen or of an
 * overlap): `strip` and `show` set it without the pointer being over the
 * panel, and the strip can go from under the pointer with no `pointerleave`
 * (none comes from a removed element), so a stale `true` would leave the
 * next fullscreen or overlap unable to hide the dock. `Dock.svelte` tells
 * it where the pointer is again: the next move over the panel does, and so
 * does the panel's `:hover` when auto-hide comes on again.
 */

import type { AutoHideMode } from '$lib/ipc/generated/AutoHideMode';
import type { MonitorInfo } from '$lib/ipc/generated/MonitorInfo';
import type { Rect } from '$lib/ipc/generated/Rect';

import type { DockEdge } from './edge';
import type { HitRegion, Viewport } from './geometry';

/** The slide (or, under reduced motion, the fade) in each direction, ms (ADR-0008). */
export const AUTO_HIDE_SLIDE_MS = 200;
/** Thickness of the reveal strip along the dock window's edge side, CSS px (ADR-0008). */
export const REVEAL_STRIP_PX = 2;
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

/**
 * The dock window's edge side is the screen's: the window
 * sits on `edge` of `monitor`'s work area (ADR-0008), which reaches the
 * monitor's own edge there unless a visible stock taskbar (or another app
 * bar) holds the space between (docs/ipc/hit-region.md: on `bottom`
 * `workArea.y + workArea.height >= bounds.y + bounds.height`, on `top`
 * `workArea.y <= bounds.y`, on `left` `workArea.x <= bounds.x`, on `right`
 * `workArea.x + workArea.width >= bounds.x + bounds.width`). True with the
 * taskbar auto-hidden or on another edge, and for a monitor not known yet
 * (the strip as it always was).
 */
export function workAreaReachesEdge(
  monitor: MonitorInfo | null | undefined,
  edge: DockEdge,
): boolean {
  if (monitor === null || monitor === undefined) {
    return true;
  }
  const { bounds, workArea } = monitor;
  switch (edge) {
    case 'bottom':
      return workArea.y + workArea.height >= bounds.y + bounds.height;
    case 'top':
      return workArea.y <= bounds.y;
    case 'left':
      return workArea.x <= bounds.x;
    case 'right':
      return workArea.x + workArea.width >= bounds.x + bounds.width;
  }
}

/**
 * The reveal strip is in place in `state`: auto-hide applies, and either
 * the strip is at the screen's edge (`atScreenEdge`, `workAreaReachesEdge`)
 * or something other than fullscreen hides the dock (the mode `always`, or
 * an overlap with `whenOverlapped`), whose strip sits on the work area's
 * edge as it always has (ADR-0008 "Consequences").
 */
export function revealStripOn(state: AutoHideState, atScreenEdge: boolean): boolean {
  return autoHideActive(state) && (atScreenEdge || !onlyFullscreenHides(state));
}

/**
 * (`x`, `y`) is on the dock where no pointer event says so (a drag or a
 * file drag released): over `panel` (the panel as drawn, CSS px of
 * the viewport), or, with the reveal strip in the DOM (`stripOn`,
 * `revealStripOn`), in the band between it and the window's edge side, as
 * long as the panel (below it on `bottom`, left of it on `left`, and so
 * on), out to that side of `viewport`: the strip fills that band while the
 * dock is on screen (`revealStripThickness`), so its `pointerleave` reports
 * the pointer going. Without a strip (fullscreen above a visible taskbar,
 * or auto-hide off) nothing there would, and a pointer counted inside would
 * stay so until it had crossed the panel.
 */
export function onDockAt(
  panel: Rect,
  x: number,
  y: number,
  viewport: Viewport,
  stripOn: boolean,
  edge: DockEdge,
): boolean {
  let left = panel.x;
  let top = panel.y;
  let right = panel.x + panel.width;
  let bottom = panel.y + panel.height;
  if (stripOn) {
    switch (edge) {
      case 'bottom':
        bottom = viewport.height;
        break;
      case 'top':
        top = 0;
        break;
      case 'left':
        left = 0;
        break;
      case 'right':
        right = viewport.width;
        break;
    }
  }
  return x >= left && x < right && y >= top && y < bottom;
}

/**
 * The reveal strip's thickness (across the edge) in `phase`, CSS px:
 * `REVEAL_STRIP_PX` while hidden (the only place the window takes the
 * pointer); on screen, the whole band from the window's edge side to the
 * panel (`gap`, the margin between them), so the pointer coming in from
 * the strip is over the panel the moment it leaves the strip. With a 0 ms
 * countdown (fullscreen, or a delay of 0) a gap between the two
 * would hide the dock on the way in. Never less than the strip.
 */
export function revealStripThickness(phase: AutoHidePhase, gap: number): number {
  return phase === 'hidden' ? REVEAL_STRIP_PX : Math.max(REVEAL_STRIP_PX, gap);
}

/** How long the timer an `arm` asks for runs: no delay at all while fullscreen. */
export function hideDelayMs(state: AutoHideState): number {
  return state.fullscreen ? 0 : state.delayMs;
}

function clampDelay(ms: number): number {
  if (!Number.isFinite(ms)) {
    return 0;
  }
  return Math.min(MAX_AUTO_HIDE_DELAY_MS, Math.max(0, Math.round(ms)));
}

function held(state: AutoHideState): boolean {
  const { holds } = state;
  return (
    holds.drag || holds.menu || holds.bounce || holds.fileDrag || holds.attention || holds.focus
  );
}

function wantsHide(state: AutoHideState): boolean {
  return autoHideActive(state) && !state.pointerInside && !held(state);
}

/**
 * The phase `next` (fields already updated, phase still the previous one)
 * should be in. `hidden` is sticky while auto-hide is active: only `strip`,
 * `show` and a revealing hold leave it, by setting the phase to `shown`
 * before calling this. `restart` re-arms a running countdown (the delay
 * changed). A countdown always goes through `pending` and a timer, a 0 ms
 * one included: the pointer leaving the strip for the panel must not hide
 * the dock between two events of one dispatch (only the `fullscreen` event
 * itself hides at once, see `autoHideStep`).
 */
function settle(next: AutoHideState, restart = false): AutoHideStep {
  const wasPending = next.phase === 'pending';
  if (!autoHideActive(next)) {
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
  return { state: { ...next, phase: 'pending' }, timer: 'arm' };
}

/**
 * One transition. Never mutates `state`. When it ends auto-hide (the mode,
 * the end of fullscreen or of an overlap), the pointer is forgotten: see
 * the module comment.
 */
export function autoHideStep(state: AutoHideState, event: AutoHideEvent): AutoHideStep {
  const next = transition(state, event);
  if (next.state.pointerInside && autoHideActive(state) && !autoHideActive(next.state)) {
    return { state: { ...next.state, pointerInside: false }, timer: next.timer };
  }
  return next;
}

function transition(state: AutoHideState, event: AutoHideEvent): AutoHideStep {
  switch (event.type) {
    case 'configure': {
      const enabled = autoHideEnabled(event.mode);
      const dodges = autoHideDodges(event.mode);
      const delayMs = clampDelay(event.delayMs);
      const restart =
        enabled !== state.enabled || dodges !== state.dodges || delayMs !== state.delayMs;
      return settle({ ...state, enabled, dodges, delayMs }, restart);
    }
    case 'pointer':
      return settle({ ...state, pointerInside: event.inside });
    case 'strip':
      if (!autoHideActive(state)) {
        return { state, timer: 'none' };
      }
      return settle({
        ...state,
        pointerInside: true,
        phase: state.phase === 'hidden' ? 'shown' : state.phase,
      });
    case 'show':
      if (!autoHideActive(state) || state.phase !== 'hidden') {
        return { state, timer: 'none' };
      }
      return settle({ ...state, pointerInside: true, phase: 'shown' });
    case 'hold': {
      const holds = { ...state.holds, [event.hold]: event.active };
      // A revealing hold brings a hidden dock back; the pointer stays where
      // it was, so the countdown starts once the hold ends with it away.
      const reveal =
        event.active &&
        autoHideActive(state) &&
        state.phase === 'hidden' &&
        REVEALING_HOLDS.has(event.hold);
      return settle({ ...state, holds, phase: reveal ? 'shown' : state.phase });
    }
    case 'fullscreen': {
      if (event.active === state.fullscreen) {
        return { state, timer: 'none' };
      }
      const next = { ...state, fullscreen: event.active };
      if (event.active && next.phase !== 'hidden' && wantsHide(next)) {
        // A fullscreen app took the screen with the pointer away and nothing
        // held: out at once, no countdown (a running one is dropped).
        return {
          state: { ...next, phase: 'hidden' },
          timer: state.phase === 'pending' ? 'clear' : 'none',
        };
      }
      // Begun with the pointer over the dock (or a hold on): it stays until
      // the pointer leaves. Over: the mode decides again, a running 0 ms
      // countdown restarted with the real delay; a hidden dock stays hidden
      // while `always` or an overlap keeps the machine active, and comes
      // back otherwise.
      return settle(next, !event.active);
    }
    case 'overlapped': {
      if (event.active === state.overlapped) {
        return { state, timer: 'none' };
      }
      // A window came over the dock: with the mode `whenOverlapped` the
      // normal countdown starts (never an instant hide: a window dragged
      // past must not make the dock jump), unless the pointer is over it or
      // a hold is on. It left: the dock comes back at once, a running
      // countdown dropped, unless `always` or fullscreen still applies. With
      // any other mode the fact is kept and changes nothing.
      return settle({ ...state, overlapped: event.active });
    }
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
 * The reveal strip (ADR-0008): the `REVEAL_STRIP_PX` CSS px of `viewport`
 * along its edge side (`edge`: the bottom 2 px on `bottom`, the top 2 px on
 * `top`, the leftmost on `left`, the rightmost on `right`), exactly as long
 * as the resting panel `panel` (whole CSS px, as `integerRect` gives it)
 * and in line with it.
 */
export function revealStrip(panel: Rect, viewport: Viewport, edge: DockEdge): Rect {
  switch (edge) {
    case 'bottom': {
      const height = Math.max(0, Math.min(REVEAL_STRIP_PX, viewport.height));
      return { x: panel.x, y: viewport.height - height, width: panel.width, height };
    }
    case 'top': {
      const height = Math.max(0, Math.min(REVEAL_STRIP_PX, viewport.height));
      return { x: panel.x, y: 0, width: panel.width, height };
    }
    case 'left': {
      const width = Math.max(0, Math.min(REVEAL_STRIP_PX, viewport.width));
      return { x: 0, y: panel.y, width, height: panel.height };
    }
    case 'right': {
      const width = Math.max(0, Math.min(REVEAL_STRIP_PX, viewport.width));
      return { x: viewport.width - width, y: panel.y, width, height: panel.height };
    }
  }
}

/** While hidden the strip is both rectangles: the window takes the pointer only there. */
export function hiddenHitRegion(panel: Rect, viewport: Viewport, edge: DockEdge): HitRegion {
  const strip = revealStrip(panel, viewport, edge);
  return { panel: strip, envelope: { ...strip } };
}

/**
 * No clickable area at all: 0 × 0 at the viewport's origin, which contains
 * no point, as both rectangles (docs/ipc/hit-region.md takes any
 * non-negative size; the envelope contains the panel).
 */
export const NO_HIT_REGION: Readonly<HitRegion> = Object.freeze({
  panel: Object.freeze({ x: 0, y: 0, width: 0, height: 0 }),
  envelope: Object.freeze({ x: 0, y: 0, width: 0, height: 0 }),
});

/**
 * What the dock on `edge` reports for `region` (the panel at rest and its
 * envelope) in `state`: `region` itself while auto-hide does not
 * apply; with the reveal strip (`revealStripOn`), the strip while hidden
 * (`hiddenHitRegion`) and the envelope grown out to the window's edge side
 * while shown (`shownHitRegion`); with none (fullscreen above a visible
 * taskbar), `NO_HIT_REGION` while hidden, so the window is
 * click-through everywhere, and `region` while shown.
 */
export function autoHideHitRegion(
  state: AutoHideState,
  region: HitRegion,
  viewport: Viewport,
  atScreenEdge: boolean,
  edge: DockEdge,
): HitRegion {
  if (!autoHideActive(state)) {
    return region;
  }
  const hidden = isAutoHidden(state);
  if (!revealStripOn(state, atScreenEdge)) {
    return hidden
      ? { panel: { ...NO_HIT_REGION.panel }, envelope: { ...NO_HIT_REGION.envelope } }
      : region;
  }
  return hidden
    ? hiddenHitRegion(region.panel, viewport, edge)
    : shownHitRegion(region, viewport, edge);
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
 * envelope grown to take in the reveal strip between the panel and the
 * window's edge side. A pointer that revealed the dock from the strip, or
 * went from the panel to the screen edge, so stays tracked (the window
 * keeps taking it) instead of the window turning click-through under it;
 * the panel, where the window starts taking the pointer, is unchanged.
 */
export function shownHitRegion(region: HitRegion, viewport: Viewport, edge: DockEdge): HitRegion {
  return {
    panel: region.panel,
    envelope: union(region.envelope, revealStrip(region.panel, viewport, edge)),
  };
}
