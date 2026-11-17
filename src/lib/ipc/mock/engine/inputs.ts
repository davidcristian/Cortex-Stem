/**
 * The engine's inputs (`DockEngine::apply`): each one changes the state and
 * returns whether the snapshot changed.
 */

import type { AppId } from '$lib/ipc/generated/AppId';
import type { AppIdentity } from '$lib/ipc/generated/AppIdentity';
import type { ClockFormat } from '$lib/ipc/generated/ClockFormat';
import type { DockItemKind } from '$lib/ipc/generated/DockItemKind';
import type { DockState } from '$lib/ipc/generated/DockState';
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

import { overlapped } from '../overlap';
import type { Tracked } from './state';
import { EngineSnapshot } from './snapshot';

/**
 * What `DockEngine::apply` compares to tell whether an input changed the
 * snapshot (`renders_differently` in engine.rs): everything but the
 * revision, with `power` only while the battery item is shown, `volume`
 * only while the volume item is and `clockFormat` only while the clock item
 * is, since nothing else on the pages reads them. Two snapshots
 * that give the same string show the same thing.
 */
export function shownState(state: DockState): string {
  const shows = (kind: DockItemKind): boolean => state.items.some((i) => i.kind === kind);
  return JSON.stringify({
    ...state,
    revision: 0,
    power: shows('battery') ? state.power : 'not shown',
    volume: shows('volume') ? state.volume : 'not shown',
    clockFormat: shows('clock') ? state.clockFormat : 'not shown',
  });
}

export class EngineInputs extends EngineSnapshot {
  // --- inputs (each returns whether the snapshot changed; `input` decides and bumps the revision)

  /**
   * `DockEngine::apply`: run one input (`mutate`), then tell whether the
   * renderable snapshot changed by comparing it with the one before
   * (`EngineOutcome::changed`); only then does the revision go up,
   * by one, and only then does the backend send a `dock://state`. So an
   * input that changes nothing the snapshot shows changes nothing: the
   * theme, a monitor list that moves no dock's fact, the identity of an
   * app without an item, a setting no item depends on, a frame that flips
   * no `overlapped`, the size of the Recycle Bin's contents, and a power,
   * volume or regional-format reading while its item is not shown
   * (`shownState`).
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
        // As `PlatformEvent::WindowAttention` in engine/platform.rs: the window
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
   * bumps the revision, and only while the clock item is shown.
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
}
