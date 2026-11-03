import type { ThemeInfo } from '$lib/ipc/generated/ThemeInfo';
import type { ThemeMode } from '$lib/ipc/generated/ThemeMode';

export type ResolvedTheme = 'dark' | 'light';

/**
 * Which colour scheme the dock renders with.
 *
 * The user's explicit choice wins; `system` follows the OS facts from
 * `dock://theme` and, until those arrived, the browser's
 * `prefers-color-scheme`.
 */
export function resolveTheme(
  mode: ThemeMode,
  os: ThemeInfo | null,
  prefersDark: boolean,
): ResolvedTheme {
  switch (mode) {
    case 'dark':
      return 'dark';
    case 'light':
      return 'light';
    case 'system':
      return (os ? os.dark : prefersDark) ? 'dark' : 'light';
  }
}

/**
 * Panel opacity after OS accessibility facts: high contrast means opaque,
 * "transparency effects off" means nearly opaque, otherwise the user's value.
 */
export function effectiveOpacity(opacity: number, os: ThemeInfo | null): number {
  if (os?.highContrast) {
    return 1;
  }
  const clamped = Math.min(1, Math.max(0, opacity));
  if (os && !os.transparency) {
    return Math.max(clamped, 0.95);
  }
  return clamped;
}

/** Default accent until `dock://theme` says otherwise (Windows blue). */
export const DEFAULT_ACCENT = '#0078D4';

/** `#RRGGBB` only; anything else falls back to the default accent. */
export function accentColor(os: ThemeInfo | null): string {
  const accent = os?.accent ?? DEFAULT_ACCENT;
  return /^#[0-9a-fA-F]{6}$/.test(accent) ? accent : DEFAULT_ACCENT;
}
