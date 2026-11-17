import { describe, expect, it } from 'vitest';

import type { MonitorInfo } from '$lib/ipc/generated/MonitorInfo';

import {
  AUTO_HIDE_SLIDE_MS,
  autoHideActive,
  autoHideHitRegion,
  autoHideStep,
  hiddenHitRegion,
  hideDelayMs,
  INITIAL_AUTO_HIDE,
  isAutoHidden,
  NO_HIT_REGION,
  onDockAt,
  onlyFullscreenHides,
  REVEAL_STRIP_PX,
  revealStrip,
  revealStripOn,
  revealStripThickness,
  REVEALING_HOLDS,
  shownHitRegion,
  workAreaReachesEdge,
  type AutoHideEvent,
  type AutoHideState,
  type TimerCommand,
} from './autohide';
import { DOCK_EDGES } from './edge';
import { hitRegionFor, rectContains, rectContainsPoint, type HitRegion } from './geometry';

/**
 * Drives the machine like `auto-hide.svelte.ts` does: one timer, armed and cleared
 * as the steps say, firing when the clock passes its deadline.
 */
class Harness {
  state: AutoHideState = INITIAL_AUTO_HIDE;
  now = 0;
  deadline: number | null = null;
  readonly timers: TimerCommand[] = [];

  send(event: AutoHideEvent): this {
    const step = autoHideStep(this.state, event);
    this.state = step.state;
    this.timers.push(step.timer);
    if (step.timer === 'clear') {
      this.deadline = null;
    } else if (step.timer === 'arm') {
      this.deadline = this.now + hideDelayMs(step.state);
    }
    return this;
  }

  /** Let `ms` pass; the timer fires on the way if its deadline is reached. */
  advance(ms: number): this {
    const until = this.now + ms;
    if (this.deadline !== null && this.deadline <= until) {
      this.now = this.deadline;
      this.deadline = null;
      this.send({ type: 'timeout' });
    }
    this.now = until;
    return this;
  }

  get phase(): AutoHideState['phase'] {
    return this.state.phase;
  }
}

function always(delayMs = 250): Harness {
  return new Harness().send({ type: 'configure', mode: 'always', delayMs });
}

