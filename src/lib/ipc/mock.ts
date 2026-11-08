/**
 * `MockBackend`: the whole IPC contract (docs/ipc/ v1.9) without Rust, for
 * browser runs and Playwright screenshots.
 *
 * It mirrors, deliberately in this one file so the two sides can be compared,
 * the Rust pieces the UI would otherwise be talking to:
 *
 * 1. the demo scenario of `crates/stem-platform/src/mock.rs` (windows,
 *    monitor, recycle bin, deterministic icon colours);
 * 2. the ordering and click rules of `crates/stem-core/src/engine.rs`;
 * 3. the command semantics and lazy identity resolution of
 *    `crates/stem-service/src/lib.rs`;
 * 4. which dropped paths go to the Recycle Bin
 *    (`crates/stem-platform/src/rules/recycle.rs`).
 *
 * Behaviour decisions belong to Rust (ADR-0006). Nothing here is used by the
 * UI directly; the UI only ever sees `DockState` snapshots and `Settings`.
 */

import type { AppId } from '$lib/ipc/generated/AppId';
import type { AppIdentity } from '$lib/ipc/generated/AppIdentity';
import type { DockItem } from '$lib/ipc/generated/DockItem';
import type { DockItemKind } from '$lib/ipc/generated/DockItemKind';
import type { DockState } from '$lib/ipc/generated/DockState';
import type { IconKey } from '$lib/ipc/generated/IconKey';
import type { LaunchTarget } from '$lib/ipc/generated/LaunchTarget';
import type { MonitorInfo } from '$lib/ipc/generated/MonitorInfo';
import type { PinnedApp } from '$lib/ipc/generated/PinnedApp';
import type { RawWindow } from '$lib/ipc/generated/RawWindow';
import type { Rect } from '$lib/ipc/generated/Rect';
import type { RecycleBinState } from '$lib/ipc/generated/RecycleBinState';
import type { Settings } from '$lib/ipc/generated/Settings';
import type { ShellAction } from '$lib/ipc/generated/ShellAction';
import type { ThemeInfo } from '$lib/ipc/generated/ThemeInfo';
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

export type MockScenario = 'default' | 'many';

export interface MockOptions {
  scenario?: MockScenario;
  /** Simulated command latency range in ms. `[0, 0]` settles in a microtask. */
  latencyMs?: readonly [number, number];
  /** Delay before app identities (names, icons) resolve. `0` resolves synchronously at construction. */
  identityDelayMs?: number;
  /** Seed of the latency PRNG so runs are reproducible. */
  seed?: number;
  /** Report side effects (launches, shell actions, window sizing) with `console.info`. */
  log?: boolean;
}

/**
 * A side effect the mock would have performed on a real OS (mirror of
 * `MockCall`), or a pin request (`pinTarget`, `reorderPinned`: what the UI
 * asked to pin and in which order, so tests can tell one request from
 * several).
 */
export type MockCall =
  | { type: 'windowAction'; windowId: WindowId; action: WindowAction }
  | { type: 'pinTarget'; target: LaunchTarget }
  | { type: 'reorderPinned'; order: AppId[] }
  | { type: 'launch'; target: LaunchTarget }
  | { type: 'deleteToRecycleBin'; paths: string[] }
  | { type: 'shell'; action: ShellAction }
  | { type: 'setDockContentSize'; width: number; height: number }
  | { type: 'setDockHitRegion'; panel: Rect; envelope: Rect }
  | { type: 'showItemMenu'; itemKey: string; x: number; y: number }
  | { type: 'showDockMenu'; x: number; y: number }
  | { type: 'openSettings' }
  | { type: 'setDockHover'; hovering: boolean }
  | { type: 'quit' };

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
    },
    pinned: [],
    system: {
      autostart: false,
      hideWindowsTaskbar: false,
      reserveScreenSpace: false,
      fileLogging: true,
    },
  };
}

const clamp = (v: number, lo: number, hi: number): number => Math.min(hi, Math.max(lo, v));

