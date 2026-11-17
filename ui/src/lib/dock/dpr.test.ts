import { describe, expect, it, vi } from 'vitest';

import {
  normalizeRatio,
  ratioQuery,
  watchDevicePixelRatio,
  type RatioQuery,
  type RatioSource,
} from './dpr';

interface FakeQuery extends RatioQuery {
  readonly query: string;
  readonly listeners: Set<() => void>;
}

/** A window whose ratio the test moves; a change fires every armed query, like the browser does. */
class FakeSource implements RatioSource {
  devicePixelRatio = 1;
  readonly queries: FakeQuery[] = [];

  matchMedia(query: string): RatioQuery {
    const listeners = new Set<() => void>();
    const q: FakeQuery = {
      query,
      listeners,
      addEventListener: (_type, listener) => {
        listeners.add(listener);
      },
      removeEventListener: (_type, listener) => {
        listeners.delete(listener);
      },
    };
    this.queries.push(q);
    return q;
  }

  /** The queries that still have a listener. */
  get armed(): string[] {
    return this.queries.filter((q) => q.listeners.size > 0).map((q) => q.query);
  }

  /**
   * The ratio becomes `ratio`: the queries on any other value stop matching
   * and fire (the ones existing now; a listener re-arming on the new value
   * adds a query the browser would not fire for this change).
   */
  change(ratio: number): void {
    this.devicePixelRatio = ratio;
    for (const q of [...this.queries]) {
      if (q.query !== ratioQuery(ratio)) {
        for (const listener of [...q.listeners]) {
          listener();
        }
      }
    }
  }
}

describe('normalizeRatio', () => {
  it('keeps a finite positive ratio and falls back to 1 for anything else', () => {
    expect(normalizeRatio(1)).toBe(1);
    expect(normalizeRatio(1.25)).toBe(1.25);
    expect(normalizeRatio(2)).toBe(2);
    expect(normalizeRatio(0)).toBe(1);
    expect(normalizeRatio(-1.5)).toBe(1);
    expect(normalizeRatio(NaN)).toBe(1);
    expect(normalizeRatio(Infinity)).toBe(1);
    expect(normalizeRatio(undefined)).toBe(1);
  });
});

describe('ratioQuery', () => {
  it('matches exactly one ratio in dppx', () => {
    expect(ratioQuery(1)).toBe('(resolution: 1dppx)');
    expect(ratioQuery(1.5)).toBe('(resolution: 1.5dppx)');
    expect(ratioQuery(1.25)).toBe('(resolution: 1.25dppx)');
  });
});

describe('watchDevicePixelRatio', () => {
  it('reports the current ratio at once and arms a query on it', () => {
    const source = new FakeSource();
    source.devicePixelRatio = 1.5;
    const onChange = vi.fn();
    watchDevicePixelRatio(source, onChange);
    expect(onChange.mock.calls).toEqual([[1.5]]);
    expect(source.armed).toEqual(['(resolution: 1.5dppx)']);
  });

  it('reports a change once and re-arms on the new ratio', () => {
    const source = new FakeSource();
    const onChange = vi.fn();
    watchDevicePixelRatio(source, onChange);
    source.change(2);
    expect(onChange.mock.calls).toEqual([[1], [2]]);
    // The query on 1 is done with; only the one on 2 is listening.
    expect(source.armed).toEqual(['(resolution: 2dppx)']);
    source.change(1.25);
    expect(onChange.mock.calls).toEqual([[1], [2], [1.25]]);
    expect(source.armed).toEqual(['(resolution: 1.25dppx)']);
  });

  it('only re-arms when the ratio is back where it was by the time the query fires', () => {
    const source = new FakeSource();
    const onChange = vi.fn();
    watchDevicePixelRatio(source, onChange);
    // The browser reports the flip after the ratio went 1 → 2 → 1.
    source.devicePixelRatio = 1;
    for (const listener of [...source.queries[0]!.listeners]) {
      listener();
    }
    expect(onChange.mock.calls).toEqual([[1]]);
    expect(source.armed).toEqual(['(resolution: 1dppx)']);
    expect(source.queries).toHaveLength(2);
  });

  it('treats a broken ratio as 1', () => {
    const source = new FakeSource();
    const onChange = vi.fn();
    watchDevicePixelRatio(source, onChange);
    source.change(NaN);
    expect(onChange.mock.calls).toEqual([[1]]);
    expect(source.armed).toEqual(['(resolution: 1dppx)']);
  });

  it('stops: no listener is left and later changes report nothing', () => {
    const source = new FakeSource();
    const onChange = vi.fn();
    const stop = watchDevicePixelRatio(source, onChange);
    source.change(2);
    stop();
    expect(source.armed).toEqual([]);
    source.change(1);
    expect(onChange.mock.calls).toEqual([[1], [2]]);
    // Stopping twice is harmless.
    stop();
  });
});