describe('autoHideStep', () => {
  it('starts on screen with auto-hide off', () => {
    expect(INITIAL_AUTO_HIDE.phase).toBe('shown');
    expect(INITIAL_AUTO_HIDE.enabled).toBe(false);
    expect(isAutoHidden(INITIAL_AUTO_HIDE)).toBe(false);
  });

  it('never hides with auto-hide off, nor when overlapped while no window overlaps', () => {
    // (`whenOverlapped` hides only on the `overlapped` event, see the
    // `when overlapped` block below.)
    for (const mode of ['off', 'whenOverlapped'] as const) {
      const h = new Harness()
        .send({ type: 'configure', mode, delayMs: 250 })
        .send({ type: 'pointer', inside: true })
        .send({ type: 'pointer', inside: false })
        .advance(10_000);
      expect(h.phase).toBe('shown');
      expect(h.state.enabled).toBe(false);
      expect(h.state.dodges).toBe(mode === 'whenOverlapped');
      expect(autoHideActive(h.state)).toBe(false);
      expect(h.timers.every((t) => t === 'none')).toBe(true);
    }
  });

  it('hides delayMs after the pointer left', () => {
    const h = always(250).send({ type: 'pointer', inside: true });
    expect(h.phase).toBe('shown');
    h.send({ type: 'pointer', inside: false });
    expect(h.phase).toBe('pending');
    expect(h.timers.at(-1)).toBe('arm');
    h.advance(249);
    expect(h.phase).toBe('pending');
    h.advance(1);
    expect(h.phase).toBe('hidden');
    expect(isAutoHidden(h.state)).toBe(true);
  });

  it('starts counting down at once when enabled while the pointer is away', () => {
    const h = always(400);
    expect(h.phase).toBe('pending');
    expect(h.timers).toEqual(['arm']);
    h.advance(400);
    expect(h.phase).toBe('hidden');
  });

  it('does not hide when the pointer comes back before the delay', () => {
    const h = always(250)
      .send({ type: 'pointer', inside: true })
      .send({ type: 'pointer', inside: false })
      .advance(200)
      .send({ type: 'pointer', inside: true });
    expect(h.phase).toBe('shown');
    expect(h.timers.at(-1)).toBe('clear');
    h.advance(10_000);
    expect(h.phase).toBe('shown');
    // Leaving again starts a fresh countdown.
    h.send({ type: 'pointer', inside: false }).advance(249);
    expect(h.phase).toBe('pending');
    h.advance(1);
    expect(h.phase).toBe('hidden');
  });

  it('ignores a timer that fires after it stopped mattering', () => {
    const shown = always(250).send({ type: 'pointer', inside: true }).state;
    expect(autoHideStep(shown, { type: 'timeout' })).toEqual({ state: shown, timer: 'none' });
    const hidden = always(250).advance(250).state;
    expect(autoHideStep(hidden, { type: 'timeout' })).toEqual({ state: hidden, timer: 'none' });
  });

  it('reveals when the pointer reaches the strip, and only then', () => {
    const h = always(250).advance(250);
    expect(h.phase).toBe('hidden');
    // Pointer events elsewhere (nothing else takes the pointer while hidden) change nothing.
    h.send({ type: 'pointer', inside: true }).send({ type: 'pointer', inside: false });
    expect(h.phase).toBe('hidden');
    h.send({ type: 'strip' });
    expect(h.phase).toBe('shown');
    expect(h.state.pointerInside).toBe(true);
    // The pointer resting on the strip keeps it up.
    h.advance(10_000);
    expect(h.phase).toBe('shown');
    // Moving off the strip without entering the panel hides it again after the delay.
    h.send({ type: 'pointer', inside: false }).advance(250);
    expect(h.phase).toBe('hidden');
  });

  it('keeps the dock up when the pointer goes from the strip straight into the panel', () => {
    // The strip's `pointerleave` and the panel's `pointerenter` arrive in one dispatch.
    const h = always(250)
      .advance(250)
      .send({ type: 'strip' })
      .send({ type: 'pointer', inside: false })
      .send({ type: 'pointer', inside: true })
      .advance(10_000);
    expect(h.phase).toBe('shown');
  });

  it('treats the strip as inside while shown (no countdown restarts)', () => {
    const h = always(250).send({ type: 'pointer', inside: false });
    expect(h.phase).toBe('pending');
    h.send({ type: 'strip' });
    expect(h.phase).toBe('shown');
    expect(h.timers.at(-1)).toBe('clear');
  });

  it('the window shown on request brings a hidden dock back until the pointer has been and gone', () => {
    const h = always(250).advance(250);
    expect(h.phase).toBe('hidden');
    h.send({ type: 'show' });
    expect(h.phase).toBe('shown');
    h.advance(10_000);
    expect(h.phase).toBe('shown');
    h.send({ type: 'pointer', inside: true }).send({ type: 'pointer', inside: false }).advance(250);
    expect(h.phase).toBe('hidden');
  });

  it('the window shown on request changes nothing unless the dock is hidden', () => {
    const pending = always(250).state;
    expect(pending.phase).toBe('pending');
    expect(autoHideStep(pending, { type: 'show' })).toEqual({ state: pending, timer: 'none' });
    const off = new Harness().send({ type: 'configure', mode: 'off', delayMs: 250 }).state;
    expect(autoHideStep(off, { type: 'show' })).toEqual({ state: off, timer: 'none' });
  });

  it('does nothing on the strip with auto-hide off', () => {
    const off = new Harness().send({ type: 'configure', mode: 'off', delayMs: 250 }).state;
    expect(autoHideStep(off, { type: 'strip' })).toEqual({ state: off, timer: 'none' });
  });

  it.each(['drag', 'menu', 'bounce', 'fileDrag', 'attention', 'focus'] as const)(
    'never hides during a %s hold',
    (hold) => {
      const h = always(250)
        .send({ type: 'pointer', inside: true })
        .send({ type: 'hold', hold, active: true })
        .send({ type: 'pointer', inside: false });
      expect(h.phase).toBe('shown');
      h.advance(10_000);
      expect(h.phase).toBe('shown');
      // The countdown starts when the hold ends with the pointer away.
      h.send({ type: 'hold', hold, active: false });
      expect(h.phase).toBe('pending');
      h.advance(250);
      expect(h.phase).toBe('hidden');
    },
  );

  it('a file dragged onto the strip reveals; the drag holds it; its leave starts the countdown', () => {
    // What file-drag.svelte.ts sends for an OLE drag: the hold on enter, `strip`
    // when the drag is over the strip, the pointer away and the hold off on
    // leave (no pointer events come during such a drag).
    const h = always(250).advance(250);
    expect(h.phase).toBe('hidden');
    h.send({ type: 'hold', hold: 'fileDrag', active: true });
    expect(h.phase).toBe('hidden');
    h.send({ type: 'strip' });
    expect(h.phase).toBe('shown');
    h.advance(10_000);
    expect(h.phase).toBe('shown');
    h.send({ type: 'pointer', inside: false }).send({
      type: 'hold',
      hold: 'fileDrag',
      active: false,
    });
    expect(h.phase).toBe('pending');
    h.advance(250);
    expect(h.phase).toBe('hidden');
  });

  it('a hold that begins during the countdown cancels it', () => {
    const h = always(250).send({ type: 'pointer', inside: false }).advance(100);
    expect(h.phase).toBe('pending');
    h.send({ type: 'hold', hold: 'menu', active: true });
    expect(h.phase).toBe('shown');
    expect(h.timers.at(-1)).toBe('clear');
    h.advance(10_000);
    expect(h.phase).toBe('shown');
  });

  it('a hold ending with the pointer inside starts nothing', () => {
    const h = always(250)
      .send({ type: 'pointer', inside: true })
      .send({ type: 'hold', hold: 'drag', active: true })
      .send({ type: 'hold', hold: 'drag', active: false })
      .advance(10_000);
    expect(h.phase).toBe('shown');
  });

  it('holds overlap: the dock stays until the last one ends', () => {
    const h = always(250)
      .send({ type: 'hold', hold: 'bounce', active: true })
      .send({ type: 'hold', hold: 'menu', active: true })
      .send({ type: 'hold', hold: 'bounce', active: false })
      .advance(10_000);
    expect(h.phase).toBe('shown');
    h.send({ type: 'hold', hold: 'menu', active: false }).advance(250);
    expect(h.phase).toBe('hidden');
  });

  it('a hold never reveals a hidden dock (a bounce for a launch from elsewhere)', () => {
    const h = always(250).advance(250);
    expect(h.phase).toBe('hidden');
    h.send({ type: 'hold', hold: 'bounce', active: true });
    expect(h.phase).toBe('hidden');
    h.send({ type: 'hold', hold: 'bounce', active: false }).advance(10_000);
    expect(h.phase).toBe('hidden');
  });

  it('an item with keyboard focus keeps the dock up; once focus leaves it hides after the delay', () => {
    // What keyboard.svelte.ts sends: the hold on the panel's focusin, off on its
    // focusout (moving between items sends nothing). The pointer is away
    // throughout, as when the keyboard is used alone.
    const h = always(250).send({ type: 'hold', hold: 'focus', active: true });
    expect(h.phase).toBe('shown');
    expect(h.timers.at(-1)).toBe('clear');
    h.advance(10_000);
    expect(h.phase).toBe('shown');
    // The pointer visiting and leaving does not hide it while an item has focus.
    h.send({ type: 'pointer', inside: true }).send({ type: 'pointer', inside: false });
    h.advance(10_000);
    expect(h.phase).toBe('shown');
    // Escape, Tab out or a click elsewhere: the countdown starts.
    h.send({ type: 'hold', hold: 'focus', active: false });
    expect(h.phase).toBe('pending');
    expect(h.timers.at(-1)).toBe('arm');
    h.advance(249);
    expect(h.phase).toBe('pending');
    h.advance(1);
    expect(h.phase).toBe('hidden');
  });

  it('keyboard focus never reveals a hidden dock and changes nothing with auto-hide off', () => {
    expect(REVEALING_HOLDS.has('focus')).toBe(false);
    const h = always(250).advance(250);
    expect(h.phase).toBe('hidden');
    h.send({ type: 'hold', hold: 'focus', active: true });
    expect(h.phase).toBe('hidden');
    expect(h.timers.at(-1)).toBe('none');
    h.send({ type: 'hold', hold: 'focus', active: false }).advance(10_000);
    expect(h.phase).toBe('hidden');

    const off = new Harness().send({ type: 'configure', mode: 'off', delayMs: 250 });
    off.send({ type: 'hold', hold: 'focus', active: true });
    off.send({ type: 'hold', hold: 'focus', active: false }).advance(10_000);
    expect(off.phase).toBe('shown');
    expect(off.timers.every((t) => t === 'none')).toBe(true);
  });

  it('keyboard focus holds a fullscreen dock until it leaves, then hides through a 0 ms countdown', () => {
    const h = new Harness()
      .send({ type: 'configure', mode: 'off', delayMs: 250 })
      .send({ type: 'hold', hold: 'focus', active: true })
      .send({ type: 'fullscreen', active: true });
    expect(h.phase).toBe('shown');
    h.advance(10_000);
    expect(h.phase).toBe('shown');
    h.send({ type: 'hold', hold: 'focus', active: false });
    expect(h.phase).toBe('pending');
    h.advance(0);
    expect(h.phase).toBe('hidden');
  });

  it('focus and a menu opened from it overlap: the dock stays until both have ended', () => {
    // Shift+F10 on a focused item: the menu hold comes on top of the focus one.
    const h = always(250)
      .send({ type: 'hold', hold: 'focus', active: true })
      .send({ type: 'hold', hold: 'menu', active: true })
      .send({ type: 'hold', hold: 'menu', active: false })
      .advance(10_000);
    expect(h.phase).toBe('shown');
    h.send({ type: 'hold', hold: 'focus', active: false }).advance(250);
    expect(h.phase).toBe('hidden');
  });

  it('an item that needs attention brings a hidden dock back and keeps it up until it is answered', () => {
    expect([...REVEALING_HOLDS]).toEqual(['attention']);
    const h = always(250).advance(250);
    expect(h.phase).toBe('hidden');
    h.send({ type: 'hold', hold: 'attention', active: true });
    expect(h.phase).toBe('shown');
    // Revealed for the item, not for the pointer: it is still away.
    expect(h.state.pointerInside).toBe(false);
    h.advance(10_000);
    expect(h.phase).toBe('shown');
    // The pointer visiting and leaving does not hide it while the item waits.
    h.send({ type: 'pointer', inside: true }).send({ type: 'pointer', inside: false });
    h.advance(10_000);
    expect(h.phase).toBe('shown');
    // The window was activated from elsewhere: the countdown starts.
    h.send({ type: 'hold', hold: 'attention', active: false });
    expect(h.phase).toBe('pending');
    h.advance(250);
    expect(h.phase).toBe('hidden');
  });

  it('attention answered from the dock itself leaves it up under the pointer', () => {
    const h = always(250).advance(250);
    h.send({ type: 'hold', hold: 'attention', active: true });
    // The user clicks the wiggling icon: the pointer is over the panel when the item is answered.
    h.send({ type: 'pointer', inside: true }).send({
      type: 'hold',
      hold: 'attention',
      active: false,
    });
    h.advance(10_000);
    expect(h.phase).toBe('shown');
    h.send({ type: 'pointer', inside: false }).advance(250);
    expect(h.phase).toBe('hidden');
  });

  it('attention reveals nothing with auto-hide off and repeats are harmless', () => {
    const off = new Harness().send({ type: 'configure', mode: 'off', delayMs: 250 });
    off.send({ type: 'hold', hold: 'attention', active: true });
    expect(off.phase).toBe('shown');
    expect(off.timers.at(-1)).toBe('none');
    // Turned on while an item waits: it stays up.
    off.send({ type: 'configure', mode: 'always', delayMs: 250 }).advance(10_000);
    expect(off.phase).toBe('shown');

    const h = always(250).advance(250);
    h.send({ type: 'hold', hold: 'attention', active: true });
    h.send({ type: 'hold', hold: 'attention', active: true });
    expect(h.phase).toBe('shown');
    expect(h.timers.slice(-2)).toEqual(['none', 'none']);
    // Ending a hold that never began changes nothing.
    const quiet = always(250).advance(250);
    quiet.send({ type: 'hold', hold: 'attention', active: false });
    expect(quiet.phase).toBe('hidden');
  });

  it('turning auto-hide off reveals at once and stops the countdown', () => {
    const hidden = always(250).advance(250);
    expect(hidden.phase).toBe('hidden');
    hidden.send({ type: 'configure', mode: 'off', delayMs: 250 });
    expect(hidden.phase).toBe('shown');
    expect(hidden.timers.at(-1)).toBe('none');

    const pending = always(250);
    pending.send({ type: 'configure', mode: 'whenOverlapped', delayMs: 250 });
    expect(pending.phase).toBe('shown');
    expect(pending.timers.at(-1)).toBe('clear');
    pending.advance(10_000);
    expect(pending.phase).toBe('shown');
  });

  it('with delayMs 0 it still hides through a 0 ms countdown, never between two events', () => {
    // Hiding at once would let a strip-leave followed by a panel-enter in one
    // dispatch leave the dock hidden.
    const h = always(0);
    expect(h.phase).toBe('pending');
    expect(h.timers).toEqual(['arm']);
    expect(hideDelayMs(h.state)).toBe(0);
    h.advance(0);
    expect(h.phase).toBe('hidden');
    h.send({ type: 'strip' });
    expect(h.phase).toBe('shown');
    // From the strip straight into the panel: still up.
    h.send({ type: 'pointer', inside: false });
    expect(h.phase).toBe('pending');
    h.send({ type: 'pointer', inside: true });
    expect(h.phase).toBe('shown');
    expect(h.timers.at(-1)).toBe('clear');
    h.advance(10_000);
    expect(h.phase).toBe('shown');
    // Away: hidden as soon as the timer fires.
    h.send({ type: 'pointer', inside: false });
    expect(h.phase).toBe('pending');
    expect(h.timers.at(-1)).toBe('arm');
    h.advance(0);
    expect(h.phase).toBe('hidden');
  });

  it('a delay change restarts a running countdown; other settings changes do not', () => {
    const h = always(1000).advance(600);
    h.send({ type: 'configure', mode: 'always', delayMs: 1000 });
    expect(h.timers.at(-1)).toBe('none');
    h.advance(400);
    expect(h.phase).toBe('hidden');

    const g = always(1000).advance(600);
    g.send({ type: 'configure', mode: 'always', delayMs: 500 });
    expect(g.timers.at(-1)).toBe('arm');
    g.advance(499);
    expect(g.phase).toBe('pending');
    g.advance(1);
    expect(g.phase).toBe('hidden');
  });

  it('a delay change to 0 during the countdown restarts it as a 0 ms one', () => {
    const h = always(1000).advance(10);
    h.send({ type: 'configure', mode: 'always', delayMs: 0 });
    expect(h.phase).toBe('pending');
    expect(h.timers.at(-1)).toBe('arm');
    expect(hideDelayMs(h.state)).toBe(0);
    h.advance(0);
    expect(h.phase).toBe('hidden');
  });

  it('settings changes while hidden keep it hidden', () => {
    const h = always(250).advance(250);
    h.send({ type: 'configure', mode: 'always', delayMs: 800 });
    expect(h.phase).toBe('hidden');
    expect(h.state.delayMs).toBe(800);
  });

  it('clamps the delay like Rust does', () => {
    expect(always(9000).state.delayMs).toBe(5000);
    expect(always(-5).state.delayMs).toBe(0);
    expect(always(12.6).state.delayMs).toBe(13);
    expect(always(Number.NaN).state.delayMs).toBe(0);
  });

  it('never mutates the state it is given', () => {
    const before = always(250).state;
    const copy = structuredClone(before);
    autoHideStep(before, { type: 'hold', hold: 'drag', active: true });
    autoHideStep(before, { type: 'hold', hold: 'focus', active: true });
    const hidden = always(0).state;
    const hiddenCopy = structuredClone(hidden);
    autoHideStep(hidden, { type: 'hold', hold: 'attention', active: true });
    autoHideStep(hidden, { type: 'hold', hold: 'focus', active: true });
    expect(hidden).toEqual(hiddenCopy);
    autoHideStep(before, { type: 'pointer', inside: true });
    autoHideStep(before, { type: 'configure', mode: 'off', delayMs: 0 });
    expect(before).toEqual(copy);
  });

  it('slides for 200 ms over a 2 px strip (ADR-0008)', () => {
    expect(AUTO_HIDE_SLIDE_MS).toBe(200);
    expect(REVEAL_STRIP_PX).toBe(2);
  });
});

