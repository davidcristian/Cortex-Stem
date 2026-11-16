/**
 * `MockBackend`: the whole IPC contract (docs/ipc/ v1.30) without Rust, for
 * browser runs and Playwright screenshots.
 *
 * It mirrors, deliberately in this one file so the two sides can be compared,
 * the Rust pieces the UI would otherwise be talking to:
 *
 * 1. the demo scenario of `crates/stem-platform/src/mock.rs` (windows with
 *    their frames, monitor, recycle bin, deterministic icon colours), plus a
 *    second monitor with two windows (`twoMonitors`);
 * 2. the ordering and click rules of `crates/stem-core/src/engine.rs`, the
 *    overlap rule of `crates/stem-core/src/overlap.rs`, and the
 *    per-dock facts of `DockState.docks`;
 * 3. the command semantics and lazy identity resolution of
 *    `crates/stem-service/src/lib.rs`, and the app layer's conversion of the
 *    dock panel to screen coordinates and its registration under the dock
 *    window's label and monitor (`src-tauri/src/windows.rs`); one backend
 *    serves one page, so it knows which dock window it stands for
 *    (`MockOptions.monitor`);
 * 4. which dropped paths go to the Recycle Bin
 *    (`crates/stem-platform/src/rules/recycle.rs`).
 *
 * Behaviour decisions belong to Rust (ADR-0006). Nothing here is used by the
 * UI directly; the UI only ever sees `DockState` snapshots and `Settings`.
 */

import type { Viewport } from '$lib/dock/geometry';
import type { AppFolder } from '$lib/ipc/generated/AppFolder';
import type { AppId } from '$lib/ipc/generated/AppId';
import type { AppIdentity } from '$lib/ipc/generated/AppIdentity';
import type { AppInfo } from '$lib/ipc/generated/AppInfo';
import type { ClockFormat } from '$lib/ipc/generated/ClockFormat';
import type { DockFacts } from '$lib/ipc/generated/DockFacts';
import type { DockItem } from '$lib/ipc/generated/DockItem';
import type { DockItemKind } from '$lib/ipc/generated/DockItemKind';
import type { DockPosition } from '$lib/ipc/generated/DockPosition';
import type { DockState } from '$lib/ipc/generated/DockState';
import type { HiddenApp } from '$lib/ipc/generated/HiddenApp';
import type { IconKey } from '$lib/ipc/generated/IconKey';
import type { LaunchTarget } from '$lib/ipc/generated/LaunchTarget';
import type { MonitorId } from '$lib/ipc/generated/MonitorId';
import type { MonitorInfo } from '$lib/ipc/generated/MonitorInfo';
import type { PinnedApp } from '$lib/ipc/generated/PinnedApp';
import type { PowerStatus } from '$lib/ipc/generated/PowerStatus';
import type { VolumeStatus } from '$lib/ipc/generated/VolumeStatus';
import type { RawWindow } from '$lib/ipc/generated/RawWindow';
import type { Rect } from '$lib/ipc/generated/Rect';
import type { RecycleBinState } from '$lib/ipc/generated/RecycleBinState';
import type { ScrollDirection } from '$lib/ipc/generated/ScrollDirection';
import type { Settings } from '$lib/ipc/generated/Settings';
import type { ShellAction } from '$lib/ipc/generated/ShellAction';
import type { ThemeInfo } from '$lib/ipc/generated/ThemeInfo';
import type { UserCss } from '$lib/ipc/generated/UserCss';
import type { UserCssProblem } from '$lib/ipc/generated/UserCssProblem';
import type { WindowAction } from '$lib/ipc/generated/WindowAction';
import type { WindowId } from '$lib/ipc/generated/WindowId';
import type { WindowInfo } from '$lib/ipc/generated/WindowInfo';

import {
  isShellIconKey,
  type Backend,
  type BackendEvent,
  type BackendEvents,
  type EventHandler,
  type FileDragEvent,
  type FileDragHandler,
  type Unsubscribe,
} from './backend';

// ---------------------------------------------------------------------------
// Options and public helper types
// ---------------------------------------------------------------------------

export type MockScenario = 'default' | 'many' | 'twoMonitors';

export const MOCK_SCENARIOS: readonly MockScenario[] = ['default', 'many', 'twoMonitors'];

export function isMockScenario(value: string | null): value is MockScenario {
  return value !== null && (MOCK_SCENARIOS as readonly string[]).includes(value);
}

export interface MockOptions {
  scenario?: MockScenario;
  /**
   * The monitor of the dock page this backend serves (`dock.html?monitor=
   * <id>`), as the app layer knows it for the window: the page's
   * panel (`setDockPanelRect`) is placed on that monitor's work area at
   * its scale and registered under that dock window's label (`dockLabel`),
   * and `dockPanel()` answers for it. `null` or absent: the primary
   * monitor's page, the `dock` window.
   */
  monitor?: MonitorId | null;
  /**
   * Simulated command latency range in ms. `[0, 0]` settles in a microtask.
   * Commands run in the order they were issued whatever each one draws,
   * like Tauri's IPC, so one may wait a little longer for an earlier one.
   */
  latencyMs?: readonly [number, number];
  /** Delay before app identities (names, icons) resolve. `0` resolves synchronously at construction. */
  identityDelayMs?: number;
  /** Seed of the latency PRNG so runs are reproducible. */
  seed?: number;
  /** Report side effects (launches, shell actions, window sizing) with `console.info`. */
  log?: boolean;
  /**
   * Height of the dock window's viewport in CSS px, for placing the resting
   * panel the UI reports (`setDockPanelRect`) on the mock monitor: like the
   * real window's, the viewport sits flush with its edge of the work area
   * (`panelOnScreen`; on `bottom` its bottom edge is the work area's bottom
   * edge). Default: `window.innerHeight` in a browser, read per call (a
   * window that grows for a drag stays right); the work area's height, i.e.
   * viewport px = screen px, where there is no `window` (unit tests).
   */
  viewportHeight?: number;
  /**
   * Width of the dock window's viewport in CSS px, the same way: on
   * `right` the viewport's right edge is the work area's right edge.
   * Default: `window.innerWidth` in a browser, read per call; the work
   * area's width where there is no `window`.
   */
  viewportWidth?: number;
  /**
   * The edge the scenario's settings put the dock on (`Settings.position`).
   * Absent: the scenario's own (`bottom`). A page in a browser
   * takes it from `?position=` instead (`demo.ts`, `setPosition`).
   */
  position?: DockPosition;
  /**
   * The user's `dock.css` when the backend starts (a `MockUserCssFile`:
   * its text, its bytes, or a file that cannot be read), read then like
   * Rust reads it at start (`readUserCss`: over 64 KiB, or bytes that are
   * not UTF-8, are no CSS); absent or `null`: there is no file.
   * `writeUserCss` changes the file afterwards and `reloadUserCss` reads
   * it again.
   */
  userCss?: MockUserCssFile;
  /**
   * The power reading when the backend starts (`PlatformEvent::PowerChanged`
   * after the start snapshot). Absent: the demo's laptop at 76 %, on
   * battery (`DEMO_POWER`, as `MockPlatform::with_demo_scenario`);
   * `setPower` changes it afterwards.
   */
  power?: PowerStatus;
  /**
   * The regional format when the backend starts
   * (`PlatformEvent::ClockFormatChanged` after the power reading).
   * Absent: nothing known (`locale` and `hour12` both `null`, as
   * `MockPlatform`), so the clock follows the page's own locale;
   * `setClockFormat` changes it afterwards.
   */
  clockFormat?: ClockFormat;
  /**
   * The default output device's volume when the backend starts
   * (`PlatformEvent::VolumeChanged`); `null` for no output device.
   * Absent: the demo's device at 60 %, not muted (`DEMO_VOLUME`, as
   * `MockPlatform::with_demo_scenario`); `setVolume` changes it afterwards,
   * as the volume keys or another device would.
   */
  volume?: VolumeStatus | null;
}

/**
 * The demo's power reading: a laptop at 76 %, on battery; the same
 * as `demo_power` in `crates/stem-platform/src/mock.rs`.
 */
export const DEMO_POWER: Readonly<PowerStatus> = Object.freeze({
  hasBattery: true,
  percent: 76,
  charging: false,
  onAc: false,
  saver: false,
});

/** What the engine knows before the first power reading: nothing. */
const NO_POWER: Readonly<PowerStatus> = Object.freeze({
  hasBattery: false,
  percent: null,
  charging: false,
  onAc: false,
  saver: false,
});

/**
 * The demo's output device: at 60 %, not muted; the same as
 * `demo_volume` in `crates/stem-platform/src/mock.rs`.
 */
export const DEMO_VOLUME: Readonly<VolumeStatus> = Object.freeze({ percent: 60, muted: false });

/**
 * How far one wheel notch over the volume item moves the volume, in
 * percentage points (`VOLUME_STEP` in engine.rs).
 */
export const VOLUME_STEP = 2;

/**
 * What one wheel notch over the volume item makes of `current`
 * (`volume_step` in engine.rs): `previous` (the wheel turned away
 * from the user, up) raises it by `VOLUME_STEP` up to 100 and unmutes;
 * `next` lowers it by `VOLUME_STEP` down to 0 and keeps the mute.
 */
export function volumeStep(current: VolumeStatus, direction: ScrollDirection): VolumeStatus {
  const percent = Math.min(100, current.percent);
  return direction === 'previous'
    ? { percent: Math.min(100, percent + VOLUME_STEP), muted: false }
    : { percent: Math.max(0, percent - VOLUME_STEP), muted: current.muted };
}

/**
 * The volume item's title (`volume_title` in engine.rs):
 * `Volume 45 %`, `Volume muted` while muted, `No audio output` without a
 * default output device.
 */
export function volumeTitle(volume: VolumeStatus | null): string {
  if (volume === null) {
    return 'No audio output';
  }
  return volume.muted ? 'Volume muted' : `Volume ${volume.percent} %`;
}

/** What the engine knows before the first regional-format reading: nothing. */
const NO_CLOCK_FORMAT: Readonly<ClockFormat> = Object.freeze({ locale: null, hour12: null });

/**
 * The battery item's title (`battery_title` in engine.rs):
 * `Battery 73 %`, then `, charging` or `, plugged in` (on AC, not charging),
 * then `, battery saver on`; `Battery` alone, whatever else is known, without
 * a known percent.
 */
export function batteryTitle(power: PowerStatus): string {
  if (power.percent === null) {
    return 'Battery';
  }
  let title = `Battery ${power.percent} %`;
  if (power.charging) {
    title += ', charging';
  } else if (power.onAc) {
    title += ', plugged in';
  }
  if (power.saver) {
    title += ', battery saver on';
  }
  return title;
}

/**
 * A side effect the mock would have performed on a real OS (mirror of
 * `MockCall`), or a request worth telling apart (`pinTarget`,
 * `reorderPinned`: what the UI asked to pin and in which order, so tests
 * can tell one request from several; `scrollItem`: every wheel
 * notch the page sent, whatever it did).
 */
export type MockCall =
  | { type: 'windowAction'; windowId: WindowId; action: WindowAction }
  | { type: 'scrollItem'; itemKey: string; direction: ScrollDirection }
  | { type: 'pinTarget'; target: LaunchTarget }
  | { type: 'reorderPinned'; order: AppId[] }
  | { type: 'launch'; target: LaunchTarget }
  | { type: 'deleteToRecycleBin'; paths: string[] }
  | { type: 'shell'; action: ShellAction }
  | { type: 'stepVolume'; direction: ScrollDirection }
  | { type: 'setVolumeMuted'; muted: boolean }
  | { type: 'setDockContentSize'; width: number; height: number; edge: DockPosition }
  | { type: 'setDockHitRegion'; panel: Rect; envelope: Rect; edge: DockPosition }
  | { type: 'setDockPanelRect'; rect: Rect; edge: DockPosition; viewport: Viewport }
  | { type: 'setDockAutoHidden'; hidden: boolean }
  | { type: 'showItemMenu'; itemKey: string; x: number; y: number }
  | { type: 'showDockMenu'; x: number; y: number }
  | { type: 'openSettings' }
  | { type: 'openAppFolder'; folder: AppFolder }
  | { type: 'reloadUserCss' }
  | { type: 'openUserCss'; created: boolean }
  | { type: 'setDockHover'; hovering: boolean }
  | { type: 'releaseDockFocus' }
  | { type: 'quit' };

/**
 * What `get_app_info` answers in a demo run: a mock version, the
 * paths a real installation has for the user `me`, and the platform name.
 * The paths are shown, never opened, in a browser.
 */
export function demoAppInfo(): AppInfo {
  return {
    version: '0.1.0-mock',
    settingsPath: 'C:\\Users\\me\\AppData\\Roaming\\dev.davidcristian.stem\\settings.json',
    logPath: 'C:\\Users\\me\\AppData\\Local\\dev.davidcristian.stem\\logs\\stem.log',
    platform: 'mock',
  };
}

/** Where a real installation of the user `me` keeps `dock.css`: next to `demoAppInfo().settingsPath`. */
export const DEMO_USER_CSS_PATH =
  'C:\\Users\\me\\AppData\\Roaming\\dev.davidcristian.stem\\dock.css';

