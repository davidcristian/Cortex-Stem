/** The options a `MockBackend` takes and the side effects it records. */

import type { Viewport } from '$lib/dock/geometry';
import type { AppFolder } from '$lib/ipc/generated/AppFolder';
import type { AppId } from '$lib/ipc/generated/AppId';
import type { ClockFormat } from '$lib/ipc/generated/ClockFormat';
import type { DockPosition } from '$lib/ipc/generated/DockPosition';
import type { LaunchTarget } from '$lib/ipc/generated/LaunchTarget';
import type { MonitorId } from '$lib/ipc/generated/MonitorId';
import type { PowerStatus } from '$lib/ipc/generated/PowerStatus';
import type { Rect } from '$lib/ipc/generated/Rect';
import type { ScrollDirection } from '$lib/ipc/generated/ScrollDirection';
import type { ShellAction } from '$lib/ipc/generated/ShellAction';
import type { VolumeStatus } from '$lib/ipc/generated/VolumeStatus';
import type { WindowAction } from '$lib/ipc/generated/WindowAction';
import type { WindowId } from '$lib/ipc/generated/WindowId';

import type { MockUserCssFile } from './app-files';

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