describe('fullscreen', () => {
  function off(): Harness {
    return new Harness().send({ type: 'configure', mode: 'off', delayMs: 250 });
  }

  it('is off at the start and never active with auto-hide off alone', () => {
    expect(INITIAL_AUTO_HIDE.fullscreen).toBe(false);
    expect(autoHideActive(INITIAL_AUTO_HIDE)).toBe(false);
    expect(hideDelayMs(INITIAL_AUTO_HIDE)).toBe(0);
    const h = off();
    expect(autoHideActive(h.state)).toBe(false);
    expect(h.state.enabled).toBe(false);
    expect(hideDelayMs(h.state)).toBe(250);
  });

  it('hides at once with the pointer away, whatever the mode', () => {
    for (const mode of ['off', 'whenOverlapped'] as const) {
      const h = new Harness().send({ type: 'configure', mode, delayMs: 250 });
      h.send({ type: 'fullscreen', active: true });
      expect(h.phase).toBe('hidden');
      expect(h.timers.at(-1)).toBe('none');
      expect(isAutoHidden(h.state)).toBe(true);
      // Active for the strip and the hit region, while `enabled` still says the mode.
      expect(autoHideActive(h.state)).toBe(true);
      expect(h.state.enabled).toBe(false);
      expect(hideDelayMs(h.state)).toBe(0);
    }
    // With `always` and the countdown running: out now, countdown dropped.
    const pending = always(250).send({ type: 'pointer', inside: false }).advance(100);
    expect(pending.phase).toBe('pending');
    pending.send({ type: 'fullscreen', active: true });
    expect(pending.phase).toBe('hidden');
    expect(pending.timers.at(-1)).toBe('clear');
    // Already hidden with `always`: nothing to do.
    const hidden = always(250).advance(250);
    hidden.send({ type: 'fullscreen', active: true });
    expect(hidden.phase).toBe('hidden');
    expect(hidden.timers.at(-1)).toBe('none');
  });

  it('with the pointer over the dock it stays until the pointer leaves, then hides through a 0 ms countdown', () => {
    const h = off().send({ type: 'pointer', inside: true });
    h.send({ type: 'fullscreen', active: true });
    expect(h.phase).toBe('shown');
    expect(h.timers.at(-1)).toBe('none');
    h.advance(10_000);
    expect(h.phase).toBe('shown');
    // The leave goes through `pending` and a 0 ms timer, never straight to
    // `hidden`: a strip-leave followed by a panel-enter in the same dispatch
    // must leave the dock up (below).
    h.send({ type: 'pointer', inside: false });
    expect(h.phase).toBe('pending');
    expect(h.timers.at(-1)).toBe('arm');
    expect(hideDelayMs(h.state)).toBe(0);
    h.advance(0);
    expect(h.phase).toBe('hidden');
  });

  it('the strip reveals, the pointer resting there keeps it up, and leaving hides it again', () => {
    const h = off().send({ type: 'fullscreen', active: true });
    expect(h.phase).toBe('hidden');
    h.send({ type: 'strip' });
    expect(h.phase).toBe('shown');
    expect(h.state.pointerInside).toBe(true);
    h.advance(10_000);
    expect(h.phase).toBe('shown');
    // From the strip straight into the panel: `pointerleave` of the strip
    // and `pointerenter` of the panel arrive in one dispatch, before the
    // 0 ms timer can fire.
    h.send({ type: 'pointer', inside: false });
    expect(h.phase).toBe('pending');
    h.send({ type: 'pointer', inside: true });
    expect(h.phase).toBe('shown');
    expect(h.timers.at(-1)).toBe('clear');
    h.advance(10_000);
    expect(h.phase).toBe('shown');
    // Away again: hidden as soon as the timer fires.
    h.send({ type: 'pointer', inside: false }).advance(0);
    expect(h.phase).toBe('hidden');
    // Pointer events elsewhere change nothing while hidden.
    h.send({ type: 'pointer', inside: true }).send({ type: 'pointer', inside: false });
    expect(h.phase).toBe('hidden');
  });

  it('the window shown on request reveals, as with auto-hide on', () => {
    const h = off().send({ type: 'fullscreen', active: true });
    h.send({ type: 'show' });
    expect(h.phase).toBe('shown');
    h.advance(10_000);
    expect(h.phase).toBe('shown');
    h.send({ type: 'pointer', inside: true }).send({ type: 'pointer', inside: false }).advance(0);
    expect(h.phase).toBe('hidden');
  });

  it.each(['drag', 'menu', 'bounce', 'fileDrag', 'attention', 'focus'] as const)(
    'a %s hold keeps a revealed dock up; its end with the pointer away hides at once',
    (hold) => {
      const h = off().send({ type: 'fullscreen', active: true }).send({ type: 'strip' });
      h.send({ type: 'hold', hold, active: true }).send({ type: 'pointer', inside: false });
      expect(h.phase).toBe('shown');
      h.advance(10_000);
      expect(h.phase).toBe('shown');
      h.send({ type: 'hold', hold, active: false });
      expect(h.phase).toBe('pending');
      h.advance(0);
      expect(h.phase).toBe('hidden');
    },
  );

  it('fullscreen during a drag: the dock stays lifted until the drop, then hides at once', () => {
    const h = off()
      .send({ type: 'pointer', inside: true })
      .send({ type: 'hold', hold: 'drag', active: true })
      .send({ type: 'pointer', inside: false });
    h.send({ type: 'fullscreen', active: true });
    expect(h.phase).toBe('shown');
    expect(h.timers.at(-1)).toBe('none');
    h.advance(10_000);
    expect(h.phase).toBe('shown');
    // Released away from the dock (what `endDragHold` sends).
    h.send({ type: 'pointer', inside: false }).send({ type: 'hold', hold: 'drag', active: false });
    expect(h.phase).toBe('pending');
    h.advance(0);
    expect(h.phase).toBe('hidden');
    // Released over the dock instead: it stays until the pointer leaves.
    const g = off()
      .send({ type: 'hold', hold: 'drag', active: true })
      .send({ type: 'fullscreen', active: true })
      .send({ type: 'pointer', inside: true })
      .send({ type: 'hold', hold: 'drag', active: false })
      .advance(10_000);
    expect(g.phase).toBe('shown');
  });

  it('an item that needs attention brings the dock back and keeps it up while fullscreen', () => {
    const h = off().send({ type: 'fullscreen', active: true });
    expect(h.phase).toBe('hidden');
    h.send({ type: 'hold', hold: 'attention', active: true });
    expect(h.phase).toBe('shown');
    h.advance(10_000);
    expect(h.phase).toBe('shown');
    h.send({ type: 'hold', hold: 'attention', active: false });
    expect(h.phase).toBe('pending');
    h.advance(0);
    expect(h.phase).toBe('hidden');
    // A bounce for a launch from elsewhere still reveals nothing.
    h.send({ type: 'hold', hold: 'bounce', active: true });
    expect(h.phase).toBe('hidden');
  });

  it('the end of fullscreen shows the dock again with the mode off, and leaves it shown', () => {
    for (const mode of ['off', 'whenOverlapped'] as const) {
      const h = new Harness().send({ type: 'configure', mode, delayMs: 250 });
      h.send({ type: 'fullscreen', active: true });
      expect(h.phase).toBe('hidden');
      h.send({ type: 'fullscreen', active: false });
      expect(h.phase).toBe('shown');
      expect(h.timers.at(-1)).toBe('none');
      expect(autoHideActive(h.state)).toBe(false);
      h.send({ type: 'pointer', inside: true }).send({ type: 'pointer', inside: false });
      h.advance(10_000);
      expect(h.phase).toBe('shown');
    }
    // Ending while the 0 ms countdown runs: the timer is cleared.
    const pending = off()
      .send({ type: 'pointer', inside: true })
      .send({ type: 'fullscreen', active: true })
      .send({ type: 'pointer', inside: false });
    expect(pending.phase).toBe('pending');
    pending.send({ type: 'fullscreen', active: false });
    expect(pending.phase).toBe('shown');
    expect(pending.timers.at(-1)).toBe('clear');
    pending.advance(10_000);
    expect(pending.phase).toBe('shown');
  });

  it('the end of fullscreen with the mode always: hidden stays hidden, shown hides after the normal delay', () => {
    // Hidden before, during and after: the stock behaviour, no reveal.
    const hidden = always(250).advance(250);
    hidden.send({ type: 'fullscreen', active: true }).send({ type: 'fullscreen', active: false });
    expect(hidden.phase).toBe('hidden');
    expect(hidden.timers.slice(-2)).toEqual(['none', 'none']);
    hidden.advance(10_000);
    expect(hidden.phase).toBe('hidden');
    hidden.send({ type: 'strip' });
    expect(hidden.phase).toBe('shown');

    // Revealed from the strip during fullscreen, the pointer away when it
    // ends: the normal countdown, not the 0 ms one.
    const shown = always(250).advance(250).send({ type: 'fullscreen', active: true });
    shown.send({ type: 'strip' }).send({ type: 'hold', hold: 'menu', active: true });
    shown.send({ type: 'pointer', inside: false });
    expect(shown.phase).toBe('shown');
    shown.send({ type: 'fullscreen', active: false });
    expect(shown.phase).toBe('shown');
    shown.send({ type: 'hold', hold: 'menu', active: false });
    expect(shown.phase).toBe('pending');
    expect(hideDelayMs(shown.state)).toBe(250);
    shown.advance(249);
    expect(shown.phase).toBe('pending');
    shown.advance(1);
    expect(shown.phase).toBe('hidden');

    // A 0 ms countdown running when fullscreen ends is restarted with the delay.
    const restarted = always(250)
      .send({ type: 'pointer', inside: true })
      .send({ type: 'fullscreen', active: true })
      .send({ type: 'pointer', inside: false });
    expect(restarted.phase).toBe('pending');
    restarted.send({ type: 'fullscreen', active: false });
    expect(restarted.phase).toBe('pending');
    expect(restarted.timers.at(-1)).toBe('arm');
    restarted.advance(249);
    expect(restarted.phase).toBe('pending');
    restarted.advance(1);
    expect(restarted.phase).toBe('hidden');
  });

  it('settings changes during fullscreen do not reveal; repeats are harmless', () => {
    const h = off().send({ type: 'fullscreen', active: true });
    h.send({ type: 'configure', mode: 'always', delayMs: 500 });
    expect(h.phase).toBe('hidden');
    h.send({ type: 'configure', mode: 'off', delayMs: 500 });
    expect(h.phase).toBe('hidden');
    expect(autoHideActive(h.state)).toBe(true);
    h.send({ type: 'fullscreen', active: true });
    expect(h.timers.at(-1)).toBe('none');
    expect(h.phase).toBe('hidden');
    // Off twice: the second is a no-op.
    h.send({ type: 'fullscreen', active: false });
    expect(h.phase).toBe('shown');
    const settled = h.state;
    expect(autoHideStep(settled, { type: 'fullscreen', active: false })).toEqual({
      state: settled,
      timer: 'none',
    });
  });

  it('a late timer from the 0 ms countdown is ignored once fullscreen ended', () => {
    const h = off()
      .send({ type: 'pointer', inside: true })
      .send({ type: 'fullscreen', active: true })
      .send({ type: 'pointer', inside: false });
    expect(h.phase).toBe('pending');
    h.send({ type: 'fullscreen', active: false });
    expect(h.phase).toBe('shown');
    const shown = h.state;
    expect(autoHideStep(shown, { type: 'timeout' })).toEqual({ state: shown, timer: 'none' });
  });

  it('never mutates the state it is given', () => {
    const before = off().send({ type: 'pointer', inside: true }).state;
    const copy = structuredClone(before);
    autoHideStep(before, { type: 'fullscreen', active: true });
    autoHideStep(before, { type: 'fullscreen', active: false });
    expect(before).toEqual(copy);
    const hidden = off().send({ type: 'fullscreen', active: true }).state;
    const hiddenCopy = structuredClone(hidden);
    autoHideStep(hidden, { type: 'strip' });
    autoHideStep(hidden, { type: 'fullscreen', active: false });
    expect(hidden).toEqual(hiddenCopy);
  });
});

