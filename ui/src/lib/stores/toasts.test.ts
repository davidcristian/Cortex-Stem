import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { describeError, Toasts } from './toasts.svelte';

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

const messages = (t: Toasts): string[] => t.items.map((i) => i.message);

describe('Toasts', () => {
  it('shows a message and drops it after the time to live', () => {
    const toasts = new Toasts(4000);
    toasts.push('launch failed');
    expect(messages(toasts)).toEqual(['launch failed']);
    vi.advanceTimersByTime(3999);
    expect(messages(toasts)).toEqual(['launch failed']);
    vi.advanceTimersByTime(1);
    expect(messages(toasts)).toEqual([]);
  });

  it('dismisses early and ignores a second dismiss', () => {
    const toasts = new Toasts(4000);
    const id = toasts.push('a');
    toasts.push('b');
    toasts.dismiss(id);
    expect(messages(toasts)).toEqual(['b']);
    toasts.dismiss(id);
    expect(messages(toasts)).toEqual(['b']);
    vi.advanceTimersByTime(4000);
    expect(messages(toasts)).toEqual([]);
  });

  it('keeps only the newest `max` entries', () => {
    const toasts = new Toasts(4000, 2);
    toasts.push('1');
    toasts.push('2');
    toasts.push('3');
    expect(messages(toasts)).toEqual(['2', '3']);
    // The dropped entry's timer is gone too: nothing odd happens when it would have fired.
    vi.advanceTimersByTime(4000);
    expect(messages(toasts)).toEqual([]);
    expect(vi.getTimerCount()).toBe(0);
  });

  it('clear() empties the queue and cancels every timer', () => {
    const toasts = new Toasts(4000);
    toasts.push('x');
    toasts.push('y');
    toasts.clear();
    expect(messages(toasts)).toEqual([]);
    expect(vi.getTimerCount()).toBe(0);
  });

  it('replaces the array instead of mutating it', () => {
    const toasts = new Toasts(4000);
    const before = toasts.items;
    toasts.push('x');
    expect(toasts.items).not.toBe(before);
    expect(before).toEqual([]);
  });
});

describe('describeError', () => {
  it('passes IPC strings through and unwraps Errors', () => {
    expect(describeError('window not found: 4')).toBe('window not found: 4');
    expect(describeError(new Error('nope'))).toBe('nope');
    expect(describeError(42)).toBe('42');
  });
});
