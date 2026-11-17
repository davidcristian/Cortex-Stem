/**
 * The demo scenarios (mirror of crates/stem-platform/src/mock.rs and
 * mock/demo.rs): the monitors, windows, Recycle Bin, settings and theme a
 * `MockBackend` starts with.
 */

import type { MonitorId } from '$lib/ipc/generated/MonitorId';
import type { MonitorInfo } from '$lib/ipc/generated/MonitorInfo';
import type { PinnedApp } from '$lib/ipc/generated/PinnedApp';
import type { RawWindow } from '$lib/ipc/generated/RawWindow';
import type { Rect } from '$lib/ipc/generated/Rect';
import type { RecycleBinState } from '$lib/ipc/generated/RecycleBinState';
import type { Settings } from '$lib/ipc/generated/Settings';
import type { ThemeInfo } from '$lib/ipc/generated/ThemeInfo';
import type { WindowId } from '$lib/ipc/generated/WindowId';

import { appIdForExe, fileStem } from './model';
import type { MockScenario } from './options';
import { defaultSettings } from './settings';

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
 * A demo window with a frame on a demo monitor. Every demo frame
 * keeps clear of the 250 px of its work area along the bottom, the left and
 * the right edge, where a dock's panel rests on those edges, so
 * nothing overlaps it until a test moves one (`moveWindow`); a minimised
 * window keeps the frame it would be restored to (the overlap rule ignores
 * it while minimised). The top band is not kept clear: the frames start
 * near the top as on a real desktop, and the demo has no room for windows
 * of these sizes between two bands, so a dock on `top` with
 * `whenOverlapped` hides in the demo. `demoWindows` has the same frames
 * as the Rust demo (`demo_windows` in `crates/stem-platform/src/mock/demo.rs`).
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

export const SCENARIOS: Record<MockScenario, () => Scenario> = {
  default: demoScenario,
  many: manyScenario,
  twoMonitors: twoMonitorsScenario,
};

/** The mock OS's theme at start; `MockBackend` takes `dark` from the browser. */
export const DEFAULT_THEME: ThemeInfo = {
  accent: '#0078D4',
  dark: true,
  highContrast: false,
  transparency: true,
};
