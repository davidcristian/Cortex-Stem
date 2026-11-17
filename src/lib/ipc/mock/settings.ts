/**
 * `Settings` helpers mirrored from stem-core (settings.rs and settings/):
 * defaults, validation, pin order.
 */

import type { AppId } from '$lib/ipc/generated/AppId';
import type { PinnedApp } from '$lib/ipc/generated/PinnedApp';
import type { Settings } from '$lib/ipc/generated/Settings';

/** `Settings::default()`. */
export function defaultSettings(): Settings {
  return {
    version: 1,
    position: 'bottom',
    iconSize: 48,
    magnification: { enabled: true, maxScale: 1.6, rangeIcons: 3 },
    autoHide: { mode: 'off', delayMs: 250 },
    monitors: 'primary',
    theme: 'system',
    appearance: {
      theme: 'standard',
      opacity: 0.72,
      cornerRadius: 22,
      backdrop: 'css',
      tooltips: true,
      bounceOnLaunch: true,
    },
    items: {
      launcher: true,
      taskView: false,
      showDesktop: false,
      quickSettings: false,
      notificationCenter: false,
      volume: false,
      battery: false,
      clock: false,
      recycleBin: true,
      separators: true,
    },
    behavior: {
      clickActiveMinimizes: true,
      showOnlyCurrentDesktop: true,
      showOnlyCurrentMonitor: false,
      runningIndicators: true,
      clickThrough: true,
      toggleShortcut: 'Ctrl+Alt+D',
      focusShortcut: 'Ctrl+Alt+Shift+D',
      scrollAction: 'none',
    },
    pinned: [],
    hiddenApps: [],
    system: {
      autostart: false,
      hideWindowsTaskbar: false,
      reserveScreenSpace: false,
      fileLogging: true,
    },
  };
}

export const clamp = (v: number, lo: number, hi: number): number => Math.min(hi, Math.max(lo, v));

/** `finite_or` in settings/document.rs: `value` unless it is NaN or ±Infinity, then `fallback`. */
const finiteOr = (value: number, fallback: number): number =>
  Number.isFinite(value) ? value : fallback;

/**
 * `Settings::validate`: replace a non-finite float (`maxScale`, `opacity`,
 * the two Rust passes through `finite_or`) with its default, clamp numeric
 * fields, drop empty/duplicate pins, and drop hidden apps that are pinned,
 * have a blank id or repeat one. Returns a copy.
 */
export function validateSettings(input: Settings): Settings {
  const s: Settings = structuredClone(input);
  const defaults = defaultSettings();
  s.version = 1;
  s.iconSize = clamp(Math.round(s.iconSize), 24, 128);
  s.magnification.maxScale = clamp(
    finiteOr(s.magnification.maxScale, defaults.magnification.maxScale),
    1,
    2.5,
  );
  s.magnification.rangeIcons = clamp(Math.round(s.magnification.rangeIcons), 1, 6);
  s.autoHide.delayMs = clamp(Math.round(s.autoHide.delayMs), 0, 5000);
  s.appearance.opacity = clamp(finiteOr(s.appearance.opacity, defaults.appearance.opacity), 0, 1);
  s.appearance.cornerRadius = clamp(Math.round(s.appearance.cornerRadius), 0, 64);
  // A document written before an item existed (the volume item, v1.30) has
  // no flag for it: serde's `default` on `FixedItems` gives each missing
  // one its default.
  const items = defaults.items;
  for (const key of Object.keys(items) as Array<keyof typeof items>) {
    if (typeof s.items[key] !== 'boolean') {
      s.items[key] = items[key];
    }
  }
  const seen = new Set<AppId>();
  s.pinned = s.pinned.filter((p) => {
    if (p.id === '' || seen.has(p.id)) {
      return false;
    }
    seen.add(p.id);
    return true;
  });
  // After the pins, which win: a pinned app is never hidden.
  const hidden = new Set<AppId>();
  s.hiddenApps = s.hiddenApps.filter((h) => {
    if (h.id.trim() === '' || seen.has(h.id) || hidden.has(h.id)) {
      return false;
    }
    hidden.add(h.id);
    return true;
  });
  return s;
}

/** `Settings::reorder_pinned`: listed ids first, the rest keep their order. */
export function reorderPinned(pinned: PinnedApp[], order: AppId[]): PinnedApp[] {
  const remaining = [...pinned];
  const result: PinnedApp[] = [];
  for (const id of order) {
    const pos = remaining.findIndex((p) => p.id === id);
    if (pos !== -1) {
      result.push(...remaining.splice(pos, 1));
    }
  }
  return result.concat(remaining);
}