describe('when overlapped', () => {
  function dodging(delayMs = 250): Harness {
    return new Harness().send({ type: 'configure', mode: 'whenOverlapped', delayMs });
  }

  it('starts with no overlap and is active only with the mode whenOverlapped', () => {
    expect(INITIAL_AUTO_HIDE.overlapped).toBe(false);
    expect(INITIAL_AUTO_HIDE.dodges).toBe(false);
    const h = dodging();
    expect(h.state.dodges).toBe(true);
    expect(h.state.enabled).toBe(false);
    expect(autoHideActive(h.state)).toBe(false);
    expect(hideDelayMs(h.state)).toBe(250);
    // The fact is kept whatever the mode; only `whenOverlapped` acts on it.
    for (const mode of ['off', 'always'] as const) {
      const g = new Harness().send({ type: 'configure', mode, delayMs: 250 });
      const before = g.phase;
      g.send({ type: 'overlapped', active: true });
      expect(g.state.overlapped).toBe(true);
      expect(g.phase).toBe(before);
      expect(g.timers.at(-1)).toBe('none');
      expect(autoHideActive(g.state)).toBe(mode === 'always');
    }
  });

  it('a window over the dock hides it after the delay, never at once; away again shows it', () => {
    const h = dodging(250);
    h.send({ type: 'overlapped', active: true });
    expect(h.phase).toBe('pending');
    expect(h.timers.at(-1)).toBe('arm');
    expect(autoHideActive(h.state)).toBe(true);
    expect(hideDelayMs(h.state)).toBe(250);
    h.advance(249);
    expect(h.phase).toBe('pending');
    h.advance(1);
    expect(h.phase).toBe('hidden');
    expect(isAutoHidden(h.state)).toBe(true);
    h.advance(10_000);
    expect(h.phase).toBe('hidden');

    // The window moved away: back at once, no strip needed.
    h.send({ type: 'overlapped', active: false });
    expect(h.phase).toBe('shown');
    expect(h.timers.at(-1)).toBe('none');
    expect(autoHideActive(h.state)).toBe(false);
    h.send({ type: 'pointer', inside: true }).send({ type: 'pointer', inside: false });
    h.advance(10_000);
    expect(h.phase).toBe('shown');
  });

  it('a window passing by during the delay leaves the dock up', () => {
    const h = dodging(250).send({ type: 'overlapped', active: true }).advance(100);
    expect(h.phase).toBe('pending');
    h.send({ type: 'overlapped', active: false });
    expect(h.phase).toBe('shown');
    expect(h.timers.at(-1)).toBe('clear');
    h.advance(10_000);
    expect(h.phase).toBe('shown');
  });

  it('even with delayMs 0 it goes through a 0 ms countdown', () => {
    const h = dodging(0).send({ type: 'overlapped', active: true });
    expect(h.phase).toBe('pending');
    expect(h.timers.at(-1)).toBe('arm');
    expect(hideDelayMs(h.state)).toBe(0);
    h.advance(0);
    expect(h.phase).toBe('hidden');
  });

  it('with the pointer over the dock it stays until the pointer leaves, then counts down', () => {
    const h = dodging(250).send({ type: 'pointer', inside: true });
    h.send({ type: 'overlapped', active: true });
    expect(h.phase).toBe('shown');
    expect(h.timers.at(-1)).toBe('none');
    h.advance(10_000);
    expect(h.phase).toBe('shown');
    h.send({ type: 'pointer', inside: false });
    expect(h.phase).toBe('pending');
    h.advance(250);
    expect(h.phase).toBe('hidden');
    // Pointer events elsewhere change nothing while hidden.
    h.send({ type: 'pointer', inside: true }).send({ type: 'pointer', inside: false });
    expect(h.phase).toBe('hidden');
  });

  it('the strip reveals a dock hidden under a window; leaving it hides again after the delay', () => {
    const h = dodging(250).send({ type: 'overlapped', active: true }).advance(250);
    expect(h.phase).toBe('hidden');
    h.send({ type: 'strip' });
    expect(h.phase).toBe('shown');
    expect(h.state.pointerInside).toBe(true);
    h.advance(10_000);
    expect(h.phase).toBe('shown');
    // From the strip straight into the panel: still up.
    h.send({ type: 'pointer', inside: false });
    expect(h.phase).toBe('pending');
    h.send({ type: 'pointer', inside: true });
    expect(h.phase).toBe('shown');
    expect(h.timers.at(-1)).toBe('clear');
    // Away: the normal countdown, the window still being there.
    h.send({ type: 'pointer', inside: false });
    expect(h.phase).toBe('pending');
    h.advance(249);
    expect(h.phase).toBe('pending');
    h.advance(1);
    expect(h.phase).toBe('hidden');
  });

  it('the window shown on request reveals, as with auto-hide on', () => {
    const h = dodging(250).send({ type: 'overlapped', active: true }).advance(250);
    h.send({ type: 'show' });
    expect(h.phase).toBe('shown');
    h.advance(10_000);
    expect(h.phase).toBe('shown');
    h.send({ type: 'pointer', inside: true }).send({ type: 'pointer', inside: false }).advance(250);
    expect(h.phase).toBe('hidden');
  });

  it.each(['drag', 'menu', 'bounce', 'fileDrag', 'attention', 'focus'] as const)(
    'a %s hold keeps the dock up over an overlapping window; its end with the pointer away starts the countdown',
    (hold) => {
      const h = dodging(250)
        .send({ type: 'pointer', inside: true })
        .send({ type: 'hold', hold, active: true })
        .send({ type: 'pointer', inside: false });
      h.send({ type: 'overlapped', active: true });
      expect(h.phase).toBe('shown');
      h.advance(10_000);
      expect(h.phase).toBe('shown');
      h.send({ type: 'hold', hold, active: false });
      expect(h.phase).toBe('pending');
      h.advance(250);
      expect(h.phase).toBe('hidden');
    },
  );

  it('an item that needs attention brings the dock back over the window and keeps it up', () => {
    const h = dodging(250).send({ type: 'overlapped', active: true }).advance(250);
    expect(h.phase).toBe('hidden');
    h.send({ type: 'hold', hold: 'attention', active: true });
    expect(h.phase).toBe('shown');
    h.advance(10_000);
    expect(h.phase).toBe('shown');
    h.send({ type: 'hold', hold: 'attention', active: false });
    expect(h.phase).toBe('pending');
    h.advance(250);
    expect(h.phase).toBe('hidden');
    // A bounce for a launch from elsewhere still reveals nothing.
    h.send({ type: 'hold', hold: 'bounce', active: true });
    expect(h.phase).toBe('hidden');
  });

  it('switching the mode while a window overlaps: off shows, always keeps hiding, whenOverlapped starts the countdown', () => {
    // The fact arrived before the mode: hidden the moment the mode asks for it (after the delay).
    const late = new Harness()
      .send({ type: 'configure', mode: 'off', delayMs: 250 })
      .send({ type: 'overlapped', active: true });
    expect(late.phase).toBe('shown');
    late.send({ type: 'configure', mode: 'whenOverlapped', delayMs: 250 });
    expect(late.phase).toBe('pending');
    late.advance(250);
    expect(late.phase).toBe('hidden');

    // Off reveals at once.
    late.send({ type: 'configure', mode: 'off', delayMs: 250 });
    expect(late.phase).toBe('shown');
    expect(late.state.overlapped).toBe(true);
    expect(autoHideActive(late.state)).toBe(false);

    // Always keeps a hidden dock hidden; the overlap ending then changes nothing.
    const hidden = dodging(250).send({ type: 'overlapped', active: true }).advance(250);
    hidden.send({ type: 'configure', mode: 'always', delayMs: 250 });
    expect(hidden.phase).toBe('hidden');
    hidden.send({ type: 'overlapped', active: false });
    expect(hidden.phase).toBe('hidden');
    expect(hidden.timers.at(-1)).toBe('none');
    hidden.send({ type: 'strip' });
    expect(hidden.phase).toBe('shown');
  });

  it('a delay change restarts a running overlap countdown', () => {
    const h = dodging(1000).send({ type: 'overlapped', active: true }).advance(600);
    h.send({ type: 'configure', mode: 'whenOverlapped', delayMs: 500 });
    expect(h.timers.at(-1)).toBe('arm');
    h.advance(499);
    expect(h.phase).toBe('pending');
    h.advance(1);
    expect(h.phase).toBe('hidden');
  });

  it('fullscreen and overlap are independent: whichever still applies keeps the dock hidden', () => {
    // Hidden by the overlap; a fullscreen app comes and goes: still hidden.
    const h = dodging(250).send({ type: 'overlapped', active: true }).advance(250);
    h.send({ type: 'fullscreen', active: true });
    expect(h.phase).toBe('hidden');
    h.send({ type: 'fullscreen', active: false });
    expect(h.phase).toBe('hidden');
    expect(autoHideActive(h.state)).toBe(true);
    h.send({ type: 'overlapped', active: false });
    expect(h.phase).toBe('shown');

    // Hidden by fullscreen; the overlap comes and goes: still hidden, and
    // the end of fullscreen reveals only once the window is away too.
    const g = dodging(250).send({ type: 'fullscreen', active: true });
    expect(g.phase).toBe('hidden');
    g.send({ type: 'overlapped', active: true }).send({ type: 'overlapped', active: false });
    expect(g.phase).toBe('hidden');
    g.send({ type: 'overlapped', active: true }).send({ type: 'fullscreen', active: false });
    expect(g.phase).toBe('hidden');
    g.send({ type: 'overlapped', active: false });
    expect(g.phase).toBe('shown');

    // Revealed from the strip with both on, the pointer away: fullscreen's
    // 0 ms countdown; fullscreen ending restarts it with the real delay.
    const both = dodging(250)
      .send({ type: 'overlapped', active: true })
      .send({ type: 'fullscreen', active: true })
      .send({ type: 'strip' })
      .send({ type: 'pointer', inside: false });
    expect(both.phase).toBe('pending');
    expect(hideDelayMs(both.state)).toBe(0);
    both.send({ type: 'fullscreen', active: false });
    expect(both.phase).toBe('pending');
    expect(both.timers.at(-1)).toBe('arm');
    expect(hideDelayMs(both.state)).toBe(250);
    both.advance(250);
    expect(both.phase).toBe('hidden');
  });

  it('repeats are harmless and the state is never mutated', () => {
    const h = dodging(250).send({ type: 'overlapped', active: true });
    h.send({ type: 'overlapped', active: true });
    expect(h.timers.at(-1)).toBe('none');
    expect(h.phase).toBe('pending');
    const settled = h.send({ type: 'overlapped', active: false }).state;
    expect(autoHideStep(settled, { type: 'overlapped', active: false })).toEqual({
      state: settled,
      timer: 'none',
    });
    const copy = structuredClone(settled);
    autoHideStep(settled, { type: 'overlapped', active: true });
    autoHideStep(settled, { type: 'configure', mode: 'off', delayMs: 0 });
    expect(settled).toEqual(copy);
    const hidden = dodging(250).send({ type: 'overlapped', active: true }).advance(250).state;
    const hiddenCopy = structuredClone(hidden);
    autoHideStep(hidden, { type: 'overlapped', active: false });
    autoHideStep(hidden, { type: 'strip' });
    expect(hidden).toEqual(hiddenCopy);
  });
});

