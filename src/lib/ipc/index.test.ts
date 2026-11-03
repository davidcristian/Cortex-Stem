import { describe, expect, it } from 'vitest';

import { chooseBackend } from './index';

describe('chooseBackend', () => {
  it('uses Tauri when its internals exist and nothing forces the mock', () => {
    expect(chooseBackend('', true)).toEqual({ kind: 'tauri' });
    expect(chooseBackend('?monitor=abc', true)).toEqual({ kind: 'tauri' });
  });

  it('falls back to the mock in a plain browser', () => {
    expect(chooseBackend('', false)).toEqual({ kind: 'mock', scenario: 'default' });
  });

  it('lets ?mock force the mock, and ?mock=many pick the large scenario', () => {
    expect(chooseBackend('?mock=1', true)).toEqual({ kind: 'mock', scenario: 'default' });
    expect(chooseBackend('?mock=many', true)).toEqual({ kind: 'mock', scenario: 'many' });
    expect(chooseBackend('?mock=many&x=1', false)).toEqual({ kind: 'mock', scenario: 'many' });
  });
});