/** The largest `dock.css` Rust reads (`user_css::MAX_BYTES`, 64 KiB). */
export const USER_CSS_MAX_BYTES = 64 * 1024;

/**
 * What `open_user_css` writes when there is no file (a stand-in: the real
 * template is `user_css::TEMPLATE` in src-tauri). Comments only, like the
 * real one, so creating it changes nothing.
 */
export const MOCK_USER_CSS_TEMPLATE =
  "/* dock.css: your own styles for Stem's dock (mock template). */\n.dock-stage {\n  /* --stem-dot: #0078d4; */\n}\n";

/**
 * The mock's `dock.css` "on disk": its text (saved as
 * UTF-8), its bytes as an editor saved them (`Uint8Array`, which need not
 * be UTF-8), a file that is there but cannot be read (`{ unreadable }`,
 * with the error the log would name), or `null` for no file.
 */
export type MockUserCssFile = string | Uint8Array | { readonly unreadable: string } | null;

/**
 * `user_css::read_file` and `report` for the mock's file: what the page
 * gets (`UserCss` without its path). No file is `notFound`; over 64 KiB
 * is `tooLarge` (checked first, as Rust checks the size before it
 * decodes); bytes that are not UTF-8 are `notUtf8`; `{ unreadable }` is
 * `unreadable`. A leading byte order mark is dropped.
 */
export function readUserCss(file: MockUserCssFile): Pick<UserCss, 'css' | 'problem'> {
  const refused = (problem: UserCssProblem): Pick<UserCss, 'css' | 'problem'> => ({
    css: null,
    problem,
  });
  if (file === null) {
    return refused('notFound');
  }
  if (!(file instanceof Uint8Array) && typeof file !== 'string') {
    return refused('unreadable');
  }
  const bytes = typeof file === 'string' ? new TextEncoder().encode(file) : file;
  if (bytes.length > USER_CSS_MAX_BYTES) {
    return refused('tooLarge');
  }
  let text: string;
  try {
    // `ignoreBOM` keeps a byte order mark in the text, so that only a
    // leading one is dropped below, as `user_css::decode` drops it.
    text = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(bytes);
  } catch {
    return refused('notUtf8');
  }
  return { css: text.startsWith('\uFEFF') ? text.slice(1) : text, problem: null };
}

/** What a read of `file` gives the page (`readUserCss` with the demo's path). */
function userCssFrom(file: MockUserCssFile): UserCss {
  return { path: DEMO_USER_CSS_PATH, ...readUserCss(file) };
}

// ---------------------------------------------------------------------------
// 0. Pure helpers mirrored from stem-core (model.rs, settings.rs)
// ---------------------------------------------------------------------------

/** `stem_core::normalize_path`: lower-case, forward slashes. */
export function normalizePath(path: string): string {
  return path.replace(/\\/g, '/').toLowerCase();
}

export function appIdForExe(path: string): AppId {
  return `exe:${normalizePath(path)}`;
}

export function appIdForAppUserModelId(aumid: string): AppId {
  return `aumid:${aumid.trim()}`;
}

export function appIdForWindowClass(className: string): AppId {
  return `class:${className.trim().toLowerCase()}`;
}

/** `stem_core::app_id_for`: AUMID, else exe path, else window class. */
export function appIdFor(raw: RawWindow): AppId {
  const aumid = raw.appUserModelId?.trim();
  if (aumid) {
    return appIdForAppUserModelId(aumid);
  }
  if (raw.exePath !== null) {
    return appIdForExe(raw.exePath);
  }
  return appIdForWindowClass(raw.className);
}

/** File name without extension; `/` and `\` both separate (mock.rs `file_stem`). */
export function fileStem(path: string): string {
  const name = path.split(/[/\\]/).pop() ?? path;
  const dot = name.lastIndexOf('.');
  return dot <= 0 ? name : name.slice(0, dot);
}

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

const clamp = (v: number, lo: number, hi: number): number => Math.min(hi, Math.max(lo, v));

/** `finite_or` in settings.rs: `value` unless it is NaN or ±Infinity, then `fallback`. */
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

// ---------------------------------------------------------------------------
// 0a. The overlap rule (mirror of crates/stem-core/src/overlap.rs)
// ---------------------------------------------------------------------------

/** `overlap::intersects`: a shared region of positive area; touching edges do not count. */
export function rectsIntersect(a: Rect, b: Rect): boolean {
  if (a.width <= 0 || a.height <= 0 || b.width <= 0 || b.height <= 0) {
    return false;
  }
  return a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height;
}

/**
 * `overlap::overlapped`: `DockState.overlapped` for a panel resting at
 * `panel` (screen px, `null` while unknown) among `windows`: one of them is
 * visible, not minimised, not cloaked, not ours (`ownProcessId`), has a
 * frame, and that frame intersects the panel with a positive area.
 */
export function overlapped(
  panel: Rect | null,
  windows: Iterable<RawWindow>,
  ownProcessId: number | null,
): boolean {
  if (panel === null || panel.width <= 0 || panel.height <= 0) {
    return false;
  }
  for (const w of windows) {
    if (!w.isVisible || w.isMinimized || w.isCloaked || w.rect === null) {
      continue;
    }
    if (ownProcessId !== null && w.processId === ownProcessId) {
      continue;
    }
    if (rectsIntersect(panel, w.rect)) {
      return true;
    }
  }
  return false;
}

/**
 * The app layer's conversion of the UI's resting panel (CSS px of the dock
 * window's viewport) to screen pixels (`windows::panel_on_screen` with the
 * dock window where `windows::layout` puts it: flush with `edge` of the
 * work area and spanning its length along it, docs/ipc/sizing-protocol.md,
 * v1.16). The viewport's corner on the edge side is the work area's: its
 * bottom-left corner on `bottom`, its top-left corner on `top` and `left`,
 * its top-right corner on `right`. At a `scale` other than 1 the app layer
 * scales the CSS rect (rounded outwards, like a hit region) and the
 * viewport before placing them on the physical work area (`scale_rect`,
 * `panel_on_screen` in src-tauri/src/windows.rs).
 */
export function panelOnScreen(
  rect: Rect,
  workArea: Rect,
  viewport: { width: number; height: number },
  edge: DockPosition,
  scale = 1,
): Rect {
  const s = Number.isFinite(scale) && scale > 0 ? scale : 1;
  const scaled =
    s === 1
      ? { ...rect }
      : {
          x: Math.floor(rect.x * s),
          y: Math.floor(rect.y * s),
          width: Math.ceil((rect.x + rect.width) * s) - Math.floor(rect.x * s),
          height: Math.ceil((rect.y + rect.height) * s) - Math.floor(rect.y * s),
        };
  const width = s === 1 ? viewport.width : Math.round(viewport.width * s);
  const height = s === 1 ? viewport.height : Math.round(viewport.height * s);
  const originX = edge === 'right' ? workArea.x + workArea.width - width : workArea.x;
  const originY = edge === 'bottom' ? workArea.y + workArea.height - height : workArea.y;
  return { ...scaled, x: originX + scaled.x, y: originY + scaled.y };
}

/**
 * `windows::VIEWPORT_SLACK_CSS`: two viewports that differ by less than
 * this on an axis (CSS px) are the same on it (`reanchorPanel`).
 */
export const VIEWPORT_SLACK_CSS = 1;

/**
 * The app layer's re-anchoring of a reported panel rect
 * (`windows::reanchor_panel`; docs/ipc/report-provenance.md): `rect`,
 * measured in the viewport `from` for a dock on `edge`, moved to where the
 * page's layout puts it in the viewport `to`. Across the edge it keeps its
 * distance to the viewport's edge side (on `bottom` it moves by the change
 * in height, on `right` by the change in width), along the edge its offset
 * from the centre (half the change in length, rounded to the nearest px,
 * a half away from zero like Rust's `f64::round`). A difference under
 * `VIEWPORT_SLACK_CSS` on an axis moves nothing on it; the size never
 * changes.
 */
export function reanchorPanel(rect: Rect, edge: DockPosition, from: Viewport, to: Viewport): Rect {
  const change = (before: number, after: number): number => {
    const d = after - before;
    return Number.isFinite(d) && Math.abs(d) >= VIEWPORT_SLACK_CSS ? d : 0;
  };
  const dw = change(from.width, to.width);
  const dh = change(from.height, to.height);
  const shifts: Record<DockPosition, readonly [number, number]> = {
    bottom: [dw / 2, dh],
    top: [dw / 2, 0],
    left: [0, dh / 2],
    right: [dw, dh / 2],
  };
  const [dx, dy] = shifts[edge];
  // `Math.round` rounds a half up; Rust's `round` away from zero (`+ 0`: no -0).
  const round = (v: number): number => (v < 0 ? -Math.round(-v) : Math.round(v)) + 0;
  return { ...rect, x: round(rect.x + dx), y: round(rect.y + dy) };
}