describe('hit regions while auto-hide is on', () => {
  // A 559 × 64 panel resting 8 px above the bottom of a 1280 × 240 viewport.
  const viewport = { width: 1280, height: 240 };
  const panel = { x: 360, y: 168, width: 561, height: 64 };
  const region: HitRegion = {
    panel,
    envelope: { x: 278, y: 147, width: 725, height: 85 },
  };

  it('the strip is the bottom 2 px, exactly as wide as the panel and in line with it', () => {
    expect(revealStrip(panel, viewport, 'bottom')).toEqual({
      x: 360,
      y: 238,
      width: 561,
      height: 2,
    });
  });

  it('the strip never reaches beyond a tiny viewport', () => {
    expect(revealStrip(panel, { width: 1280, height: 1 }, 'bottom')).toEqual({
      x: 360,
      y: 0,
      width: 561,
      height: 1,
    });
    expect(revealStrip(panel, { width: 1280, height: 0 }, 'bottom')).toEqual({
      x: 360,
      y: 0,
      width: 561,
      height: 0,
    });
    expect(revealStrip(panel, { width: 1, height: 900 }, 'right')).toEqual({
      x: 0,
      y: 168,
      width: 1,
      height: 64,
    });
  });

  it('while hidden the strip is both rectangles', () => {
    const strip = { x: 360, y: 238, width: 561, height: 2 };
    expect(hiddenHitRegion(panel, viewport, 'bottom')).toEqual({ panel: strip, envelope: strip });
  });

  it('while shown the envelope takes in the strip and the panel is unchanged', () => {
    expect(shownHitRegion(region, viewport, 'bottom')).toEqual({
      panel,
      envelope: { x: 278, y: 147, width: 725, height: 93 },
    });
  });

  it('an envelope that already reaches the edge (a drag) is unchanged', () => {
    const whole: HitRegion = { panel, envelope: { x: 0, y: 0, width: 1280, height: 240 } };
    for (const edge of DOCK_EDGES) {
      expect(shownHitRegion(whole, viewport, edge)).toEqual(whole);
    }
  });

  it('follows the phase, and reports nothing while hidden without a strip', () => {
    const strip = { x: 360, y: 238, width: 561, height: 2 };
    const at = (h: Harness, atEdge: boolean): HitRegion =>
      autoHideHitRegion(h.state, region, viewport, atEdge, 'bottom');
    // Off: the plain region, wherever the window is.
    expect(at(new Harness(), true)).toEqual(region);
    expect(at(new Harness(), false)).toEqual(region);
    // `always`: the strip while hidden, the grown envelope while shown,
    // whether or not the work area reaches the screen's edge.
    for (const atEdge of [true, false]) {
      expect(at(always(250).advance(250), atEdge)).toEqual({ panel: strip, envelope: strip });
      expect(at(always(250).send({ type: 'pointer', inside: true }), atEdge)).toEqual(
        shownHitRegion(region, viewport, 'bottom'),
      );
    }
    // Fullscreen alone at the screen's edge: the same.
    const off = (): Harness => new Harness().send({ type: 'configure', mode: 'off', delayMs: 0 });
    const hidden = off().send({ type: 'fullscreen', active: true });
    expect(at(hidden, true)).toEqual({ panel: strip, envelope: strip });
    // Above a visible taskbar: no region while hidden, the plain one shown.
    const none = at(hidden, false);
    expect(none).toEqual({
      panel: { x: 0, y: 0, width: 0, height: 0 },
      envelope: { x: 0, y: 0, width: 0, height: 0 },
    });
    expect(none).toEqual(NO_HIT_REGION);
    expect(none).not.toBe(NO_HIT_REGION);
    for (let x = -1; x <= 1; x += 1) {
      for (let y = -1; y <= 1; y += 1) {
        expect(rectContainsPoint(none.panel, x, y)).toBe(false);
      }
    }
    const shown = off().send({ type: 'fullscreen', active: true }).send({ type: 'show' });
    expect(at(shown, false)).toEqual(region);
  });
});

