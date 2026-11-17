/** The auto-hide machine's transitions (`../autohide.ts`). */

import {
  autoHideActive,
  autoHideDodges,
  autoHideEnabled,
  MAX_AUTO_HIDE_DELAY_MS,
  REVEALING_HOLDS,
  type AutoHideEvent,
  type AutoHideState,
  type AutoHideStep,
} from './state';

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
 * the module comment of `../autohide.ts`.
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
