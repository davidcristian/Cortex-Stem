/**
 * The mock's engine (mirror of crates/stem-core/src/engine.rs): what a click
 * or a wheel notch over an item does. Its state, snapshot and inputs are the
 * classes in `engine/` it extends.
 */

import type { AppId } from '$lib/ipc/generated/AppId';
import type { ScrollDirection } from '$lib/ipc/generated/ScrollDirection';
import type { ShellAction } from '$lib/ipc/generated/ShellAction';
import type { WindowId } from '$lib/ipc/generated/WindowId';

import { EngineInputs } from './engine/inputs';
import { appIdFor } from './model';

/** `ItemAction`: what a primary click should do. */
export type ItemAction =
  | { type: 'launch'; appId: AppId }
  | { type: 'activate'; windowId: WindowId }
  | { type: 'minimize'; windowId: WindowId }
  | { type: 'shell'; action: ShellAction }
  | { type: 'none' };

/** `ScrollEffect`: what one wheel notch over an item does. */
export type ScrollEffect =
  | { type: 'activate'; windowId: WindowId }
  | { type: 'stepVolume'; direction: ScrollDirection }
  | { type: 'nothing' }
  | { type: 'unknownItem' };

export class Engine extends EngineInputs {
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
}