describe('hit regions on every edge', () => {
  // The same 64 px thick panel on each edge of its window, 8 px from the edge side:
  // a 1280 × 240 window on `bottom` and `top`, a 330 × 900 one on `left` and `right`.
  const horizontal = { width: 1280, height: 240 };
  const vertical = { width: 330, height: 900 };
  const cases = {
    bottom: { viewport: horizontal, panel: { x: 360, y: 168, width: 561, height: 64 } },
    top: { viewport: horizontal, panel: { x: 360, y: 8, width: 561, height: 64 } },
    left: { viewport: vertical, panel: { x: 8, y: 170, width: 64, height: 561 } },
    right: { viewport: vertical, panel: { x: 258, y: 170, width: 64, height: 561 } },
  } as const;

  it("the strip lies along the window's edge side, as long as the panel and in line with it", () => {
    expect(revealStrip(cases.bottom.panel, horizontal, 'bottom')).toEqual({
      x: 360,
      y: 238,
      width: 561,
      height: 2,
    });
    expect(revealStrip(cases.top.panel, horizontal, 'top')).toEqual({
      x: 360,
      y: 0,
      width: 561,
      height: 2,
    });
    expect(revealStrip(cases.left.panel, vertical, 'left')).toEqual({
      x: 0,
      y: 170,
      width: 2,
      height: 561,
    });
    expect(revealStrip(cases.right.panel, vertical, 'right')).toEqual({
      x: 328,
      y: 170,
      width: 2,
      height: 561,
    });
  });

  it('hidden, the strip is both rectangles; shown, the envelope reaches out to the edge side', () => {
    for (const edge of DOCK_EDGES) {
      const { viewport, panel } = cases[edge];
      const region = hitRegionFor(panel, 117.6, 36.8, edge);
      const strip = revealStrip(panel, viewport, edge);
      expect(hiddenHitRegion(panel, viewport, edge)).toEqual({ panel: strip, envelope: strip });
      const shown = shownHitRegion(region, viewport, edge);
      expect(shown.panel).toEqual(region.panel);
      expect(rectContains(shown.envelope, region.envelope)).toBe(true);
      expect(rectContains(shown.envelope, strip)).toBe(true);
      // Hidden by `always`: the strip; above a visible taskbar in fullscreen alone: nothing.
      expect(
        autoHideHitRegion(always(250).advance(250).state, region, viewport, true, edge),
      ).toEqual({ panel: strip, envelope: strip });
      const fullscreen = new Harness()
        .send({ type: 'configure', mode: 'off', delayMs: 0 })
        .send({ type: 'fullscreen', active: true });
      expect(autoHideHitRegion(fullscreen.state, region, viewport, false, edge)).toEqual(
        NO_HIT_REGION,
      );
    }
    // The grown envelope of each edge: the panel's envelope out to that side.
    expect(
      shownHitRegion(hitRegionFor(cases.left.panel, 0, 0, 'left'), vertical, 'left').envelope,
    ).toEqual({ x: 0, y: 170, width: 72, height: 561 });
    expect(
      shownHitRegion(hitRegionFor(cases.right.panel, 0, 0, 'right'), vertical, 'right').envelope,
    ).toEqual({ x: 258, y: 170, width: 72, height: 561 });
    expect(
      shownHitRegion(hitRegionFor(cases.top.panel, 0, 0, 'top'), horizontal, 'top').envelope,
    ).toEqual({ x: 360, y: 0, width: 561, height: 72 });
  });

  it('counts the band between the panel and the edge side as the dock only with the strip there', () => {
    const inBand = {
      bottom: [500, 236],
      top: [500, 3],
      left: [3, 400],
      right: [327, 400],
    } as const;
    const beyondPanel = {
      bottom: [500, 100],
      top: [500, 120],
      left: [120, 400],
      right: [200, 400],
    } as const;
    for (const edge of DOCK_EDGES) {
      const { viewport, panel } = cases[edge];
      const [bx, by] = inBand[edge];
      expect(onDockAt(panel, bx, by, viewport, true, edge)).toBe(true);
      expect(onDockAt(panel, bx, by, viewport, false, edge)).toBe(false);
      const [ox, oy] = beyondPanel[edge];
      expect(onDockAt(panel, ox, oy, viewport, true, edge)).toBe(false);
      // The panel itself, whatever the strip.
      const cx = panel.x + panel.width / 2;
      const cy = panel.y + panel.height / 2;
      expect(onDockAt(panel, cx, cy, viewport, true, edge)).toBe(true);
      expect(onDockAt(panel, cx, cy, viewport, false, edge)).toBe(true);
    }
    // Beside the band along the edge: never.
    expect(onDockAt(cases.left.panel, 3, 169, vertical, true, 'left')).toBe(false);
    expect(onDockAt(cases.left.panel, 3, 731, vertical, true, 'left')).toBe(false);
  });
});