/** `Settings::validate`: clamp numeric fields, drop empty/duplicate pins. Returns a copy. */
export function validateSettings(input: Settings): Settings {
  const s: Settings = structuredClone(input);
  s.version = 1;
  s.iconSize = clamp(Math.round(s.iconSize), 24, 128);
  s.magnification.maxScale = clamp(s.magnification.maxScale, 1, 2.5);
  s.magnification.rangeIcons = clamp(Math.round(s.magnification.rangeIcons), 1, 6);
  s.autoHide.delayMs = clamp(Math.round(s.autoHide.delayMs), 0, 5000);
  s.appearance.opacity = clamp(s.appearance.opacity, 0, 1);
  s.appearance.cornerRadius = clamp(Math.round(s.appearance.cornerRadius), 0, 64);
  const seen = new Set<AppId>();
  s.pinned = s.pinned.filter((p) => {
    if (p.id === '' || seen.has(p.id)) {
      return false;
    }
    seen.add(p.id);
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

function demoWindow(id: number, exe: string, title: string, minimized: boolean): RawWindow {
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
    monitor: DEMO_MONITOR_ID,
    // The demo has no geometry, like the Rust demo (`rect: None`).
    rect: null,
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
} as const;

function demoWindows(): RawWindow[] {
  return [
    demoWindow(1, EXE.explorer, 'Downloads', false),
    demoWindow(2, EXE.firefox, 'Stem — Mozilla Firefox', false),
    demoWindow(3, EXE.firefox, 'GitHub — Mozilla Firefox', true),
    demoWindow(4, EXE.code, 'lib.rs — Stem', false),
    demoWindow(5, EXE.terminal, 'PowerShell', false),
    demoWindow(6, EXE.spotify, 'Spotify Premium', true),
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
    windows: demoWindows(),
    foreground: 2,
    recycleBin: { itemCount: 3, sizeBytes: 1_234_567 },
    settings,
  };
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
    windows.push(demoWindow(windows.length + 1, exeOf(name), title, minimized));
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
    windows,
    foreground: chrome?.id ?? null,
    recycleBin: { itemCount: 0, sizeBytes: 0 },
    settings,
  };
}

const SCENARIOS: Record<MockScenario, () => Scenario> = {
  default: demoScenario,
  many: manyScenario,
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

/** `ItemAction`: what a primary click should do. */
export type ItemAction =
  | { type: 'launch'; appId: AppId }
  | { type: 'activate'; windowId: WindowId }
  | { type: 'minimize'; windowId: WindowId }
  | { type: 'shell'; action: ShellAction }
  | { type: 'none' };

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
  /** The platform's fullscreen verdict, stored as is. */
  fullscreen = false;
  monitors: MonitorInfo[] = [];
  theme: ThemeInfo;
  recycleBin: RecycleBinState = { itemCount: 0, sizeBytes: 0 };
  private nextSeq = 1;
  revision = 0;

  constructor(settings: Settings, theme: ThemeInfo) {
    this.settings = settings;
    this.theme = theme;
  }

  // --- inputs (each returns whether the snapshot changed; `apply` bumps the revision)

  apply(changed: boolean): boolean {
    this.rebuildRunningOrder();
    if (changed) {
      this.revision += 1;
    }
    return changed;
  }

  snapshotWindows(list: RawWindow[]): boolean {
    const before = JSON.stringify([...this.windows.values()]);
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
    return this.apply(JSON.stringify([...this.windows.values()]) !== before);
  }

  upsertWindow(raw: RawWindow): boolean {
    const t = this.windows.get(raw.id);
    if (t) {
      if (sameWindow(t.raw, raw)) {
        return this.apply(false);
      }
      t.raw = raw;
      return this.apply(true);
    }
    this.windows.set(raw.id, { raw, seq: this.allocSeq(), attention: false });
    return this.apply(true);
  }

  removeWindow(id: WindowId): boolean {
    const removed = this.windows.delete(id);
    if (this.foreground === id) {
      this.foreground = null;
    }
    return this.apply(removed);
  }

  setForeground(id: WindowId | null): boolean {
    const next = id !== null && this.windows.has(id) ? id : null;
    let changed = this.foreground !== next;
    this.foreground = next;
    const t = next !== null ? this.windows.get(next) : undefined;
    if (t?.attention) {
      // Activating a window clears its attention request.
      t.attention = false;
      changed = true;
    }
    return this.apply(changed);
  }

  setAttention(id: WindowId, flashing: boolean): boolean {
    const t = this.windows.get(id);
    if (!t) {
      return this.apply(false);
    }
    const changed = t.attention !== flashing;
    t.attention = flashing;
    return this.apply(changed);
  }

  /** `PlatformEvent::FullscreenChanged`: part of the snapshot, so a repeat changes nothing. */
  setFullscreen(active: boolean): boolean {
    const changed = this.fullscreen !== active;
    this.fullscreen = active;
    return this.apply(changed);
  }

  setMonitors(monitors: MonitorInfo[]): boolean {
    const changed = JSON.stringify(monitors) !== JSON.stringify(this.monitors);
    this.monitors = monitors;
    return this.apply(changed);
  }

  setTheme(theme: ThemeInfo): boolean {
    const changed = JSON.stringify(theme) !== JSON.stringify(this.theme);
    this.theme = theme;
    return this.apply(changed);
  }

  setRecycleBin(state: RecycleBinState): boolean {
    const changed =
      state.itemCount !== this.recycleBin.itemCount ||
      state.sizeBytes !== this.recycleBin.sizeBytes;
    this.recycleBin = state;
    return this.apply(changed);
  }

  setSettings(settings: Settings): boolean {
    const changed = JSON.stringify(settings) !== JSON.stringify(this.settings);
    this.settings = settings;
    return this.apply(changed);
  }

  setIdentity(appId: AppId, identity: AppIdentity): boolean {
    const changed = JSON.stringify(this.identities.get(appId)) !== JSON.stringify(identity);
    this.identities.set(appId, identity);
    return this.apply(changed);
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

    // 3. Running, unpinned apps in first-seen order.
    const runningItems: DockItem[] = [];
    for (const id of this.runningOrder) {
      const windows = byApp.get(id);
      if (windows) {
        byApp.delete(id);
        runningItems.push(this.appItem(id, false, null, windows));
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

    // 4. Recycle Bin.
    if (s.items.recycleBin) {
      if (items.length > 0) {
        separator();
      }
      const icon = this.recycleBin.itemCount === 0 ? 'recycle-bin-empty' : 'recycle-bin-full';
      items.push(fixedItem('recycleBin', 'fixed:recycle-bin', 'Recycle Bin', `shell:${icon}`));
    }

    return {
      revision: this.revision,
      items,
      foreground: this.foreground,
      fullscreen: this.fullscreen,
    };
  }

  /** `DockEngine::primary_action`. */
  primaryAction(itemKey: string): ItemAction {
    switch (itemKey) {
      case 'fixed:launcher':
        return { type: 'shell', action: 'openStartMenu' };
      case 'fixed:task-view':
        return { type: 'shell', action: 'openTaskView' };
      case 'fixed:show-desktop':
        return { type: 'shell', action: 'toggleShowDesktop' };
      case 'fixed:recycle-bin':
        return { type: 'shell', action: 'openRecycleBin' };
      default:
        break;
    }
    if (!itemKey.startsWith('app:')) {
      return { type: 'none' };
    }
    const appId = itemKey.slice('app:'.length);
    const windows = this.shownWindows()
      .filter((t) => appIdFor(t.raw) === appId)
      .map((t) => t.raw.id);
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

  // --- internals

  private allocSeq(): number {
    return this.nextSeq++;
  }

  private isShown(raw: RawWindow): boolean {
    return raw.isVisible && !(raw.isCloaked && this.settings.behavior.showOnlyCurrentDesktop);
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
      needsAttention: windows.some((t) => t.attention),
      badge: null,
    };
  }
}

function sameWindow(a: RawWindow, b: RawWindow): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
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
  private readonly timers = new Set<ReturnType<typeof setTimeout>>();
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

  constructor(options: MockOptions = {}) {
    const scenario = SCENARIOS[options.scenario ?? 'default']();
    this.latency = options.latencyMs ?? [20, 80];
    this.identityDelayMs = options.identityDelayMs ?? 300;
    this.logging = options.log ?? true;
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
    this.engine = new Engine(scenario.settings, DEFAULT_THEME);
    this.engine.setMonitors([demoMonitor()]);
    this.engine.setTheme(theme);
    this.engine.setRecycleBin(scenario.recycleBin);
    this.engine.snapshotWindows(scenario.windows);
    this.engine.setForeground(scenario.foreground);

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

  updateSettings(settings: Settings): Promise<Settings> {
    return this.command(() => {
      const validated = validateSettings(settings);
      this.applySettings(validated);
      return structuredClone(validated);
    });
  }

  getTheme(): Promise<ThemeInfo> {
    return this.command(() => ({ ...this.engine.theme }));
  }

  getMonitors(): Promise<MonitorInfo[]> {
    return this.command(() => structuredClone(this.engine.monitors));
  }

  // --- Backend: intents

  primaryClick(itemKey: string): Promise<void> {
    return this.command(() => {
      const action = this.engine.primaryAction(itemKey);
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

  reorderPinned(order: AppId[]): Promise<void> {
    return this.command(() => {
      this.record({ type: 'reorderPinned', order: [...order] });
      const settings = structuredClone(this.engine.settings);
      settings.pinned = reorderPinned(settings.pinned, order);
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

  setDockContentSize(width: number, height: number): Promise<void> {
    return this.command(() => {
      this.record({ type: 'setDockContentSize', width, height });
      this.log(`set_dock_content_size(${Math.round(width)} × ${Math.round(height)})`);
    });
  }

  /** `set_dock_hit_region`: a browser has no OS hit-testing; recorded and logged. */
  setDockHitRegion(panel: Rect, envelope: Rect): Promise<void> {
    return this.command(() => {
      this.record({ type: 'setDockHitRegion', panel: { ...panel }, envelope: { ...envelope } });
      this.log(
        `set_dock_hit_region(panel ${panel.width}×${panel.height} at ${panel.x},${panel.y}; ` +
          `envelope ${envelope.width}×${envelope.height} at ${envelope.x},${envelope.y})`,
      );
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

  /** `DockService::reset_settings`: the defaults, validated and applied like any update. */
  resetSettings(): Promise<Settings> {
    return this.command(() => {
      const defaults = validateSettings(defaultSettings());
      this.applySettings(defaults);
      return structuredClone(defaults);
    });
  }

  quit(): Promise<void> {
    return this.command(() => {
      this.record({ type: 'quit' });
      this.log('quit()');
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

  pushForeground(id: WindowId | null): void {
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
   * a game, a browser after F11, a video or a slideshow took the screen, or
   * gave it back. One `dock://state` when the value differs.
   */
  setFullscreen(active: boolean): void {
    if (this.engine.setFullscreen(active)) {
      this.log(`fullscreen ${active ? 'on' : 'off'}`);
      this.emitState();
    }
  }

  pushRecycleBin(state: RecycleBinState): void {
    if (this.engine.setRecycleBin(state)) {
      this.emitState();
    }
  }

  setTheme(theme: ThemeInfo): void {
    if (this.engine.setTheme(theme)) {
      this.emit('dock://theme', { ...theme });
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
    if (ms <= 0) {
      return Promise.resolve().then(run);
    }
    return new Promise<T>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.timers.delete(timer);
        try {
          resolve(run());
        } catch (err) {
          // Commands fail with a `string` across IPC (docs/ipc/); the mock
          // throws strings for the same reason.
          reject(err);
        }
      }, ms);
      this.timers.add(timer);
    });
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

  /** `persist_and_apply`: the settings event always, a state event if items changed. */
  private applySettings(settings: Settings): void {
    const changed = this.engine.setSettings(settings);
    this.emit('dock://settings', structuredClone(settings));
    if (changed) {
      this.emitState();
    }
    this.scheduleIdentityResolution();
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