/** `CssViewport::validate`: both sides positive and finite, else the Rust error. */
function viewportError(viewport: Viewport): string | null {
  const usable = (v: number): boolean => Number.isFinite(v) && v > 0;
  return usable(viewport.width) && usable(viewport.height)
    ? null
    : `the viewport must have a positive, finite width and height: ${JSON.stringify(viewport)}`;
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

// ---------------------------------------------------------------------------
// 0b. Dropped paths for the Recycle Bin (mirror of crates/stem-platform/src/rules/recycle.rs)
// ---------------------------------------------------------------------------

/** Why `rules::recycle::plan_recycle` leaves a dropped path out (`SkipReason`). */
export type RecycleSkip = 'notAFilePath' | 'root' | 'missing' | 'duplicate' | 'insideDropped';

const RECYCLE_SKIP_TEXT: Record<RecycleSkip, string> = {
  notAFilePath: 'is not an absolute file path',
  root: 'is a drive or share root',
  missing: 'does not exist',
  duplicate: 'is listed twice',
  insideDropped: 'is inside another dropped folder',
};

/** `RecyclePlan`: what a drop on the Recycle Bin asks the shell to do. */
export interface RecyclePlan {
  /** In drop order, in the shell's form (`\` separators, none doubled or trailing). */
  recycle: string[];
  /** In drop order, as dropped. */
  skipped: Array<{ path: string; reason: RecycleSkip }>;
}

/** `shell_path`: a plain absolute file path in the shell's form, or why it is not one. */
function shellPath(raw: string): { path: string } | { skip: RecycleSkip } {
  if (raw === '' || /[\0*?]/.test(raw)) {
    return { skip: 'notAFilePath' };
  }
  const path = raw.replace(/\//g, '\\');
  const plain = (c: string): boolean => c !== '.' && c !== '..' && !c.includes(':');
  let head: string;
  let tail: string[];
  if (path.startsWith('\\\\')) {
    const parts = path.slice(2).split('\\');
    const server = parts[0] ?? '';
    const share = parts[1] ?? '';
    if (server === '' || !plain(server)) {
      return { skip: 'notAFilePath' };
    }
    if (share === '') {
      return { skip: 'root' };
    }
    if (!plain(share)) {
      return { skip: 'notAFilePath' };
    }
    head = `\\\\${server}\\${share}`;
    tail = parts.slice(2);
  } else if (/^[A-Za-z]:\\/.test(path)) {
    head = path.slice(0, 2);
    tail = path.slice(3).split('\\');
  } else {
    return { skip: 'notAFilePath' };
  }
  const components = tail.filter((c) => c !== '');
  if (components.length === 0) {
    return { skip: 'root' };
  }
  if (!components.every(plain)) {
    return { skip: 'notAFilePath' };
  }
  return { path: `${head}\\${components.join('\\')}` };
}

/**
 * `rules::recycle::plan_recycle`: plain absolute paths only, never a root,
 * only what `exists`, each once, nothing inside another dropped folder.
 */
export function recyclePlan(
  paths: readonly string[],
  exists: (path: string) => boolean = () => true,
): RecyclePlan {
  const kept: Array<{ index: number; path: string; key: string }> = [];
  const keys = new Set<string>();
  const skipped: Array<{ index: number; reason: RecycleSkip }> = [];
  paths.forEach((raw, index) => {
    const shell = shellPath(raw);
    if ('skip' in shell) {
      skipped.push({ index, reason: shell.skip });
      return;
    }
    if (!exists(shell.path)) {
      skipped.push({ index, reason: 'missing' });
      return;
    }
    const key = shell.path.toLowerCase();
    if (keys.has(key)) {
      skipped.push({ index, reason: 'duplicate' });
      return;
    }
    keys.add(key);
    kept.push({ index, path: shell.path, key });
  });
  const recycle: string[] = [];
  for (const { index, path, key } of kept) {
    let inside = false;
    for (let at = key.indexOf('\\', 1); at !== -1 && !inside; at = key.indexOf('\\', at + 1)) {
      inside = keys.has(key.slice(0, at));
    }
    if (inside) {
      skipped.push({ index, reason: 'insideDropped' });
    } else {
      recycle.push(path);
    }
  }
  skipped.sort((a, b) => a.index - b.index);
  return {
    recycle,
    skipped: skipped.map(({ index, reason }) => ({ path: paths[index]!, reason })),
  };
}

/** `nothing_to_recycle`: the error of a plan that recycles nothing. */
export function nothingToRecycle(plan: RecyclePlan): string {
  const { skipped } = plan;
  if (skipped.length === 0) {
    return 'no paths to move to the Recycle Bin';
  }
  if (skipped.length === 1) {
    return `${skipped[0]!.path} ${RECYCLE_SKIP_TEXT[skipped[0]!.reason]}`;
  }
  return skipped.every((s) => s.reason === 'missing')
    ? `none of the ${skipped.length} paths exists`
    : `none of the ${skipped.length} paths can be moved to the Recycle Bin`;
}

// ---------------------------------------------------------------------------
// 1. Demo scenario (mirror of crates/stem-platform/src/mock.rs)
// ---------------------------------------------------------------------------

const DEMO_MONITOR_ID = 'mock-primary';
/**
 * The second monitor of the `twoMonitors` scenario: a 1920 × 1080
 * display to the right of the primary, scaled 150 %, with the stock
 * taskbar's 48 px taken off its work area. Named like a real Windows
 * device, so the URL and label rules get a `\\.\` to chew on.
 */
export const SECOND_MONITOR_ID = '\\\\.\\DISPLAY3';

function demoMonitor(): MonitorInfo {
  return {
    id: DEMO_MONITOR_ID,
    name: 'Mock Display',
    bounds: { x: 0, y: 0, width: 2560, height: 1440 },
    workArea: { x: 0, y: 0, width: 2560, height: 1392 },
    scaleFactor: 1,
    isPrimary: true,
  };
}

function secondMonitor(): MonitorInfo {
  return {
    id: SECOND_MONITOR_ID,
    name: 'DISPLAY3',
    bounds: { x: 2560, y: 0, width: 1920, height: 1080 },
    workArea: { x: 2560, y: 0, width: 1920, height: 1032 },
    scaleFactor: 1.5,
    isPrimary: false,
  };
}

/**
 * The label of the dock window on `monitor` (`dock_label` in
 * src-tauri/src/windows.rs): `dock` for the primary monitor's, else `dock-`
 * and the id with every character outside `[A-Za-z0-9_-]` dropped.
 */
export function dockLabel(monitor: MonitorId | null, primary: MonitorId | null): string {
  if (monitor === null || monitor === primary) {
    return 'dock';
  }
  return `dock-${monitor.replace(/[^A-Za-z0-9_-]/g, '')}`;
}

/**
 * A demo window with a frame on a demo monitor. Every demo frame
 * keeps clear of the 250 px of its work area along the bottom, the left and
 * the right edge, where a dock's panel rests on those edges, so
 * nothing overlaps it until a test moves one (`moveWindow`); a minimised
 * window keeps the frame it would be restored to (the overlap rule ignores
 * it while minimised). The top band is not kept clear: the frames start
 * near the top as on a real desktop, and the demo has no room for windows
 * of these sizes between two bands, so a dock on `top` with
 * `whenOverlapped` hides in the demo. `demoWindows` has the same frames
 * as the Rust demo (`demo_windows` in `crates/stem-platform/src/mock.rs`).
 */
function demoWindow(
  id: number,
  exe: string,
  title: string,
  minimized: boolean,
  rect: Rect,
  monitor: MonitorId = DEMO_MONITOR_ID,
): RawWindow {
  return {
    id,
    title,
    className: `${fileStem(exe)}Class`,
    processId: 1000 + id,
    exePath: exe,
    appUserModelId: null,
    isVisible: true,
    isMinimized: minimized,
    isCloaked: false,
    onCurrentDesktop: true,
    monitor,
    rect,
    iconKey: null,
  };
}

const EXE = {
  explorer: 'C:\\Windows\\explorer.exe',
  firefox: 'C:\\Program Files\\Mozilla Firefox\\firefox.exe',
  code: 'C:\\Users\\me\\AppData\\Local\\Programs\\Microsoft VS Code\\Code.exe',
  terminal: 'C:\\Program Files\\WindowsApps\\Terminal\\WindowsTerminal.exe',
  spotify: 'C:\\Program Files\\Spotify\\Spotify.exe',
  notepad: 'C:\\Windows\\notepad.exe',
  paint: 'C:\\Windows\\System32\\mspaint.exe',
} as const;

function demoWindows(): RawWindow[] {
  return [
    demoWindow(1, EXE.explorer, 'Downloads', false, { x: 260, y: 90, width: 1100, height: 700 }),
    demoWindow(2, EXE.firefox, 'Stem — Mozilla Firefox', false, {
      x: 640,
      y: 60,
      width: 1500,
      height: 1000,
    }),
    demoWindow(3, EXE.firefox, 'GitHub — Mozilla Firefox', true, {
      x: 700,
      y: 120,
      width: 1400,
      height: 900,
    }),
    demoWindow(4, EXE.code, 'lib.rs — Stem', false, { x: 260, y: 40, width: 1600, height: 1000 }),
    demoWindow(5, EXE.terminal, 'PowerShell', false, { x: 1300, y: 500, width: 1000, height: 600 }),
    demoWindow(6, EXE.spotify, 'Spotify Premium', true, {
      x: 400,
      y: 200,
      width: 1200,
      height: 800,
    }),
  ];
}

function pinExe(path: string, label: string | null = null): PinnedApp {
  return {
    id: appIdForExe(path),
    label,
    launch: { kind: 'exe', path, args: [], workingDir: null },
  };
}

interface Scenario {
  monitors: MonitorInfo[];
  windows: RawWindow[];
  foreground: WindowId | null;
  recycleBin: RecycleBinState;
  settings: Settings;
}

/**
 * The Rust demo: six windows of five apps, Firefox in the foreground, the
 * Recycle Bin full. The pinned list is this mock's own addition (the Rust
 * service tests run with default, empty settings); it exercises pinned apps
 * with and without windows and a non-exe launch target.
 */
function demoScenario(): Scenario {
  const settings = defaultSettings();
  settings.pinned = [
    pinExe(EXE.explorer),
    pinExe(EXE.firefox),
    pinExe(EXE.terminal),
    pinExe(EXE.notepad),
    // A URI has no file name to derive a display name from; `label` is the
    // user override a real settings file would carry for such an entry.
    { id: 'uri:ms-settings:', label: 'Settings', launch: { kind: 'uri', uri: 'ms-settings:' } },
  ];
  return {
    monitors: [demoMonitor()],
    windows: demoWindows(),
    foreground: 2,
    recycleBin: { itemCount: 3, sizeBytes: 1_234_567 },
    settings,
  };
}

/**
 * The demo plus a second monitor to the right with two windows on
 * it: one of Notepad (pinned, no window on the primary) and one of Paint
 * (not pinned, nowhere else), both clear of the 250 px of that work area
 * along its bottom, left and right edges. With `behavior.showOnlyCurrentMonitor` on, the second monitor's
 * dock shows the pinned apps plus Paint, with Notepad's one window, and the
 * primary's shows everything but Paint, with no window for Notepad.
 */
function twoMonitorsScenario(): Scenario {
  const scenario = demoScenario();
  scenario.monitors.push(secondMonitor());
  scenario.windows.push(
    demoWindow(
      7,
      EXE.notepad,
      'notes.txt - Notepad',
      false,
      { x: 2820, y: 100, width: 900, height: 600 },
      SECOND_MONITOR_ID,
    ),
    demoWindow(
      8,
      EXE.paint,
      'Untitled - Paint',
      false,
      { x: 3100, y: 60, width: 1100, height: 700 },
      SECOND_MONITOR_ID,
    ),
  );
  return scenario;
}

/** Twenty apps (eight pinned, twelve running unpinned) for layout stress. */
function manyScenario(): Scenario {
  const names = [
    'Slack',
    'Discord',
    'Teams',
    'Outlook',
    'Word',
    'Excel',
    'Photoshop',
    'Figma',
    'Steam',
    'Obsidian',
    'Notion',
    'Chrome',
    'Edge',
    'Blender',
    'Paint',
    'Zoom',
    'Telegram',
    'Calculator',
    'Mail',
    'Photos',
  ];
  const exeOf = (name: string): string => `C:\\Program Files\\${name}\\${name}.exe`;
  const settings = defaultSettings();
  settings.pinned = names.slice(0, 8).map((n) => pinExe(exeOf(n)));

  const windows: RawWindow[] = [];
  const open = (name: string, title: string, minimized = false): void => {
    // A cascade from the top-left corner, every frame clear of the 250 px of
    // the work area along the bottom, left and right edges (the 19th ends at
    // y = 40 + 18 × 20 + 640 and x = 260 + 18 × 50 + 1100).
    const n = windows.length;
    windows.push(
      demoWindow(n + 1, exeOf(name), title, minimized, {
        x: 260 + 50 * n,
        y: 40 + 20 * n,
        width: 1100,
        height: 640,
      }),
    );
  };
  // Pinned apps that are running.
  open('Slack', 'general — Stem Team');
  open('Slack', 'DM — Alex');
  open('Discord', 'Discord');
  open('Outlook', 'Inbox — Outlook');
  open('Word', 'Design notes.docx — Word');
  // Running, unpinned apps; Chrome has three windows, Steam is minimised.
  for (const name of names.slice(8)) {
    open(name, name === 'Chrome' ? 'Stem — Chrome' : name, name === 'Steam');
    if (name === 'Chrome') {
      open(name, 'GitHub — Chrome');
      open(name, 'Docs — Chrome', true);
    }
  }
  const chrome = windows.find((w) => w.title === 'Stem — Chrome');
  return {
    monitors: [demoMonitor()],
    windows,
    foreground: chrome?.id ?? null,
    recycleBin: { itemCount: 0, sizeBytes: 0 },
    settings,
  };
}

const SCENARIOS: Record<MockScenario, () => Scenario> = {
  default: demoScenario,
  many: manyScenario,
  twoMonitors: twoMonitorsScenario,
};

/** `identity_for` in mock.rs: display name from the exe stem, launch by exe. */
function identityForWindow(raw: RawWindow): AppIdentity {
  const id = appIdFor(raw);
  return {
    id,
    displayName: raw.exePath !== null ? fileStem(raw.exePath) : raw.title,
    iconKey: `mock:${id}`,
    launch:
      raw.exePath !== null ? { kind: 'exe', path: raw.exePath, args: [], workingDir: null } : null,
  };
}

/** `identify_target` in mock.rs. */
function identityForTarget(target: LaunchTarget): AppIdentity {
  switch (target.kind) {
    case 'exe':
    case 'shortcut': {
      const id = appIdForExe(target.path);
      return { id, displayName: fileStem(target.path), iconKey: `mock:${id}`, launch: target };
    }
    case 'storeApp': {
      const id = appIdForAppUserModelId(target.appUserModelId);
      return { id, displayName: target.appUserModelId, iconKey: `mock:${id}`, launch: target };
    }
    case 'uri': {
      const id = `uri:${target.uri}`;
      return { id, displayName: target.uri, iconKey: `mock:${id}`, launch: target };
    }
  }
}

// ---------------------------------------------------------------------------
// 2. Engine (mirror of crates/stem-core/src/engine.rs)
// ---------------------------------------------------------------------------

interface Tracked {
  raw: RawWindow;
  /** First-seen sequence number; drives stable ordering. */
  seq: number;
  attention: boolean;
}

/** One dock window as the app layer registered it (`DockPanel` in the Rust engine). */
interface DockPanel {
  /** The monitor the window sits on; `null` while unknown (no entry in `DockState.docks` then). */
  monitor: MonitorId | null;
  /** Where its panel rests, in screen px; `null` before the page's first report and while hidden. */
  rect: Rect | null;
  /** `DockFacts.overlapped` for this panel. */
  overlapped: boolean;
}

/** `ItemAction`: what a primary click should do. */
export type ItemAction =
  | { type: 'launch'; appId: AppId }
  | { type: 'activate'; windowId: WindowId }
  | { type: 'minimize'; windowId: WindowId }
  | { type: 'shell'; action: ShellAction }
  | { type: 'none' };

/**
 * What `DockEngine::apply` compares to tell whether an input changed the
 * snapshot (`renders_differently` in engine.rs): everything but the
 * revision, with `power` only while the battery item is shown and `volume`
 * only while the volume item is, since nothing else on the pages reads them.
 * Two snapshots that give the same string show the same thing.
 */
export function shownState(state: DockState): string {
  const shows = (kind: DockItemKind): boolean => state.items.some((i) => i.kind === kind);
  return JSON.stringify({
    ...state,
    revision: 0,
    power: shows('battery') ? state.power : 'not shown',
    volume: shows('volume') ? state.volume : 'not shown',
  });
}

/** `ScrollEffect`: what one wheel notch over an item does. */
export type ScrollEffect =
  | { type: 'activate'; windowId: WindowId }
  | { type: 'stepVolume'; direction: ScrollDirection }
  | { type: 'nothing' }
  | { type: 'unknownItem' };

function fixedItem(
  kind: DockItemKind,
  key: string,
  title: string,
  iconKey: IconKey | null,
): DockItem {
  return {
    key,
    kind,
    appId: null,
    title,
    iconKey,
    pinned: false,
    windows: [],
    isActive: false,
    needsAttention: false,
    badge: null,
  };
}

class Engine {
  settings: Settings;
  readonly windows = new Map<WindowId, Tracked>();
  readonly identities = new Map<AppId, AppIdentity>();
  private runningOrder: AppId[] = [];
  foreground: WindowId | null = null;
  /** The platform's fullscreen verdict, stored as is, with the monitor it names. */
  fullscreen: { active: boolean; monitor: MonitorId | null } = { active: false, monitor: null };
  /** The dock windows the app layer registered, by label. */
  readonly panels = new Map<string, DockPanel>();
  /** The mock has no process of its own; every window counts (`overlapped`). */
  readonly ownProcessId: number | null = null;
  monitors: MonitorInfo[] = [];
  theme: ThemeInfo;
  recycleBin: RecycleBinState = { itemCount: 0, sizeBytes: 0 };
  /** The last power reading, `DockState.power`; nothing known at first. */
  power: PowerStatus = { ...NO_POWER };
  /** The last volume reading, `DockState.volume`; no device known at first. */
  volume: VolumeStatus | null = null;
  /** The last regional-format reading, `DockState.clockFormat`; nothing known at first. */
  clockFormat: ClockFormat = { ...NO_CLOCK_FORMAT };
  private nextSeq = 1;
  revision = 0;

  constructor(settings: Settings, theme: ThemeInfo) {
    this.settings = settings;
    this.theme = theme;
  }

  // --- inputs (each returns whether the snapshot changed; `input` decides and bumps the revision)

  /**
   * `DockEngine::apply`: run one input (`mutate`), then tell whether the
   * renderable snapshot changed by comparing it with the one before
   * (`EngineOutcome::changed`); only then does the revision go up,
   * by one, and only then does the backend send a `dock://state`. So an
   * input that changes nothing the snapshot shows changes nothing: the
   * theme, a monitor list that moves no dock's fact, the identity of an
   * app without an item, a setting no item depends on, a frame that flips
   * no `overlapped`, the size of the Recycle Bin's contents, and a power
   * or volume reading while its item is not shown (`shownState`).
   */
  private input(mutate: () => void): boolean {
    const before = shownState(this.snapshot());
    mutate();
    this.rebuildRunningOrder();
    const changed = shownState(this.snapshot()) !== before;
    if (changed) {
      this.revision += 1;
    }
    return changed;
  }

  /**
   * `WindowsSnapshot`: the list replaces the tracked windows, each one seen
   * before keeping its first-seen order and its attention flag; a foreground
   * window that is not in it any more is forgotten, as in Rust.
   */
  snapshotWindows(list: RawWindow[]): boolean {
    return this.input(() => {
      const next = new Map<WindowId, Tracked>();
      for (const raw of list) {
        const prev = this.windows.get(raw.id);
        next.set(raw.id, {
          raw,
          seq: prev?.seq ?? this.allocSeq(),
          attention: prev?.attention ?? false,
        });
      }
      this.windows.clear();
      for (const [id, t] of next) {
        this.windows.set(id, t);
      }
      // A resync can reveal that the foreground window is gone, as in
      // `DockEngine::apply_platform`.
      if (this.foreground !== null && !this.windows.has(this.foreground)) {
        this.foreground = null;
      }
      this.recomputeOverlap();
    });
  }

  /**
   * `WindowAdded` / `WindowUpdated`. A change of the frame alone is stored
   * and changes nothing (no item shows it), unless it flips `overlapped`,
   * exactly like `DockEngine::upsert`.
   */
  upsertWindow(raw: RawWindow): boolean {
    return this.input(() => {
      const t = this.windows.get(raw.id);
      if (t) {
        t.raw = raw;
      } else {
        this.windows.set(raw.id, { raw, seq: this.allocSeq(), attention: false });
      }
      this.recomputeOverlap();
    });
  }

  removeWindow(id: WindowId): boolean {
    return this.input(() => {
      if (this.windows.delete(id)) {
        this.recomputeOverlap();
      }
      if (this.foreground === id) {
        this.foreground = null;
      }
    });
  }

  /**
   * `EngineInput::DockPanelChanged`: the dock window `label`
   * sits on `monitor` with its panel at `rect` (screen px; `null` while the
   * window is hidden); both `null` forgets the label (the window closed).
   * Only a change of the docks' facts (an entry coming or going, a flip of
   * its `overlapped`) changes the snapshot.
   */
  setDockPanel(label: string, monitor: MonitorId | null, rect: Rect | null): boolean {
    return this.input(() => {
      if (monitor === null && rect === null) {
        this.panels.delete(label);
      } else {
        const panel = this.panels.get(label) ?? { monitor: null, rect: null, overlapped: false };
        panel.monitor = monitor;
        panel.rect = rect === null ? null : { ...rect };
        this.panels.set(label, panel);
      }
      this.recomputeOverlap();
    });
  }

  /** `overlap::overlapped` over the tracked windows, per panel. */
  private recomputeOverlap(): void {
    const windows = [...this.windows.values()].map((t) => t.raw);
    for (const panel of this.panels.values()) {
      panel.overlapped = overlapped(panel.rect, windows, this.ownProcessId);
    }
  }

  /** The primary monitor as the app layer picks it: the flagged one, else the first. */
  primaryMonitorId(): MonitorId | null {
    return (this.monitors.find((m) => m.isPrimary) ?? this.monitors[0])?.id ?? null;
  }

  /**
   * `DockState.docks` (`DockEngine::dock_facts`): one entry per
   * monitor with a registered dock window, sorted by monitor id;
   * `fullscreen` when the verdict names that monitor, or names none and it
   * is the primary; `overlapped` from the panel (any panel on it).
   */
  dockFacts(): DockFacts[] {
    const primary = this.primaryMonitorId();
    const byMonitor = new Map<MonitorId, DockFacts>();
    for (const panel of this.panels.values()) {
      if (panel.monitor === null) {
        continue;
      }
      const fullscreen =
        this.fullscreen.active &&
        (this.fullscreen.monitor === null
          ? primary === panel.monitor
          : this.fullscreen.monitor === panel.monitor);
      const facts = byMonitor.get(panel.monitor) ?? {
        monitor: panel.monitor,
        fullscreen,
        overlapped: false,
      };
      facts.overlapped ||= panel.overlapped;
      byMonitor.set(panel.monitor, facts);
    }
    return [...byMonitor.values()].sort((a, b) =>
      a.monitor < b.monitor ? -1 : a.monitor > b.monitor ? 1 : 0,
    );
  }

  setForeground(id: WindowId | null): boolean {
    return this.input(() => {
      const next = id !== null && this.windows.has(id) ? id : null;
      this.foreground = next;
      const t = next !== null ? this.windows.get(next) : undefined;
      if (t) {
        // Activating a window clears its attention request.
        t.attention = false;
      }
    });
  }

  setAttention(id: WindowId, flashing: boolean): boolean {
    return this.input(() => {
      const t = this.windows.get(id);
      if (t) {
        // As `PlatformEvent::WindowAttention` in engine.rs: the window
        // in front already has the user's attention, so its flash flags nothing.
        t.attention = flashing && this.foreground !== id;
      }
    });
  }

  /**
   * `PlatformEvent::FullscreenChanged` with the window's monitor:
   * part of the snapshot through the docks' facts, so a repeat, or a
   * verdict about a monitor without a dock, changes nothing.
   */
  setFullscreen(active: boolean, monitor: MonitorId | null): boolean {
    return this.input(() => {
      this.fullscreen = { active, monitor: active ? monitor : null };
    });
  }

  /**
   * `PlatformEvent::MonitorsChanged`: not part of the snapshot, except that
   * a new primary can move a verdict without a monitor to another dock.
   */
  setMonitors(monitors: MonitorInfo[]): boolean {
    return this.input(() => {
      this.monitors = monitors;
    });
  }

  /** `PlatformEvent::ThemeChanged`: kept for `get_theme`; not part of the snapshot. */
  setTheme(theme: ThemeInfo): boolean {
    return this.input(() => {
      this.theme = theme;
    });
  }

  /** `PlatformEvent::RecycleBinChanged`: the snapshot shows only whether the bin is empty. */
  setRecycleBin(state: RecycleBinState): boolean {
    return this.input(() => {
      this.recycleBin = state;
    });
  }

  /**
   * `PlatformEvent::PowerChanged`: part of the snapshot whatever the
   * settings say, but only a different reading while the battery item is
   * shown (or one that brings or takes it) bumps the revision.
   */
  setPower(power: PowerStatus): boolean {
    return this.input(() => {
      this.power = { ...power };
    });
  }

  /**
   * `PlatformEvent::VolumeChanged`: part of the snapshot whatever
   * the settings say (and the volume item's title), but only a different
   * reading while the volume item is shown bumps the revision.
   */
  setVolume(volume: VolumeStatus | null): boolean {
    return this.input(() => {
      this.volume = volume === null ? null : { percent: volume.percent, muted: volume.muted };
    });
  }

  /**
   * `PlatformEvent::ClockFormatChanged`: part of the snapshot
   * whatever the settings say and nothing else, so only a different reading
   * bumps the revision.
   */
  setClockFormat(format: ClockFormat): boolean {
    return this.input(() => {
      this.clockFormat = { locale: format.locale, hour12: format.hour12 };
    });
  }

  /** `EngineInput::SettingsChanged`: a change counts only where an item shows it. */
  setSettings(settings: Settings): boolean {
    return this.input(() => {
      this.settings = settings;
    });
  }

  /** `EngineInput::IdentityResolved`: counts only for an app that has an item. */
  setIdentity(appId: AppId, identity: AppIdentity): boolean {
    return this.input(() => {
      this.identities.set(appId, identity);
    });
  }

  // --- queries

  /** Apps (pinned or running) without an identity yet. */
  unresolvedApps(): AppId[] {
    const ids = new Set<AppId>(this.settings.pinned.map((p) => p.id));
    for (const id of this.runningOrder) {
      ids.add(id);
    }
    return [...ids].filter((id) => !this.identities.has(id)).sort();
  }

  isPinned(appId: AppId): boolean {
    return this.settings.pinned.some((p) => p.id === appId);
  }

  /** `Settings::is_hidden`. */
  isHidden(appId: AppId): boolean {
    return this.settings.hiddenApps.some((h) => h.id === appId);
  }

  /**
   * `DockEngine::app_name`: the resolved display name, else the
   * title of the app's first shown window (what its item's title falls
   * back to), else of its first-seen tracked window (an app with none
   * shown has no item); `null` when the engine knows nothing of the app.
   */
  appName(appId: AppId): string | null {
    const identity = this.identities.get(appId);
    if (identity) {
      return identity.displayName;
    }
    return this.appWindow(appId)?.title ?? null;
  }

  /**
   * `DockEngine::app_window`: the app's first shown window, else
   * its first-seen tracked window (an app with none shown has no item).
   */
  appWindow(appId: AppId): RawWindow | undefined {
    return (
      this.firstWindowOf(appId) ??
      [...this.windows.values()]
        .filter((t) => appIdFor(t.raw) === appId)
        .sort((a, b) => a.seq - b.seq)[0]?.raw
    );
  }

  /**
   * `hidden_app_facts` in stem-service: the program's path from the
   * app's window (`appWindow`), else the file its identity launches (an
   * `exe` or a `shortcut`); the icon key its item shows, the identity's
   * else that window's, but never one bound to a window (`hwnd:`), for
   * which the file's own key (`exe:` / `file:`) stands in when the path is
   * known.
   */
  hiddenAppFacts(appId: AppId): Pick<HiddenApp, 'path' | 'iconKey'> {
    const window = this.appWindow(appId);
    const identity = this.identities.get(appId);
    const launch = identity?.launch ?? null;
    let path: string | null = null;
    let fileIcon: IconKey | null = null;
    const exePath = window?.exePath ?? null;
    if (exePath !== null) {
      path = exePath;
      fileIcon = `exe:${path}`;
    } else if (launch?.kind === 'exe') {
      path = launch.path;
      fileIcon = `exe:${path}`;
    } else if (launch?.kind === 'shortcut') {
      path = launch.path;
      fileIcon = `file:${path}`;
    }
    const kept = [identity?.iconKey ?? null, window?.iconKey ?? null].find(
      (key): key is IconKey => key !== null && !key.startsWith('hwnd:'),
    );
    return { path, iconKey: kept ?? fileIcon };
  }

  firstWindowOf(appId: AppId): RawWindow | undefined {
    return this.shownWindows().find((t) => appIdFor(t.raw) === appId)?.raw;
  }

  /** Build the renderable snapshot (same ordering rules as `DockEngine::snapshot`). */
  snapshot(): DockState {
    const s = this.settings;
    const items: DockItem[] = [];
    let seps = 0;
    const separator = (): void => {
      if (!s.items.separators) {
        return;
      }
      seps += 1;
      items.push(fixedItem('separator', `sep:${seps}`, '', null));
    };

    // 1. Fixed left group.
    const leftStart = items.length;
    if (s.items.launcher) {
      items.push(fixedItem('launcher', 'fixed:launcher', 'Start', 'shell:start'));
    }
    if (s.items.taskView) {
      items.push(fixedItem('taskView', 'fixed:task-view', 'Task View', 'shell:task-view'));
    }
    if (s.items.showDesktop) {
      items.push(
        fixedItem('showDesktop', 'fixed:show-desktop', 'Show Desktop', 'shell:show-desktop'),
      );
    }
    const hasLeft = items.length > leftStart;

    // Group shown windows by app once.
    const byApp = new Map<AppId, Tracked[]>();
    for (const t of this.shownWindows()) {
      const id = appIdFor(t.raw);
      let list = byApp.get(id);
      if (!list) {
        list = [];
        byApp.set(id, list);
      }
      list.push(t);
    }

    // 2. Pinned apps, in the user's order.
    const pinnedItems = s.pinned.map((pin) => {
      const windows = byApp.get(pin.id) ?? [];
      byApp.delete(pin.id);
      return this.appItem(pin.id, true, pin.label, windows);
    });

    // 3. Running, unpinned apps in first-seen order, but the hidden ones;
    // the pins above took their windows first.
    const runningItems: DockItem[] = [];
    for (const id of this.runningOrder) {
      const windows = byApp.get(id);
      if (windows) {
        byApp.delete(id);
        if (!this.isHidden(id)) {
          runningItems.push(this.appItem(id, false, null, windows));
        }
      }
    }

    if (hasLeft && (pinnedItems.length > 0 || runningItems.length > 0)) {
      separator();
    }
    items.push(...pinnedItems);
    if (pinnedItems.length > 0 && runningItems.length > 0) {
      separator();
    }
    items.push(...runningItems);

    // 4. Fixed trailing group: Quick Settings, Notification Center, volume,
    // battery, clock, and the Recycle Bin last, with one separator before
    // the group and none inside.
    const trailing: DockItem[] = [];
    if (s.items.quickSettings) {
      trailing.push(
        fixedItem(
          'quickSettings',
          'fixed:quick-settings',
          'Quick Settings',
          'shell:quick-settings',
        ),
      );
    }
    if (s.items.notificationCenter) {
      trailing.push(
        fixedItem(
          'notificationCenter',
          'fixed:notification-center',
          'Notification Center',
          'shell:notification-center',
        ),
      );
    }
    // The volume item whenever it is on: without a device its title says so.
    if (s.items.volume) {
      trailing.push(fixedItem('volume', 'fixed:volume', volumeTitle(this.volume), 'shell:volume'));
    }
    // No battery item on a machine without a battery.
    if (s.items.battery && this.power.hasBattery) {
      trailing.push(
        fixedItem('battery', 'fixed:battery', batteryTitle(this.power), 'shell:battery'),
      );
    }
    if (s.items.clock) {
      trailing.push(fixedItem('clock', 'fixed:clock', 'Clock', 'shell:clock'));
    }
    if (s.items.recycleBin) {
      const icon = this.recycleBin.itemCount === 0 ? 'recycle-bin-empty' : 'recycle-bin-full';
      trailing.push(fixedItem('recycleBin', 'fixed:recycle-bin', 'Recycle Bin', `shell:${icon}`));
    }
    if (trailing.length > 0 && items.length > 0) {
      separator();
    }
    items.push(...trailing);

    return {
      revision: this.revision,
      items,
      foreground: this.foreground,
      docks: this.dockFacts(),
      power: { ...this.power },
      volume: this.volume === null ? null : { ...this.volume },
      clockFormat: { ...this.clockFormat },
    };
  }

  /**
   * `DockEngine::windows_on_dock`: the shown windows of `appId`
   * that the dock window `dock` lists, in first-seen order. With
   * `monitors = all` and `showOnlyCurrentMonitor` on, only those on the
   * dock's registered monitor; otherwise, or while that is unknown, all.
   */
  windowsOnDock(appId: AppId, dock: string): WindowId[] {
    const { monitors, behavior } = this.settings;
    const monitor =
      monitors === 'all' && behavior.showOnlyCurrentMonitor
        ? (this.panels.get(dock)?.monitor ?? null)
        : null;
    return this.shownWindows()
      .filter((t) => appIdFor(t.raw) === appId)
      .filter((t) => monitor === null || t.raw.monitor === monitor)
      .map((t) => t.raw.id);
  }

  /** `DockEngine::primary_action`: a click on `itemKey` in the dock window `dock`. */
  primaryAction(itemKey: string, dock: string): ItemAction {
    switch (itemKey) {
      case 'fixed:launcher':
        return { type: 'shell', action: 'openStartMenu' };
      case 'fixed:task-view':
        return { type: 'shell', action: 'openTaskView' };
      case 'fixed:show-desktop':
        return { type: 'shell', action: 'toggleShowDesktop' };
      case 'fixed:quick-settings':
        return { type: 'shell', action: 'openQuickSettings' };
      case 'fixed:notification-center':
      case 'fixed:clock':
        return { type: 'shell', action: 'openNotificationCenter' };
      case 'fixed:volume':
      case 'fixed:battery':
        return { type: 'shell', action: 'openQuickSettings' };
      case 'fixed:recycle-bin':
        return { type: 'shell', action: 'openRecycleBin' };
      default:
        break;
    }
    if (!itemKey.startsWith('app:')) {
      return { type: 'none' };
    }
    const appId = itemKey.slice('app:'.length);
    if (this.isHidden(appId) && !this.isPinned(appId)) {
      // No item: a click on its key from a page a state behind.
      return { type: 'none' };
    }
    const windows = this.windowsOnDock(appId, dock);
    if (windows.length === 0) {
      const launchable =
        this.isPinned(appId) || (this.identities.get(appId)?.launch ?? null) !== null;
      return launchable ? { type: 'launch', appId } : { type: 'none' };
    }
    const activePos = this.foreground === null ? -1 : windows.indexOf(this.foreground);
    if (activePos === -1) {
      return { type: 'activate', windowId: windows[0]! };
    }
    if (windows.length === 1) {
      return this.settings.behavior.clickActiveMinimizes
        ? { type: 'minimize', windowId: windows[activePos]! }
        : { type: 'activate', windowId: windows[activePos]! };
    }
    return { type: 'activate', windowId: windows[(activePos + 1) % windows.length]! };
  }

  /**
   * `DockEngine::scroll_action`: one wheel notch `direction` over
   * `itemKey` in the dock window `dock`. A key with no item is
   * `unknownItem`, whatever the setting; the volume item is `stepVolume`
   * whatever the setting; with `cycleWindows`, an app item
   * with windows on that dock activates the window after (`next`) or
   * before (`previous`) the one in front, wrapping around, or the first
   * (`next`) or the last (`previous`) when none of them is in front;
   * anything else is `nothing`. The engine's memory of the window a
   * dock's last notch tried (`note_scroll_activation`, for an activation
   * Windows refuses or reports late) is not mirrored: here every
   * activation succeeds and takes the foreground at once, so stepping
   * from the foreground is the same.
   */
  scrollAction(itemKey: string, dock: string, direction: ScrollDirection): ScrollEffect {
    const item = this.snapshot().items.find((i) => i.key === itemKey);
    if (item === undefined) {
      return { type: 'unknownItem' };
    }
    if (item.kind === 'volume') {
      return { type: 'stepVolume', direction };
    }
    if (this.settings.behavior.scrollAction === 'none') {
      return { type: 'nothing' };
    }
    if (item.kind !== 'app' || item.appId === null) {
      return { type: 'nothing' };
    }
    const windows = this.windowsOnDock(item.appId, dock);
    const count = windows.length;
    if (count === 0) {
      return { type: 'nothing' };
    }
    const front = this.foreground === null ? -1 : windows.indexOf(this.foreground);
    let index: number;
    if (front === -1) {
      index = direction === 'next' ? 0 : count - 1;
    } else {
      index = direction === 'next' ? (front + 1) % count : (front + count - 1) % count;
    }
    return { type: 'activate', windowId: windows[index]! };
  }

  // --- internals

  private allocSeq(): number {
    return this.nextSeq++;
  }

  /**
   * Mirror of `DockEngine::is_shown`: visible, and, while
   * `showOnlyCurrentDesktop` is on, on the current virtual desktop, which
   * the platform's `onCurrentDesktop` decides when known and the
   * cloak state otherwise.
   */
  private isShown(raw: RawWindow): boolean {
    if (!raw.isVisible) {
      return false;
    }
    if (!this.settings.behavior.showOnlyCurrentDesktop) {
      return true;
    }
    return raw.onCurrentDesktop ?? !raw.isCloaked;
  }

  /** Windows that should appear in the dock, in first-seen order. */
  private shownWindows(): Tracked[] {
    return [...this.windows.values()]
      .filter((t) => this.isShown(t.raw))
      .sort((a, b) => a.seq - b.seq);
  }

  /** Keep the relative order of apps still running, append newcomers. */
  private rebuildRunningOrder(): void {
    const present: AppId[] = [];
    for (const t of this.shownWindows()) {
      const id = appIdFor(t.raw);
      if (!present.includes(id)) {
        present.push(id);
      }
    }
    const order = this.runningOrder.filter((id) => present.includes(id));
    for (const id of present) {
      if (!order.includes(id)) {
        order.push(id);
      }
    }
    this.runningOrder = order;
  }

  private appItem(id: AppId, pinned: boolean, label: string | null, windows: Tracked[]): DockItem {
    const identity = this.identities.get(id);
    const first = windows[0]?.raw;
    const title = label ?? identity?.displayName ?? first?.title ?? id;
    const iconKey = identity?.iconKey ?? first?.iconKey ?? null;
    const infos: WindowInfo[] = windows.map((t) => ({
      id: t.raw.id,
      title: t.raw.title,
      isMinimized: t.raw.isMinimized,
      isActive: this.foreground === t.raw.id,
      monitor: t.raw.monitor,
      iconKey: t.raw.iconKey,
      needsAttention: t.attention,
    }));
    return {
      key: `app:${id}`,
      kind: 'app',
      appId: id,
      title,
      iconKey,
      pinned,
      windows: infos,
      isActive: infos.some((w) => w.isActive),
      needsAttention: infos.some((w) => w.needsAttention),
      badge: null,
    };
  }
}

/** `fallback_display_name` in stem-service. */
function fallbackDisplayName(appId: AppId): string {
  const parts = appId.split(/[/\\:]/);
  let tail = parts[parts.length - 1] ?? appId;
  if (tail.endsWith('.exe')) {
    tail = tail.slice(0, -'.exe'.length);
  }
  return tail === '' ? appId : tail;
}

// ---------------------------------------------------------------------------
// 3. The backend (mirror of stem-service command semantics)
// ---------------------------------------------------------------------------

type Listeners = { [E in BackendEvent]?: Set<EventHandler<E>> };

const DEFAULT_THEME: ThemeInfo = {
  accent: '#0078D4',
  dark: true,
  highContrast: false,
  transparency: true,
};

/** Small seeded PRNG (mulberry32) so simulated latency is reproducible. */
function mulberry32(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

export class MockBackend implements Backend {
  private readonly engine: Engine;
  private readonly listeners: Listeners = {};
  private readonly random: () => number;
  private readonly latency: readonly [number, number];
  private readonly identityDelayMs: number;
  private readonly logging: boolean;
  private readonly viewportHeight: number | undefined;
  private readonly viewportWidth: number | undefined;
  /** The monitor of the dock page this backend serves (`MockOptions.monitor`); `null` for the primary's. */
  readonly pageMonitor: MonitorId | null;
  private readonly timers = new Set<ReturnType<typeof setTimeout>>();
  /** Settles when every command issued so far has run (see `command`). */
  private queue: Promise<void> = Promise.resolve();
  private identityTimer: ReturnType<typeof setTimeout> | null = null;
  private failNext: string | null = null;
  private readonly mediaQuery: MediaQueryList | null;
  private readonly onSchemeChange = (): void => {
    this.setTheme({ ...this.engine.theme, dark: this.mediaQuery?.matches ?? true });
  };

  /**
   * Every side effect and pin request so far, oldest first (for tests;
   * mirror of `MockPlatform::calls`).
   */
  readonly calls: MockCall[] = [];
  private readonly fileDragHandlers = new Set<FileDragHandler>();
  /** What `getAppInfo` answers; `setAppInfo` changes it. */
  private appInfo: AppInfo = demoAppInfo();
  /**
   * What the page last reported through `setDockAutoHidden`: the
   * app layer's `DockAutoHidden` for this page's window, which a press of
   * the focus shortcut reads (`requestFocus`).
   */
  private autoHidden = false;
  /**
   * Whether this page's dock window has the keyboard from a press of the
   * focus shortcut: Rust's `DockFocus` session for it while it is
   * the foreground window, so the next press lets go (`requestFocus`). Set
   * by a press that gave it the focus; cleared when the page gives the
   * focus back (`releaseDockFocus`) and when another window comes to the
   * front (`pushForeground`, an activation), which the real dock sees as
   * losing the activation. A click that launches or minimises leaves it
   * set, as it leaves the real dock in front.
   */
  private shortcutFocus = false;
  /** The user's `dock.css` "on disk" (`null`: no file); `writeUserCss` changes it. */
  private userCssFile: MockUserCssFile;
  /** What the last read of it gave: at construction, then each `reloadUserCss`. */
  private userCssRead: UserCss;

  constructor(options: MockOptions = {}) {
    this.userCssFile = options.userCss ?? null;
    this.userCssRead = userCssFrom(this.userCssFile);
    const scenario = SCENARIOS[options.scenario ?? 'default']();
    if (options.position !== undefined) {
      scenario.settings.position = options.position;
    }
    this.latency = options.latencyMs ?? [20, 80];
    this.identityDelayMs = options.identityDelayMs ?? 300;
    this.logging = options.log ?? true;
    this.viewportHeight = options.viewportHeight;
    this.viewportWidth = options.viewportWidth;
    this.pageMonitor = options.monitor ?? null;
    this.random = mulberry32(options.seed ?? 0x5eed);

    // In a browser the OS theme is the browser's colour scheme, so Playwright's
    // `colorScheme` emulation drives light/dark exactly like Windows would.
    this.mediaQuery =
      typeof window !== 'undefined' && typeof window.matchMedia === 'function'
        ? window.matchMedia('(prefers-color-scheme: dark)')
        : null;
    const theme: ThemeInfo = { ...DEFAULT_THEME, dark: this.mediaQuery?.matches ?? true };
    this.mediaQuery?.addEventListener('change', this.onSchemeChange);

    // `MockPlatform::start` delivers these in this order.
    // Validated as `Settings::from_json` validates the file it loads.
    this.engine = new Engine(validateSettings(scenario.settings), DEFAULT_THEME);
    this.engine.setMonitors(scenario.monitors);
    this.engine.setTheme(theme);
    this.engine.setRecycleBin(scenario.recycleBin);
    this.engine.snapshotWindows(scenario.windows);
    this.engine.setForeground(scenario.foreground);
    this.engine.setPower(options.power ?? DEMO_POWER);
    this.engine.setClockFormat(options.clockFormat ?? NO_CLOCK_FORMAT);
    this.engine.setVolume(options.volume === undefined ? DEMO_VOLUME : options.volume);

    this.scheduleIdentityResolution();
  }

  /** Stop timers and OS listeners (tests, hot reload). */
  dispose(): void {
    for (const t of this.timers) {
      clearTimeout(t);
    }
    this.timers.clear();
    if (this.identityTimer !== null) {
      clearTimeout(this.identityTimer);
      this.identityTimer = null;
    }
    this.mediaQuery?.removeEventListener('change', this.onSchemeChange);
  }

  // --- Backend: queries

  getDockState(): Promise<DockState> {
    return this.command(() => this.engine.snapshot());
  }

  getSettings(): Promise<Settings> {
    return this.command(() => structuredClone(this.engine.settings));
  }

  /**
   * `DockService::update_settings`: the pins and hidden apps stay as their
   * own commands left them, whatever `settings` says (IPC v1.26).
   */
  updateSettings(settings: Settings): Promise<Settings> {
    return this.command(() => {
      const { pinned, hiddenApps } = this.engine.settings;
      return structuredClone(this.applySettings({ ...settings, pinned, hiddenApps }));
    });
  }

  getTheme(): Promise<ThemeInfo> {
    return this.command(() => ({ ...this.engine.theme }));
  }

  getMonitors(): Promise<MonitorInfo[]> {
    return this.command(() => structuredClone(this.engine.monitors));
  }

  // --- Backend: intents

  /**
   * `primary_click`, in this page's dock window (`pageLabel`): with a dock
   * per monitor and `showOnlyCurrentMonitor` on, only the windows on its
   * monitor take part, once its panel has registered it there.
   */
  primaryClick(itemKey: string): Promise<void> {
    return this.command(() => {
      const action = this.engine.primaryAction(itemKey, this.pageLabel);
      switch (action.type) {
        case 'launch':
          this.doLaunch(action.appId);
          return;
        case 'activate':
          this.doWindowAction(action.windowId, 'activate');
          return;
        case 'minimize':
          this.doWindowAction(action.windowId, 'minimize');
          return;
        case 'shell':
          this.doShellAction(action.action);
          return;
        case 'none':
          throw `unknown dock item: ${itemKey}`;
      }
    });
  }

  /**
   * `scroll_item`, in this page's dock window (`pageLabel`):
   * recorded, then what the engine decides (`Engine.scrollAction`): with
   * `cycleWindows` the chosen demo window is activated and becomes the
   * foreground one; otherwise nothing. Over the volume item, whatever the
   * setting, every notch steps the volume (`stepVolume`, recorded),
   * as the service has no gate for it either. An unknown key rejects with the
   * Rust error's words. The service's 150 ms gate is not mirrored: every
   * notch the page sends acts, so a test does not depend on its timing.
   */
  scrollItem(itemKey: string, direction: ScrollDirection): Promise<void> {
    return this.command(() => {
      this.record({ type: 'scrollItem', itemKey, direction });
      const effect = this.engine.scrollAction(itemKey, this.pageLabel, direction);
      switch (effect.type) {
        case 'activate':
          this.log(`scroll ${direction} over ${itemKey}: activate ${effect.windowId}`);
          this.doWindowAction(effect.windowId, 'activate');
          return;
        case 'stepVolume':
          this.doVolumeStep(effect.direction);
          return;
        case 'nothing':
          return;
        case 'unknownItem':
          throw `unknown dock item: ${itemKey}`;
      }
    });
  }

  windowAction(windowId: WindowId, action: WindowAction): Promise<void> {
    return this.command(() => this.doWindowAction(windowId, action));
  }

  launchApp(appId: AppId): Promise<void> {
    return this.command(() => this.doLaunch(appId));
  }

  pinApp(appId: AppId): Promise<void> {
    return this.command(() => {
      const launch = this.engine.identities.get(appId)?.launch ?? null;
      if (launch === null) {
        throw `app "${appId}" has no launch target`;
      }
      if (this.engine.isPinned(appId)) {
        return;
      }
      const settings = structuredClone(this.engine.settings);
      settings.pinned.push({ id: appId, label: null, launch });
      // Validated like every change (`applySettings`): a hidden app pinned is shown.
      this.applySettings(settings);
    });
  }

  pinTarget(target: LaunchTarget): Promise<AppId> {
    return this.command(() => {
      this.record({ type: 'pinTarget', target: structuredClone(target) });
      const identity = identityForTarget(target);
      if (!this.engine.isPinned(identity.id)) {
        // Make the identity known before the new item renders.
        this.engine.setIdentity(identity.id, identity);
        const settings = structuredClone(this.engine.settings);
        settings.pinned.push({ id: identity.id, label: null, launch: identity.launch ?? target });
        this.applySettings(settings);
      }
      return identity.id;
    });
  }

  unpinApp(appId: AppId): Promise<void> {
    return this.command(() => {
      if (!this.engine.isPinned(appId)) {
        throw `app "${appId}" is not pinned`;
      }
      const settings = structuredClone(this.engine.settings);
      settings.pinned = settings.pinned.filter((p) => p.id !== appId);
      this.applySettings(settings);
    });
  }

  /**
   * `hide_app`: `{ id, name, path, iconKey }` with the name the
   * app's item shows now (the id's file name when that is blank) and the
   * program's path and icon key `Engine.hiddenAppFacts` finds,
   * saved like a pin. Hiding a hidden app again changes nothing; a pinned
   * app and one the engine knows nothing of are refused with the Rust
   * error's words.
   */
  hideApp(appId: AppId): Promise<void> {
    return this.command(() => {
      if (this.engine.isPinned(appId)) {
        throw `app "${appId}" is pinned; unpin it to hide it`;
      }
      if (this.engine.isHidden(appId)) {
        return;
      }
      const name = this.engine.appName(appId);
      if (name === null) {
        throw `unknown app: "${appId}"`;
      }
      this.log(`hide ${appId}`);
      const settings = structuredClone(this.engine.settings);
      settings.hiddenApps.push({
        id: appId,
        name: name.trim() === '' ? fallbackDisplayName(appId) : name,
        ...this.engine.hiddenAppFacts(appId),
      });
      this.applySettings(settings);
    });
  }

  /** `unhide_app`: an id that is not hidden is no error and changes nothing. */
  unhideApp(appId: AppId): Promise<void> {
    return this.command(() => {
      if (!this.engine.isHidden(appId)) {
        return;
      }
      this.log(`show ${appId}`);
      const settings = structuredClone(this.engine.settings);
      settings.hiddenApps = settings.hiddenApps.filter((h) => h.id !== appId);
      this.applySettings(settings);
    });
  }

  /**
   * `reorder_pinned`: recorded; an order that changes nothing saves and
   * sends nothing, as `mutate_settings` does when its edit says it changed
   * nothing.
   */
  reorderPinned(order: AppId[]): Promise<void> {
    return this.command(() => {
      this.record({ type: 'reorderPinned', order: [...order] });
      const settings = structuredClone(this.engine.settings);
      settings.pinned = reorderPinned(settings.pinned, order);
      if (JSON.stringify(settings.pinned) === JSON.stringify(this.engine.settings.pinned)) {
        return;
      }
      this.applySettings(settings);
    });
  }

  getIcon(iconKey: IconKey, size: number): Promise<string | null> {
    return this.command(() => {
      if (isShellIconKey(iconKey) || !iconKey.startsWith('mock:')) {
        return null;
      }
      return mockIconDataUrl(iconKey, this.letterFor(iconKey), Math.round(clamp(size, 16, 512)));
    });
  }

  /**
   * Whether a report the page measured for `edge` belongs to where the
   * app layer has the window (`DockWindowState::stale_for`): the
   * mock's window moves with `Settings.position` at once, so a report for
   * another edge was measured before the page saw the change, and is
   * dropped (logged; it stays recorded in `calls`).
   */
  private reportIsCurrent(command: string, edge: DockPosition): boolean {
    const placed = this.engine.settings.position;
    if (edge === placed) {
      return true;
    }
    this.log(
      `${command} measured for the ${edge} edge dropped; the window is on the ${placed} edge`,
    );
    return false;
  }

  /** `set_dock_content_size`: a browser window is not resized; recorded and logged. */
  setDockContentSize(width: number, height: number, edge: DockPosition): Promise<void> {
    return this.command(() => {
      this.record({ type: 'setDockContentSize', width, height, edge });
      if (this.reportIsCurrent('set_dock_content_size', edge)) {
        this.log(`set_dock_content_size(${Math.round(width)} × ${Math.round(height)}, ${edge})`);
      }
    });
  }

  /** `set_dock_hit_region`: a browser has no OS hit-testing; recorded and logged. */
  setDockHitRegion(panel: Rect, envelope: Rect, edge: DockPosition): Promise<void> {
    return this.command(() => {
      this.record({
        type: 'setDockHitRegion',
        panel: { ...panel },
        envelope: { ...envelope },
        edge,
      });
      if (this.reportIsCurrent('set_dock_hit_region', edge)) {
        this.log(
          `set_dock_hit_region(panel ${panel.width}×${panel.height} at ${panel.x},${panel.y}; ` +
            `envelope ${envelope.width}×${envelope.height} at ${envelope.x},${envelope.y}; ${edge})`,
        );
      }
    });
  }

  /** The label of the dock window this backend's page is (`dock`, or `dock-<id>`). */
  get pageLabel(): string {
    return dockLabel(this.pageMonitor, this.engine.primaryMonitorId());
  }

  /** The monitor this backend's page sits on, as the app layer would place its window. */
  private pageMonitorInfo(): MonitorInfo | undefined {
    const monitors = this.engine.monitors;
    return (
      (this.pageMonitor === null ? undefined : monitors.find((m) => m.id === this.pageMonitor)) ??
      monitors.find((m) => m.isPrimary) ??
      monitors[0]
    );
  }

  /**
   * `set_dock_panel_rect`: recorded with its provenance;
   * rejected like Rust for a non-positive or non-finite viewport; dropped
   * when measured for another edge than the window's (`reportIsCurrent`);
   * else re-anchored from the viewport it carries to the window's viewport
   * now (`reanchorPanel`; `MockOptions.viewportHeight` and `viewportWidth`,
   * else `window.innerWidth` × `innerHeight` read per call, else the work
   * area's size), converted to the page's monitor's screen coordinates the
   * way the app layer converts it for the real window on the configured edge
   * (`panelOnScreen`, at that monitor's scale), and handed to the engine
   * under this dock window's label and monitor, which emits a state
   * only when the docks' facts change.
   */
  setDockPanelRect(rect: Rect, edge: DockPosition, viewport: Viewport): Promise<void> {
    return this.command(() => {
      this.record({
        type: 'setDockPanelRect',
        rect: { ...rect },
        edge,
        viewport: { width: viewport.width, height: viewport.height },
      });
      const invalid = viewportError(viewport);
      if (invalid !== null) {
        throw invalid;
      }
      if (!this.reportIsCurrent('set_dock_panel_rect', edge)) {
        return;
      }
      const monitor = this.pageMonitorInfo();
      const workArea = monitor?.workArea ?? { x: 0, y: 0, width: 0, height: 0 };
      const browser = typeof window !== 'undefined';
      const now = {
        width:
          this.viewportWidth ??
          (browser && window.innerWidth > 0 ? window.innerWidth : workArea.width),
        height:
          this.viewportHeight ??
          (browser && window.innerHeight > 0 ? window.innerHeight : workArea.height),
      };
      const css = reanchorPanel(rect, edge, viewport, now);
      const screen = panelOnScreen(css, workArea, now, edge, monitor?.scaleFactor ?? 1);
      const label = this.pageLabel;
      this.log(
        `set_dock_panel_rect(${rect.width}×${rect.height} at ${rect.x},${rect.y} in ` +
          `${viewport.width}×${viewport.height}, ${edge}) -> ${css.x},${css.y} in ` +
          `${now.width}×${now.height} -> ` +
          `${screen.x},${screen.y} on screen (${label} on ${monitor?.id ?? 'no monitor'})`,
      );
      if (this.engine.setDockPanel(label, monitor?.id ?? null, screen)) {
        this.emitState();
      }
    });
  }

  /**
   * `set_dock_auto_hidden`: the app layer only keeps the flag for
   * its `toggle_dock`, which has no counterpart in a browser; recorded and
   * logged, so tests can see each change the UI reported.
   */
  setDockAutoHidden(hidden: boolean): Promise<void> {
    return this.command(() => {
      this.record({ type: 'setDockAutoHidden', hidden });
      this.autoHidden = hidden;
      this.log(`set_dock_auto_hidden(${hidden})`);
    });
  }

  shellAction(action: ShellAction): Promise<void> {
    return this.command(() => this.doShellAction(action));
  }

  /**
   * `DockService::delete_to_recycle_bin` over `MockPlatform`: an empty list
   * does nothing; otherwise the paths are recorded, every plain absolute
   * one counts as existing and goes into the bin, and the service's refresh
   * shows it (the icon turns full). Nothing usable rejects like Rust.
   */
  deleteToRecycleBin(paths: string[]): Promise<void> {
    return this.command(() => {
      if (paths.length === 0) {
        return;
      }
      this.record({ type: 'deleteToRecycleBin', paths: [...paths] });
      this.log(`delete_to_recycle_bin(${paths.join(', ')})`);
      const plan = recyclePlan(paths);
      if (plan.recycle.length === 0) {
        throw nothingToRecycle(plan);
      }
      this.pushRecycleBin({
        itemCount: this.engine.recycleBin.itemCount + plan.recycle.length,
        sizeBytes: this.engine.recycleBin.sizeBytes,
      });
    });
  }

  openSettings(): Promise<void> {
    return this.command(() => {
      this.record({ type: 'openSettings' });
      this.log('open_settings()');
    });
  }

  /** `DockService::reset_settings`: the defaults, pins and hidden apps included, validated and applied. */
  resetSettings(): Promise<Settings> {
    return this.command(() => {
      return structuredClone(this.applySettings(defaultSettings()));
    });
  }

  quit(): Promise<void> {
    return this.command(() => {
      this.record({ type: 'quit' });
      this.log('quit()');
    });
  }

  /** `get_app_info`: the demo facts, unless `setAppInfo` changed them. */
  getAppInfo(): Promise<AppInfo> {
    return this.command(() => ({ ...this.appInfo }));
  }

  /** `open_app_folder`: a browser opens no Explorer; the request is recorded. */
  openAppFolder(folder: AppFolder): Promise<void> {
    return this.command(() => {
      this.record({ type: 'openAppFolder', folder });
      this.log(`open_app_folder(${folder})`);
    });
  }

  on<E extends BackendEvent>(event: E, handler: EventHandler<E>): Unsubscribe {
    let set = this.listeners[event] as Set<EventHandler<E>> | undefined;
    if (!set) {
      set = new Set<EventHandler<E>>();
      (this.listeners as Record<string, unknown>)[event] = set;
    }
    set.add(handler);
    return () => {
      set.delete(handler);
    };
  }

  /** File drags arrive only through `simulateFileDrag`: a browser page gets no OLE drags. */
  onFileDrag(handler: FileDragHandler): Unsubscribe {
    this.fileDragHandlers.add(handler);
    return () => {
      this.fileDragHandlers.delete(handler);
    };
  }

  // --- Backend: native context menus. A browser has none; the mock
  // records the request (Rust would build and show the menu itself).

  /** `show_item_menu`: an unknown key rejects like Rust; separators have no menu. */
  showItemMenu(itemKey: string, x: number, y: number): Promise<void> {
    return this.command(() => {
      const item = this.engine.snapshot().items.find((i) => i.key === itemKey);
      if (!item) {
        throw `unknown dock item: ${itemKey}`;
      }
      if (item.kind === 'separator') {
        return;
      }
      this.record({ type: 'showItemMenu', itemKey, x, y });
      this.log(`show_item_menu(${itemKey}, ${x}, ${y})`);
    });
  }

  showDockMenu(x: number, y: number): Promise<void> {
    return this.command(() => {
      this.record({ type: 'showDockMenu', x, y });
      this.log(`show_dock_menu(${x}, ${y})`);
    });
  }

  /**
   * `release_dock_focus`: Rust would bring back the window that was
   * in front when the focus shortcut was pressed; a browser has no
   * foreground to give back, so the call is recorded and logged only.
   */
  releaseDockFocus(): Promise<void> {
    return this.command(() => {
      this.record({ type: 'releaseDockFocus' });
      this.shortcutFocus = false;
      this.log('release_dock_focus');
    });
  }

  // --- Backend: the user's dock.css (docs/ipc/themes.md)

  /** `get_user_css`: the last read (at construction from `MockOptions.userCss`, or the last reload). */
  getUserCss(): Promise<UserCss> {
    return this.command(() => ({ ...this.userCssRead }));
  }

  /**
   * `reload_user_css`: read the file again (`readUserCss`), keep the
   * result, send it as `dock://user-css` (to this page: the mock serves one
   * page) and return it.
   */
  reloadUserCss(): Promise<UserCss> {
    return this.command(() => {
      this.record({ type: 'reloadUserCss' });
      this.userCssRead = userCssFrom(this.userCssFile);
      const { css, problem } = this.userCssRead;
      this.log(
        `reload_user_css(): ${css === null ? `no CSS (${problem})` : `${css.length} chars`}`,
      );
      this.emit('dock://user-css', { ...this.userCssRead });
      return { ...this.userCssRead };
    });
  }

  /**
   * `open_user_css`: a browser opens no editor; the file is created from
   * the (stand-in) template if there is none, and the request is recorded
   * with whether it was. Nothing is read.
   */
  openUserCss(): Promise<void> {
    return this.command(() => {
      const created = this.userCssFile === null;
      if (created) {
        this.userCssFile = MOCK_USER_CSS_TEMPLATE;
      }
      this.record({ type: 'openUserCss', created });
      this.log(`open_user_css()${created ? ': wrote the template' : ''}`);
    });
  }

  // --- Backend: planned for M2 (docs/ipc/commands.md); the mock only records it.

  setDockHover(hovering: boolean): Promise<void> {
    return this.command(() => {
      this.record({ type: 'setDockHover', hovering });
    });
  }

  // --- Scripting hooks (mirror of `MockPlatform::push`), for tests and demos.

  /**
   * Deliver a file drag event, exactly what `TauriBackend.onFileDrag` would
   * hand over for an OLE drag from Explorer (positions in CSS px of the
   * viewport). Synchronous, like Tauri's events arriving between frames.
   */
  simulateFileDrag(event: FileDragEvent): void {
    this.log(`file drag ${event.type}${'paths' in event ? ` ${event.paths.join(', ')}` : ''}`);
    for (const handler of [...this.fileDragHandlers]) {
      handler(structuredClone(event));
    }
  }

  /** Make the next command reject with `message` (what a Rust error looks like across IPC). */
  failNextCommand(message: string): void {
    this.failNext = message;
  }

  /**
   * Change what `get_app_info` answers from now on: a run with file
   * logging off is `{ logPath: '' }`. The settings page asks once at start,
   * so set it before the page's store starts.
   */
  setAppInfo(info: Partial<AppInfo>): void {
    this.appInfo = { ...this.appInfo, ...info };
  }

  /**
   * Change the user's `dock.css` on "disk" (`null` deletes it; bytes
   * or `{ unreadable }` make a file Stem cannot use), as an editor
   * would: nothing is read until the next `reloadUserCss`.
   */
  writeUserCss(file: MockUserCssFile): void {
    this.userCssFile = file;
  }

  /** The mock's `dock.css` on "disk" (`null`: none), e.g. after `openUserCss` wrote the template. */
  userCssOnDisk(): MockUserCssFile {
    return this.userCssFile;
  }

  pushWindow(raw: RawWindow): void {
    if (this.engine.upsertWindow(raw)) {
      this.emitState();
    }
    this.scheduleIdentityResolution();
  }

  pushWindowRemoved(id: WindowId): void {
    if (this.engine.removeWindow(id)) {
      this.emitState();
    }
  }

  /**
   * A window moved or resized (`MockPlatform::move_window`): its new
   * frame in screen px (`null` for one the platform could not read), reported
   * the way the events thread does after a move, with one `WindowUpdated`.
   * A state goes out only when `overlapped` flips. Unknown ids are ignored,
   * like a move of a window the thread does not track.
   */
  moveWindow(id: WindowId, rect: Rect | null): void {
    const tracked = this.engine.windows.get(id);
    if (!tracked) {
      return;
    }
    this.log(
      `window ${id} moved to ${rect === null ? 'an unreadable frame' : JSON.stringify(rect)}`,
    );
    if (this.engine.upsertWindow({ ...tracked.raw, rect: rect === null ? null : { ...rect } })) {
      this.emitState();
    }
  }

  /**
   * Where the engine believes the panel of the dock window `label` rests
   * (screen px; this page's window by default), or `null` before its first
   * report, while it is hidden, or for a label never registered.
   */
  dockPanel(label: string = this.pageLabel): Rect | null {
    const rect = this.engine.panels.get(label)?.rect ?? null;
    return rect === null ? null : { ...rect };
  }

  /**
   * A whole settings document, the pins and hidden apps included, as the
   * settings file gives one at start (validated, `dock://settings` to
   * whoever listens): for tests of a list that no command made, such as a
   * pin the dock has not resolved yet. No command does this at runtime:
   * `updateSettings` keeps the lists (IPC v1.26).
   */
  loadSettings(settings: Settings): void {
    this.applySettings(settings);
  }

  /**
   * The settings window was asked to close: what Rust tells its
   * page before it destroys it (`dock://settings-closing`). Playwright's
   * way to close the settings page without losing what it holds back.
   */
  requestSettingsClose(): void {
    this.emit('dock://settings-closing', null);
  }

  /**
   * Put the dock on `position` the way a settings change does
   * (validated, `dock://settings` to whoever listens): the browser demo's
   * and Playwright's way to show each edge. The dock page calls it at load
   * for `?position=` (`demo.ts`), before it has asked for anything.
   */
  setPosition(position: DockPosition): void {
    const settings = structuredClone(this.engine.settings);
    settings.position = position;
    this.applySettings(settings);
  }

  /**
   * The tray or the toggle shortcut was pressed while the dock window was
   * visible with its panel slid out by auto-hide: what
   * `windows::toggle_dock` emits to that window instead of hiding it. One
   * `dock://show` with a `null` payload, whatever the UI last reported.
   */
  requestShow(): void {
    this.log('dock reveal requested');
    this.emit('dock://show', null);
  }

  /**
   * The focus shortcut was pressed (docs/ipc/keyboard-entry.md):
   * what `windows::on_focus_shortcut` sends this page's dock window. While
   * the dock has the keyboard from an earlier press (`shortcutFocus`), one
   * `dock://release-focus`: the page does what Escape does. Otherwise, once
   * Rust has made it the foreground window: a panel the page last reported
   * slid out by auto-hide is asked back first (`dock://show`, as
   * `requestShow` does), then one `dock://focus`. Payloads are `null`. A
   * browser has no foreground to take, so nothing else happens.
   */
  requestFocus(): void {
    if (this.shortcutFocus) {
      this.log('dock focus release requested');
      this.emit('dock://release-focus', null);
      return;
    }
    this.shortcutFocus = true;
    if (this.autoHidden) {
      this.log('dock reveal requested');
      this.emit('dock://show', null);
    }
    this.log('dock focus requested');
    this.emit('dock://focus', null);
  }

  pushForeground(id: WindowId | null): void {
    // Another window (or the desktop) came to the front: the dock window
    // lost the activation, and with it the focus shortcut's keyboard.
    this.shortcutFocus = false;
    if (this.engine.setForeground(id)) {
      this.emitState();
    }
  }

  pushAttention(id: WindowId, flashing: boolean): void {
    if (this.engine.setAttention(id, flashing)) {
      this.emitState();
    }
  }

  /**
   * The platform's fullscreen verdict changed (`FullscreenChanged`):
   * a game, a browser after F11, a video or a slideshow took the screen of
   * `monitor` (`null` when the platform could not tell, which lands
   * on the primary monitor's dock), or gave it back. One `dock://state`
   * when some dock's fact changes.
   */
  setFullscreen(active: boolean, monitor: MonitorId | null = null): void {
    if (this.engine.setFullscreen(active, monitor)) {
      this.log(`fullscreen ${active ? `on (${monitor ?? 'unknown monitor'})` : 'off'}`);
      this.emitState();
    }
  }

  /**
   * The power source, the battery or battery saver changed: what
   * the events thread reports as `PowerChanged`, only when it differs from
   * the last reading. One `dock://state` when it does and the battery item
   * shows it (or comes or goes); otherwise it is only kept.
   */
  setPower(status: PowerStatus): void {
    if (this.engine.setPower(status)) {
      this.log(`power ${batteryTitle(status)}${status.hasBattery ? '' : ' (no battery)'}`);
      this.emitState();
    }
  }

  /**
   * The volume, the mute or the default output device changed outside Stem
   * (the volume keys, the Quick Settings slider, headphones plugged
   * in; `null`: no output device any more): what the volume thread reports
   * as `VolumeChanged`, only when it differs from the last reading. One
   * `dock://state` when it does and the volume item is shown; otherwise it
   * is only kept.
   */
  setVolume(status: VolumeStatus | null): void {
    if (this.engine.setVolume(status)) {
      this.log(`volume ${volumeTitle(status)}`);
      this.emitState();
    }
  }

  /**
   * The volume item's *Mute* (`true`) or *Unmute* (`false`) menu entry:
   * what Rust runs from its native menu (`set_volume_muted`),
   * which the mock has none of. Recorded; rejects without an output
   * device, with the platform's words.
   */
  chooseVolumeMute(muted: boolean): Promise<void> {
    return this.command(() => this.platformSetMuted(muted));
  }

  /**
   * The user changed the regional format or the short time format:
   * what the events thread reports as `ClockFormatChanged` after a
   * `WM_SETTINGCHANGE` for `intl`, only when it differs from the last
   * reading. One `dock://state` when it does.
   */
  setClockFormat(format: ClockFormat): void {
    if (this.engine.setClockFormat(format)) {
      this.log(
        `clock format ${format.locale ?? 'unknown locale'}, ${
          format.hour12 === null ? 'unknown hour cycle' : format.hour12 ? '12-hour' : '24-hour'
        }`,
      );
      this.emitState();
    }
  }

  pushRecycleBin(state: RecycleBinState): void {
    if (this.engine.setRecycleBin(state)) {
      this.emitState();
    }
  }

  /**
   * The OS theme or accent changed (`ThemeChanged`): `dock://theme` when it
   * differs from the last one; the snapshot does not carry the theme, so
   * no `dock://state` (as Rust).
   */
  setTheme(theme: ThemeInfo): void {
    const changed = JSON.stringify(theme) !== JSON.stringify(this.engine.theme);
    const stateChanged = this.engine.setTheme(theme);
    if (changed) {
      this.emit('dock://theme', { ...theme });
    }
    if (stateChanged) {
      this.emitState();
    }
  }

  /** Resolve pending identities now instead of after the configured delay. */
  resolveIdentitiesNow(): void {
    if (this.identityTimer !== null) {
      clearTimeout(this.identityTimer);
      this.identityTimer = null;
    }
    this.resolveIdentities();
  }

  // --- internals

  /**
   * A command runs after its own latency and after every command issued
   * before it has run: Tauri delivers a webview's commands in order, so a
   * later report (a hit region, the auto-hide flag) never lands before an
   * earlier one, however the latencies were drawn; `calls` is in issue order.
   */
  private command<T>(job: () => T): Promise<T> {
    const failure = this.failNext;
    this.failNext = null;
    const run =
      failure === null
        ? job
        : (): T => {
            throw failure;
          };
    const [lo, hi] = this.latency;
    const ms = lo + (hi - lo) * this.random();
    const latency =
      ms <= 0
        ? Promise.resolve()
        : new Promise<void>((resolve) => {
            const timer = setTimeout(() => {
              this.timers.delete(timer);
              resolve();
            }, ms);
            this.timers.add(timer);
          });
    // `run` may throw: commands fail with a `string` across IPC
    // (docs/ipc/) and the mock throws strings for the same reason; the
    // rejection is the caller's, the queue goes on.
    const result = Promise.all([latency, this.queue]).then(run);
    this.queue = result.then(
      () => undefined,
      () => undefined,
    );
    return result;
  }

  private emit<E extends BackendEvent>(event: E, payload: BackendEvents[E]): void {
    const set = this.listeners[event] as Set<EventHandler<E>> | undefined;
    if (!set) {
      return;
    }
    for (const handler of set) {
      handler(payload);
    }
  }

  private emitState(): void {
    this.emit('dock://state', this.engine.snapshot());
  }

  private record(call: MockCall): void {
    this.calls.push(call);
  }

  private log(message: string): void {
    if (this.logging) {
      console.info(`[mock] ${message}`);
    }
  }

  /**
   * The one way the mock's settings change, as `mutate_settings` is Rust's:
   * `next` is validated (`validateSettings`, `Settings::validate`), handed
   * to the engine, and announced: the settings event always, a state event
   * if items changed. Returns the validated document (the engine's; clone
   * it before handing it out).
   */
  private applySettings(next: Settings): Settings {
    const settings = validateSettings(next);
    const changed = this.engine.setSettings(settings);
    this.emit('dock://settings', structuredClone(settings));
    if (changed) {
      this.emitState();
    }
    this.scheduleIdentityResolution();
    return settings;
  }

  private launchTargetFor(appId: AppId): LaunchTarget | null {
    return (
      this.engine.settings.pinned.find((p) => p.id === appId)?.launch ??
      this.engine.identities.get(appId)?.launch ??
      null
    );
  }

  private doLaunch(appId: AppId): void {
    const target = this.launchTargetFor(appId);
    if (target === null) {
      throw `app "${appId}" has no launch target`;
    }
    this.record({ type: 'launch', target });
    this.log(`launch ${describeTarget(target)}`);
    this.emit('dock://item-bounce', { itemKey: `app:${appId}` });
  }

  /** `MockPlatform::window_action` plus the engine's handling of the events it emits. */
  private doWindowAction(windowId: WindowId, action: WindowAction): void {
    const tracked = this.engine.windows.get(windowId);
    if (!tracked) {
      throw `window not found: ${windowId}`;
    }
    this.record({ type: 'windowAction', windowId, action });
    let changed = false;
    switch (action) {
      case 'activate':
        changed = this.engine.upsertWindow({ ...tracked.raw, isMinimized: false });
        changed = this.engine.setForeground(windowId) || changed;
        // The window takes the foreground from the dock.
        this.shortcutFocus = false;
        break;
      case 'minimize':
        changed = this.engine.upsertWindow({ ...tracked.raw, isMinimized: true });
        if (this.engine.foreground === windowId) {
          changed = this.engine.setForeground(null) || changed;
        }
        break;
      case 'restore':
        changed = this.engine.upsertWindow({ ...tracked.raw, isMinimized: false });
        break;
      case 'close':
        changed = this.engine.removeWindow(windowId);
        break;
    }
    if (changed) {
      this.emitState();
    }
  }

  /**
   * `Platform::step_volume` (the mock platform's, which the service
   * calls once per notch): recorded, then the reading now becomes what
   * `volumeStep` makes of it, reported like an outside change; nothing
   * without an output device or at the end of the range.
   */
  private doVolumeStep(direction: ScrollDirection): void {
    this.record({ type: 'stepVolume', direction });
    const current = this.engine.volume;
    if (current === null) {
      return;
    }
    this.setVolume(volumeStep(current, direction));
  }

  /**
   * `MockPlatform::set_volume_muted`: recorded, then applied to the
   * scripted device and reported like an outside change; an error without
   * a device.
   */
  private platformSetMuted(muted: boolean): void {
    this.record({ type: 'setVolumeMuted', muted });
    const current = this.engine.volume;
    if (current === null) {
      throw 'no audio output device';
    }
    this.setVolume({ ...current, muted });
  }

  private doShellAction(action: ShellAction): void {
    this.record({ type: 'shell', action });
    this.log(`shell action ${action}`);
    if (action === 'emptyRecycleBin') {
      this.pushRecycleBin({ itemCount: 0, sizeBytes: 0 });
    }
  }

  private scheduleIdentityResolution(): void {
    if (this.engine.unresolvedApps().length === 0 || this.identityTimer !== null) {
      return;
    }
    if (this.identityDelayMs <= 0) {
      this.resolveIdentities();
      return;
    }
    this.identityTimer = setTimeout(() => {
      this.identityTimer = null;
      this.resolveIdentities();
    }, this.identityDelayMs);
  }

  /** `DockService::resolve_identity` for every unresolved app, then one state event. */
  private resolveIdentities(): void {
    let changed = false;
    for (const appId of this.engine.unresolvedApps()) {
      changed = this.engine.setIdentity(appId, this.resolveIdentity(appId)) || changed;
    }
    if (changed) {
      this.emitState();
    }
  }

  private resolveIdentity(appId: AppId): AppIdentity {
    const pinned = this.engine.settings.pinned.find((p) => p.id === appId);
    const raw = this.engine.firstWindowOf(appId);
    let identity: AppIdentity;
    if (raw) {
      identity = identityForWindow(raw);
    } else if (pinned) {
      identity = identityForTarget(pinned.launch);
    } else {
      identity = {
        id: appId,
        displayName: fallbackDisplayName(appId),
        iconKey: null,
        launch: null,
      };
    }
    // The engine groups by the id *it* derived; keep that as the key.
    return { ...identity, id: appId, launch: identity.launch ?? pinned?.launch ?? null };
  }

  /** The letter drawn on a mock icon: the item's title (so labels count), else the identity. */
  private letterFor(iconKey: IconKey): string {
    const title =
      this.engine.snapshot().items.find((i) => i.iconKey === iconKey)?.title ??
      [...this.engine.identities.values()].find((i) => i.iconKey === iconKey)?.displayName ??
      fallbackDisplayName(iconKey.slice('mock:'.length));
    const first = [...title.trim()][0];
    return (first ?? '?').toUpperCase();
  }
}

function describeTarget(target: LaunchTarget): string {
  switch (target.kind) {
    case 'exe':
      return `${target.path}${target.args.length > 0 ? ' ' + target.args.join(' ') : ''}`;
    case 'shortcut':
      return target.path;
    case 'storeApp':
      return `shell:AppsFolder\\${target.appUserModelId}`;
    case 'uri':
      return target.uri;
  }
}

// ---------------------------------------------------------------------------
// 4. Icons (mirror of `color_for` / `solid_png` in mock.rs, as inline SVG)
// ---------------------------------------------------------------------------

/** FNV-1a 64 → hue, exactly as `color_for` in mock.rs, so colours match Rust. */
export function colorFor(key: string): [number, number, number] {
  let hash = 0xcbf29ce484222325n;
  for (const byte of new TextEncoder().encode(key)) {
    hash = BigInt.asUintN(64, (hash ^ BigInt(byte)) * 0x100000001b3n);
  }
  return hslToRgb(Number(hash % 360n), 0.65, 0.55);
}

function hslToRgb(h: number, s: number, l: number): [number, number, number] {
  const c = (1 - Math.abs(2 * l - 1)) * s;
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
  const m = l - c / 2;
  // Six 60° hue sectors, same integer truncation as the Rust `match h as u32`.
  const sector = Math.trunc(h);
  const [r, g, b]: [number, number, number] =
    sector < 60
      ? [c, x, 0]
      : sector < 120
        ? [x, c, 0]
        : sector < 180
          ? [0, c, x]
          : sector < 240
            ? [0, x, c]
            : sector < 300
              ? [x, 0, c]
              : [c, 0, x];
  return [Math.trunc((r + m) * 255), Math.trunc((g + m) * 255), Math.trunc((b + m) * 255)];
}

/**
 * A rounded square (radius 22 % like `solid_png`) in the key's colour with the
 * app's initial letter, as an SVG data URL. Deterministic, so screenshots are
 * stable.
 */
export function mockIconDataUrl(key: string, letter: string, size: number): string {
  const [r, g, b] = colorFor(key);
  const text = letter.replace(/[<>&"]/g, '');
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 64 64">` +
    `<defs><linearGradient id="g" x1="0" y1="0" x2="0" y2="1">` +
    `<stop offset="0" stop-color="#fff" stop-opacity="0.22"/>` +
    `<stop offset="1" stop-color="#000" stop-opacity="0.12"/></linearGradient></defs>` +
    `<rect x="2" y="2" width="60" height="60" rx="14" fill="rgb(${r},${g},${b})"/>` +
    `<rect x="2" y="2" width="60" height="60" rx="14" fill="url(#g)"/>` +
    `<text x="32" y="33" dominant-baseline="central" text-anchor="middle" ` +
    `font-family="Segoe UI, Helvetica Neue, Arial, sans-serif" font-size="34" font-weight="600" ` +
    `fill="#fff">${text}</text></svg>`;
  return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
}
