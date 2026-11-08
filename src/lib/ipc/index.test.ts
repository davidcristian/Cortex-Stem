import { describe, expect, it } from 'vitest';

import { chooseBackend } from './index';

describe('chooseBackend', () => {
  it('uses Tauri when its internals exist and nothing forces the mock', () => {
    expect(chooseBackend('', true)).toEqual({ kind: 'tauri' });
    expect(chooseBackend('?monitor=abc', true)).toEqual({ kind: 'tauri' });
  });

  it('falls back to the mock in a plain browser', () => {
    expect(chooseBackend('', false)).toEqual({ kind: 'mock', scenario: 'default', monitor: null });
  });

  it('lets ?mock force the mock, and ?mock=many pick the large scenario', () => {
    expect(chooseBackend('?mock=1', true)).toEqual({
      kind: 'mock',
      scenario: 'default',
      monitor: null,
    });
    expect(chooseBackend('?mock=many', true)).toEqual({
      kind: 'mock',
      scenario: 'many',
      monitor: null,
    });
    expect(chooseBackend('?mock=many&x=1', false)).toEqual({
      kind: 'mock',
      scenario: 'many',
      monitor: null,
    });
  });

  it('takes ?scenario= by name and the page’s ?monitor= for the mock', () => {
    expect(chooseBackend('?mock=1&scenario=twoMonitors', true)).toEqual({
      kind: 'mock',
      scenario: 'twoMonitors',
      monitor: null,
    });
    expect(chooseBackend('?mock=1&scenario=twoMonitors&monitor=%5C%5C.%5CDISPLAY3', true)).toEqual({
      kind: 'mock',
      scenario: 'twoMonitors',
      monitor: '\\\\.\\DISPLAY3',
    });
    // An unknown scenario name falls back; `mock=many` still wins over nothing.
    expect(chooseBackend('?mock=many&scenario=nope', false)).toEqual({
      kind: 'mock',
      scenario: 'many',
      monitor: null,
    });
    expect(chooseBackend('?scenario=twoMonitors&monitor=', false)).toEqual({
      kind: 'mock',
      scenario: 'twoMonitors',
      monitor: null,
    });
  });
});
