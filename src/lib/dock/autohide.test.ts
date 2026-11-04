import { describe, expect, it } from 'vitest';

import {
  AUTO_HIDE_SLIDE_MS,
  autoHideStep,
  hiddenHitRegion,
  INITIAL_AUTO_HIDE,
  isAutoHidden,
  REVEAL_STRIP_PX,
  revealStrip,
  shownHitRegion,
  type AutoHideEvent,
  type AutoHideState,
  type TimerCommand,
} from './autohide';
import type { HitRegion } from './geometry';

/**
 * Drives the machine like `Dock.svelte` does: one timer, armed and cleared
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
      this.deadline = this.now + step.state.delayMs;
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

  it('never hides with auto-hide off or (for now) when overlapped', () => {
    for (const mode of ['off', 'whenOverlapped'] as const) {
      const h = new Harness()
        .send({ type: 'configure', mode, delayMs: 250 })
        .send({ type: 'pointer', inside: true })
        .send({ type: 'pointer', inside: false })
        .advance(10_000);
      expect(h.phase).toBe('shown');
      expect(h.state.enabled).toBe(false);
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

  it.each(['drag', 'menu', 'bounce'] as const)('never hides during a %s', (hold) => {
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

  it('hides at once with delayMs 0', () => {
    const h = always(0);
    expect(h.phase).toBe('hidden');
    expect(h.timers).toEqual(['none']);
    h.send({ type: 'strip' });
    expect(h.phase).toBe('shown');
    h.send({ type: 'pointer', inside: false });
    expect(h.phase).toBe('hidden');
    expect(h.timers.at(-1)).toBe('none');
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

  it('a delay change to 0 during the countdown hides at once', () => {
    const h = always(1000).advance(10);
    h.send({ type: 'configure', mode: 'always', delayMs: 0 });
    expect(h.phase).toBe('hidden');
    expect(h.timers.at(-1)).toBe('clear');
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
    autoHideStep(before, { type: 'pointer', inside: true });
    autoHideStep(before, { type: 'configure', mode: 'off', delayMs: 0 });
    expect(before).toEqual(copy);
  });

  it('slides for 200 ms over a 2 px strip (ADR-0008)', () => {
    expect(AUTO_HIDE_SLIDE_MS).toBe(200);
    expect(REVEAL_STRIP_PX).toBe(2);
  });
});

describe('hit regions while auto-hide is on', () => {
  // A 559 × 64 panel resting 8 px above the bottom of a 1280 × 240 viewport.
  const panel = { x: 360, y: 168, width: 561, height: 64 };
  const region: HitRegion = {
    panel,
    envelope: { x: 278, y: 147, width: 725, height: 85 },
  };

  it('the strip is the bottom 2 px, exactly as wide as the panel and in line with it', () => {
    expect(revealStrip(panel, 240)).toEqual({ x: 360, y: 238, width: 561, height: 2 });
  });

  it('the strip never reaches above a tiny viewport', () => {
    expect(revealStrip(panel, 1)).toEqual({ x: 360, y: 0, width: 561, height: 1 });
    expect(revealStrip(panel, 0)).toEqual({ x: 360, y: 0, width: 561, height: 0 });
  });

  it('while hidden the strip is both rectangles', () => {
    const strip = { x: 360, y: 238, width: 561, height: 2 };
    expect(hiddenHitRegion(panel, 240)).toEqual({ panel: strip, envelope: strip });
  });

  it('while shown the envelope takes in the strip and the panel is unchanged', () => {
    expect(shownHitRegion(region, 240)).toEqual({
      panel,
      envelope: { x: 278, y: 147, width: 725, height: 93 },
    });
  });

  it('an envelope that already reaches the bottom (a drag) is unchanged', () => {
    const whole: HitRegion = { panel, envelope: { x: 0, y: 0, width: 1280, height: 240 } };
    expect(shownHitRegion(whole, 240)).toEqual(whole);
  });
});
