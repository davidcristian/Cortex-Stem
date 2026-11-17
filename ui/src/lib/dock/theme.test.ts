import { describe, expect, it } from 'vitest';

import type { ThemeInfo } from '$lib/ipc/generated/ThemeInfo';

import { accentColor, DEFAULT_ACCENT, effectiveOpacity, resolveTheme } from './theme';

const os = (overrides: Partial<ThemeInfo> = {}): ThemeInfo => ({
  accent: '#0078D4',
  dark: true,
  highContrast: false,
  transparency: true,
  ...overrides,
});

describe('resolveTheme', () => {
  it('honours an explicit light/dark setting over everything else', () => {
    expect(resolveTheme('dark', os({ dark: false }), false)).toBe('dark');
    expect(resolveTheme('light', os({ dark: true }), true)).toBe('light');
  });

  it('follows the OS flag when the setting is "system"', () => {
    expect(resolveTheme('system', os({ dark: true }), false)).toBe('dark');
    expect(resolveTheme('system', os({ dark: false }), true)).toBe('light');
  });

  it('falls back to prefers-color-scheme until the OS theme arrived', () => {
    expect(resolveTheme('system', null, true)).toBe('dark');
    expect(resolveTheme('system', null, false)).toBe('light');
  });
});

describe('effectiveOpacity', () => {
  it('passes the user value through when transparency effects are on', () => {
    expect(effectiveOpacity(0.72, os())).toBe(0.72);
    expect(effectiveOpacity(0.72, null)).toBe(0.72);
  });

  it('clamps to 0..1', () => {
    expect(effectiveOpacity(1.5, null)).toBe(1);
    expect(effectiveOpacity(-1, null)).toBe(0);
  });

  it('is opaque in high contrast and nearly opaque without transparency effects', () => {
    expect(effectiveOpacity(0.3, os({ highContrast: true }))).toBe(1);
    expect(effectiveOpacity(0.3, os({ transparency: false }))).toBe(0.95);
    expect(effectiveOpacity(1, os({ transparency: false }))).toBe(1);
  });
});

describe('accentColor', () => {
  it('uses the OS accent when it is a #RRGGBB value', () => {
    expect(accentColor(os({ accent: '#ff8800' }))).toBe('#ff8800');
  });

  it('falls back to the default for missing or malformed values', () => {
    expect(accentColor(null)).toBe(DEFAULT_ACCENT);
    expect(accentColor(os({ accent: 'red' }))).toBe(DEFAULT_ACCENT);
    expect(accentColor(os({ accent: '#fff' }))).toBe(DEFAULT_ACCENT);
  });
});