describe('the reveal strip', () => {
  /** The mock's demo monitor: 2560 × 1440, the stock taskbar's 48 px at the bottom. */
  function monitor(
    workArea: MonitorInfo['workArea'],
    bounds = { x: 0, y: 0, width: 2560, height: 1440 },
  ): MonitorInfo {
    return { id: 'm', name: 'm', bounds, workArea, scaleFactor: 1, isPrimary: true };
  }
  const off = (): Harness => new Harness().send({ type: 'configure', mode: 'off', delayMs: 250 });

  it('the work area reaches the bottom unless a visible taskbar holds the space below it', () => {
    const reaches = (m: MonitorInfo | null | undefined): boolean =>
      workAreaReachesEdge(m, 'bottom');
    expect(reaches(monitor({ x: 0, y: 0, width: 2560, height: 1392 }))).toBe(false);
    // Auto-hidden (Windows' own setting, or Stem's): the whole monitor.
    expect(reaches(monitor({ x: 0, y: 0, width: 2560, height: 1440 }))).toBe(true);
    // The taskbar at the top or on a side.
    expect(reaches(monitor({ x: 0, y: 48, width: 2560, height: 1392 }))).toBe(true);
    expect(reaches(monitor({ x: 62, y: 0, width: 2498, height: 1440 }))).toBe(true);
    // A monitor above the primary, in negative coordinates.
    const above = { x: 0, y: -1080, width: 1920, height: 1080 };
    expect(reaches(monitor({ x: 0, y: -1080, width: 1920, height: 1032 }, above))).toBe(false);
    expect(reaches(monitor({ ...above }, above))).toBe(true);
    // Not known yet: the strip as it always was.
    expect(reaches(undefined)).toBe(true);
    expect(reaches(null)).toBe(true);
  });

  it('on each edge the work area reaches the screen unless an app bar holds that side', () => {
    const bottomBar = monitor({ x: 0, y: 0, width: 2560, height: 1392 });
    const topBar = monitor({ x: 0, y: 48, width: 2560, height: 1392 });
    const leftBar = monitor({ x: 62, y: 0, width: 2498, height: 1440 });
    const rightBar = monitor({ x: 0, y: 0, width: 2498, height: 1440 });
    const none = monitor({ x: 0, y: 0, width: 2560, height: 1440 });
    const expected = {
      bottom: [false, true, true, true, true],
      top: [true, false, true, true, true],
      left: [true, true, false, true, true],
      right: [true, true, true, false, true],
    } as const;
    for (const edge of DOCK_EDGES) {
      expect(
        [bottomBar, topBar, leftBar, rightBar, none].map((m) => workAreaReachesEdge(m, edge)),
      ).toEqual(expected[edge]);
      expect(workAreaReachesEdge(undefined, edge)).toBe(true);
    }
    // A monitor left of the primary, in negative coordinates, its taskbar on its right.
    const west = { x: -1920, y: 0, width: 1920, height: 1080 };
    expect(
      workAreaReachesEdge(monitor({ x: -1920, y: 0, width: 1858, height: 1080 }, west), 'right'),
    ).toBe(false);
    expect(
      workAreaReachesEdge(monitor({ x: -1920, y: 0, width: 1858, height: 1080 }, west), 'left'),
    ).toBe(true);
  });

  it('only fullscreen hides when the mode would not; the fullscreen window is itself the overlap', () => {
    expect(onlyFullscreenHides(INITIAL_AUTO_HIDE)).toBe(false);
    expect(onlyFullscreenHides(off().send({ type: 'fullscreen', active: true }).state)).toBe(true);
    expect(onlyFullscreenHides(always().send({ type: 'fullscreen', active: true }).state)).toBe(
      false,
    );
    const dodging = new Harness()
      .send({ type: 'configure', mode: 'whenOverlapped', delayMs: 250 })
      .send({ type: 'fullscreen', active: true });
    expect(onlyFullscreenHides(dodging.state)).toBe(true);
    // Under whenOverlapped the fullscreen window covers the dock too: that
    // overlap is the fullscreen app itself, so fullscreen alone still hides.
    dodging.send({ type: 'overlapped', active: true });
    expect(onlyFullscreenHides(dodging.state)).toBe(true);
    // An overlap with another mode does not count.
    const overlappedOff = off()
      .send({ type: 'overlapped', active: true })
      .send({ type: 'fullscreen', active: true });
    expect(onlyFullscreenHides(overlappedOff.state)).toBe(true);
  });

  it('is there whenever auto-hide applies, except for fullscreen alone above a visible taskbar', () => {
    for (const edge of [true, false]) {
      expect(revealStripOn(INITIAL_AUTO_HIDE, edge)).toBe(false);
      expect(revealStripOn(always().state, edge)).toBe(true);
      expect(revealStripOn(always().send({ type: 'fullscreen', active: true }).state, edge)).toBe(
        true,
      );
      // Under whenOverlapped the fullscreen window is itself the overlap:
      // it does not bring the strip back above a visible taskbar.
      const overlapped = new Harness()
        .send({ type: 'configure', mode: 'whenOverlapped', delayMs: 250 })
        .send({ type: 'overlapped', active: true })
        .send({ type: 'fullscreen', active: true });
      expect(revealStripOn(overlapped.state, edge)).toBe(edge);
    }
    const fullscreen = off().send({ type: 'fullscreen', active: true }).state;
    expect(revealStripOn(fullscreen, true)).toBe(true);
    expect(revealStripOn(fullscreen, false)).toBe(false);
  });

  it('is 2 px thick while hidden and fills the margin beside the panel on screen', () => {
    expect(revealStripThickness('hidden', 8)).toBe(REVEAL_STRIP_PX);
    expect(revealStripThickness('shown', 8)).toBe(8);
    expect(revealStripThickness('pending', 7.5)).toBe(7.5);
    // Never thinner than the strip itself.
    expect(revealStripThickness('shown', 0)).toBe(REVEAL_STRIP_PX);
  });

  it('counts the band below the panel as the dock after a drop only with the strip there', () => {
    // A 400 x 60 panel 8 px above the bottom of a 100 px viewport.
    const panel = { x: 100, y: 32, width: 400, height: 60 };
    const viewport = { width: 600, height: 100 };
    const at = (x: number, y: number, stripOn: boolean): boolean =>
      onDockAt(panel, x, y, viewport, stripOn, 'bottom');
    for (const stripOn of [true, false]) {
      // Over the panel, whatever the strip.
      expect(at(300, 60, stripOn)).toBe(true);
      expect(at(100, 32, stripOn)).toBe(true);
      expect(at(499, 91, stripOn)).toBe(true);
      // Beside it, above it, below the viewport: never.
      expect(at(99, 60, stripOn)).toBe(false);
      expect(at(500, 60, stripOn)).toBe(false);
      expect(at(300, 31, stripOn)).toBe(false);
      expect(at(300, 100, stripOn)).toBe(false);
      // Beside the band.
      expect(at(99, 95, stripOn)).toBe(false);
      expect(at(500, 95, stripOn)).toBe(false);
    }
    // The band from the panel's bottom edge down to the viewport's.
    for (const y of [92, 95, 99]) {
      expect(at(300, y, true)).toBe(true);
      expect(at(300, y, false)).toBe(false);
    }
  });

  it('has the strip for a drop in fullscreen at the screen edge and none above a visible taskbar', () => {
    const panel = { x: 100, y: 32, width: 400, height: 60 };
    const viewport = { width: 600, height: 100 };
    const at = (stripOn: boolean): boolean => onDockAt(panel, 300, 95, viewport, stripOn, 'bottom');
    const fullscreen = off().send({ type: 'fullscreen', active: true }).send({ type: 'show' });
    expect(at(revealStripOn(fullscreen.state, true))).toBe(true);
    expect(at(revealStripOn(fullscreen.state, false))).toBe(false);
    // `always` keeps its strip on the work area's edge, taskbar or not.
    const pinned = always().send({ type: 'fullscreen', active: true });
    expect(at(revealStripOn(pinned.state, false))).toBe(true);
    // Auto-hide off: no strip, so the band is not the dock.
    expect(at(revealStripOn(INITIAL_AUTO_HIDE, true))).toBe(false);
  });

  it('above a visible taskbar the hidden dock comes back only on request or for attention', () => {
    const h = off().send({ type: 'fullscreen', active: true });
    expect(h.phase).toBe('hidden');
    expect(revealStripOn(h.state, false)).toBe(false);
    h.send({ type: 'show' });
    expect(h.phase).toBe('shown');
    h.send({ type: 'pointer', inside: true }).send({ type: 'pointer', inside: false }).advance(0);
    expect(h.phase).toBe('hidden');
    h.send({ type: 'hold', hold: 'attention', active: true });
    expect(h.phase).toBe('shown');
  });
});

