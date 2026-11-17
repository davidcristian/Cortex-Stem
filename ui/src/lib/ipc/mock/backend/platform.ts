/**
 * The mock platform under `MockBackend`: the platform events a test or a
 * demo pushes (`MockPlatform::push`), and what a command does on the OS
 * (`MockPlatform::window_action` and the like).
 */

import type { AppId } from '$lib/ipc/generated/AppId';
import type { ClockFormat } from '$lib/ipc/generated/ClockFormat';
import type { LaunchTarget } from '$lib/ipc/generated/LaunchTarget';
import type { MonitorId } from '$lib/ipc/generated/MonitorId';
import type { PowerStatus } from '$lib/ipc/generated/PowerStatus';
import type { RawWindow } from '$lib/ipc/generated/RawWindow';
import type { Rect } from '$lib/ipc/generated/Rect';
import type { RecycleBinState } from '$lib/ipc/generated/RecycleBinState';
import type { ScrollDirection } from '$lib/ipc/generated/ScrollDirection';
import type { ShellAction } from '$lib/ipc/generated/ShellAction';
import type { VolumeStatus } from '$lib/ipc/generated/VolumeStatus';
import type { WindowAction } from '$lib/ipc/generated/WindowAction';
import type { WindowId } from '$lib/ipc/generated/WindowId';

import { batteryTitle, volumeStep, volumeTitle } from '../readings';
import { MockBackendBase } from './base';

export class MockBackendPlatform extends MockBackendBase {
  // --- Scripting hooks (mirror of `MockPlatform::push`), for tests and demos.

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
   * reading. One `dock://state` when it does and the clock item is shown;
   * without it the snapshot keeps the reading for later.
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

  /** Resolve pending identities now instead of after the configured delay. */
  resolveIdentitiesNow(): void {
    if (this.identityTimer !== null) {
      clearTimeout(this.identityTimer);
      this.identityTimer = null;
    }
    this.resolveIdentities();
  }

  // --- internals

  private launchTargetFor(appId: AppId): LaunchTarget | null {
    return (
      this.engine.settings.pinned.find((p) => p.id === appId)?.launch ??
      this.engine.identities.get(appId)?.launch ??
      null
    );
  }

  protected doLaunch(appId: AppId): void {
    const target = this.launchTargetFor(appId);
    if (target === null) {
      throw `app "${appId}" has no launch target`;
    }
    this.record({ type: 'launch', target });
    this.log(`launch ${describeTarget(target)}`);
    this.emit('dock://item-bounce', { itemKey: `app:${appId}` });
  }

  /** `MockPlatform::window_action` plus the engine's handling of the events it emits. */
  protected doWindowAction(windowId: WindowId, action: WindowAction): void {
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
  protected doVolumeStep(direction: ScrollDirection): void {
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

  protected doShellAction(action: ShellAction): void {
    this.record({ type: 'shell', action });
    this.log(`shell action ${action}`);
    if (action === 'emptyRecycleBin') {
      this.pushRecycleBin({ itemCount: 0, sizeBytes: 0 });
    }
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
