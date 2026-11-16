/**
 * The state of the mock's engine (mirror of crates/stem-core/src/engine.rs):
 * the tracked windows, identities, dock panels and readings, and the queries
 * the commands make of them.
 */

import type { AppId } from '$lib/ipc/generated/AppId';
import type { AppIdentity } from '$lib/ipc/generated/AppIdentity';
import type { ClockFormat } from '$lib/ipc/generated/ClockFormat';
import type { HiddenApp } from '$lib/ipc/generated/HiddenApp';
import type { IconKey } from '$lib/ipc/generated/IconKey';
import type { MonitorId } from '$lib/ipc/generated/MonitorId';
import type { MonitorInfo } from '$lib/ipc/generated/MonitorInfo';
import type { PowerStatus } from '$lib/ipc/generated/PowerStatus';
import type { RawWindow } from '$lib/ipc/generated/RawWindow';
import type { Rect } from '$lib/ipc/generated/Rect';
import type { RecycleBinState } from '$lib/ipc/generated/RecycleBinState';
import type { Settings } from '$lib/ipc/generated/Settings';
import type { ThemeInfo } from '$lib/ipc/generated/ThemeInfo';
import type { VolumeStatus } from '$lib/ipc/generated/VolumeStatus';
import type { WindowId } from '$lib/ipc/generated/WindowId';

import { appIdFor } from '../model';
import { NO_CLOCK_FORMAT, NO_POWER } from '../readings';

export interface Tracked {
  raw: RawWindow;
  /** First-seen sequence number; drives stable ordering. */
  seq: number;
  attention: boolean;
}

/** One dock window as the app layer registered it (`DockPanel` in the Rust engine). */
export interface DockPanel {
  /** The monitor the window sits on; `null` while unknown (no entry in `DockState.docks` then). */
  monitor: MonitorId | null;
  /** Where its panel rests, in screen px; `null` before the page's first report and while hidden. */
  rect: Rect | null;
  /** `DockFacts.overlapped` for this panel. */
  overlapped: boolean;
}

export class EngineState {
  settings: Settings;
  readonly windows = new Map<WindowId, Tracked>();
  readonly identities = new Map<AppId, AppIdentity>();
  protected runningOrder: AppId[] = [];
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
   * known. An `aumid:` app's window is never asked for the path (a Store
   * app's frame is the `ApplicationFrameHost` process): only its identity's
   * file counts.
   */
  hiddenAppFacts(appId: AppId): Pick<HiddenApp, 'path' | 'iconKey'> {
    const window = this.appWindow(appId);
    const identity = this.identities.get(appId);
    const launch = identity?.launch ?? null;
    let path: string | null = null;
    let fileIcon: IconKey | null = null;
    const exePath = appId.startsWith('aumid:') ? null : (window?.exePath ?? null);
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

  // --- internals

  protected allocSeq(): number {
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
  protected shownWindows(): Tracked[] {
    return [...this.windows.values()]
      .filter((t) => this.isShown(t.raw))
      .sort((a, b) => a.seq - b.seq);
  }

  /** Keep the relative order of apps still running, append newcomers. */
  protected rebuildRunningOrder(): void {
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
}