describe('the pointer is forgotten when auto-hide stops applying', () => {
  const off = (): Harness => new Harness().send({ type: 'configure', mode: 'off', delayMs: 250 });

  it('shown on request during fullscreen, the next fullscreen still hides it at once', () => {
    const h = off().send({ type: 'fullscreen', active: true });
    expect(h.phase).toBe('hidden');
    h.send({ type: 'show' });
    expect(h.phase).toBe('shown');
    expect(h.state.pointerInside).toBe(true);
    // Fullscreen ends before the pointer ever came.
    h.send({ type: 'fullscreen', active: false });
    expect(autoHideActive(h.state)).toBe(false);
    expect(h.state.pointerInside).toBe(false);
    h.send({ type: 'fullscreen', active: true });
    expect(h.phase).toBe('hidden');
    expect(h.timers.at(-1)).toBe('none');
  });

  it('revealed from the strip, which goes when fullscreen ends, the next fullscreen still hides it', () => {
    const h = off().send({ type: 'fullscreen', active: true }).send({ type: 'strip' });
    expect(h.state.pointerInside).toBe(true);
    // The strip leaves the DOM under the pointer: no `pointerleave` comes.
    h.send({ type: 'fullscreen', active: false });
    expect(h.state.pointerInside).toBe(false);
    h.send({ type: 'fullscreen', active: true });
    expect(h.phase).toBe('hidden');
  });

  it('the mode switched off and an overlap ending forget it too', () => {
    const configured = always().send({ type: 'strip' });
    configured.send({ type: 'configure', mode: 'off', delayMs: 250 });
    expect(configured.state.pointerInside).toBe(false);
    expect(configured.phase).toBe('shown');

    const overlapped = new Harness()
      .send({ type: 'configure', mode: 'whenOverlapped', delayMs: 250 })
      .send({ type: 'overlapped', active: true })
      .advance(250);
    expect(overlapped.phase).toBe('hidden');
    overlapped.send({ type: 'show' });
    expect(overlapped.state.pointerInside).toBe(true);
    overlapped.send({ type: 'overlapped', active: false });
    expect(overlapped.state.pointerInside).toBe(false);
    overlapped.send({ type: 'overlapped', active: true });
    expect(overlapped.phase).toBe('pending');
    overlapped.advance(250);
    expect(overlapped.phase).toBe('hidden');
  });

  it('is kept while auto-hide still applies', () => {
    // `always` outlives fullscreen: the pointer on the strip stays inside.
    const h = always(250).advance(250).send({ type: 'fullscreen', active: true });
    h.send({ type: 'strip' }).send({ type: 'fullscreen', active: false });
    expect(autoHideActive(h.state)).toBe(true);
    expect(h.state.pointerInside).toBe(true);
    expect(h.phase).toBe('shown');
    // An overlap that ends during fullscreen.
    const g = new Harness()
      .send({ type: 'configure', mode: 'whenOverlapped', delayMs: 250 })
      .send({ type: 'overlapped', active: true })
      .send({ type: 'fullscreen', active: true })
      .send({ type: 'strip' })
      .send({ type: 'overlapped', active: false });
    expect(g.state.pointerInside).toBe(true);
  });

  it('a pointer reported over the panel again counts, and keeps the next fullscreen from hiding it', () => {
    // What `auto-hide.svelte.ts` and `Dock.svelte` send from the panel's
    // `:hover` (or its next `pointermove`) once the machine has forgotten the
    // pointer.
    const h = off()
      .send({ type: 'pointer', inside: true })
      .send({ type: 'fullscreen', active: true });
    expect(h.phase).toBe('shown');
    h.send({ type: 'fullscreen', active: false });
    expect(h.state.pointerInside).toBe(false);
    h.send({ type: 'pointer', inside: true });
    expect(h.state.pointerInside).toBe(true);
    h.send({ type: 'fullscreen', active: true });
    expect(h.phase).toBe('shown');
    h.send({ type: 'pointer', inside: false }).advance(0);
    expect(h.phase).toBe('hidden');
  });

  it('leaves a state with the pointer away as it is', () => {
    const h = off().send({ type: 'fullscreen', active: true });
    const hidden = h.state;
    const step = autoHideStep(hidden, { type: 'fullscreen', active: false });
    expect(step.state.pointerInside).toBe(false);
    expect(step.state.phase).toBe('shown');
    expect(hidden.phase).toBe('hidden');
  });
});
