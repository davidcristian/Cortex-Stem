/**
 * `Backend` over Tauri's `invoke` / `listen`. Command names and argument keys
 * are exactly those of docs/ipc/commands.md (`src-tauri/src/commands.rs`):
 * Tauri maps camelCase argument keys to the Rust parameter names.
 */

import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';

import type { AppId } from '$lib/ipc/generated/AppId';
import type { DockState } from '$lib/ipc/generated/DockState';
import type { IconKey } from '$lib/ipc/generated/IconKey';
import type { LaunchTarget } from '$lib/ipc/generated/LaunchTarget';
import type { MonitorInfo } from '$lib/ipc/generated/MonitorInfo';
import type { Settings } from '$lib/ipc/generated/Settings';
import type { ShellAction } from '$lib/ipc/generated/ShellAction';
import type { ThemeInfo } from '$lib/ipc/generated/ThemeInfo';
import type { WindowAction } from '$lib/ipc/generated/WindowAction';
import type { WindowId } from '$lib/ipc/generated/WindowId';

import type { Backend, BackendEvent, BackendEvents, EventHandler, Unsubscribe } from './backend';

export class TauriBackend implements Backend {
  getDockState(): Promise<DockState> {
    return invoke<DockState>('get_dock_state');
  }

  getSettings(): Promise<Settings> {
    return invoke<Settings>('get_settings');
  }

  updateSettings(settings: Settings): Promise<Settings> {
    return invoke<Settings>('update_settings', { settings });
  }

  getTheme(): Promise<ThemeInfo> {
    return invoke<ThemeInfo>('get_theme');
  }

  getMonitors(): Promise<MonitorInfo[]> {
    return invoke<MonitorInfo[]>('get_monitors');
  }

  primaryClick(itemKey: string): Promise<void> {
    return invoke<void>('primary_click', { itemKey });
  }

  windowAction(windowId: WindowId, action: WindowAction): Promise<void> {
    return invoke<void>('window_action', { windowId, action });
  }

  launchApp(appId: AppId): Promise<void> {
    return invoke<void>('launch_app', { appId });
  }

  pinApp(appId: AppId): Promise<void> {
    return invoke<void>('pin_app', { appId });
  }

  pinTarget(target: LaunchTarget): Promise<AppId> {
    return invoke<AppId>('pin_target', { target });
  }

  unpinApp(appId: AppId): Promise<void> {
    return invoke<void>('unpin_app', { appId });
  }

  reorderPinned(order: AppId[]): Promise<void> {
    return invoke<void>('reorder_pinned', { order });
  }

  getIcon(iconKey: IconKey, size: number): Promise<string | null> {
    // `size` is a `u32` on the Rust side.
    return invoke<string | null>('get_icon', { iconKey, size: Math.max(1, Math.round(size)) });
  }

  setDockContentSize(width: number, height: number): Promise<void> {
    return invoke<void>('set_dock_content_size', { width, height });
  }

  shellAction(action: ShellAction): Promise<void> {
    return invoke<void>('shell_action', { action });
  }

  openSettings(): Promise<void> {
    return invoke<void>('open_settings');
  }

  resetSettings(): Promise<Settings> {
    return invoke<Settings>('reset_settings');
  }

  quit(): Promise<void> {
    return invoke<void>('quit');
  }

  showItemMenu(itemKey: string, x: number, y: number): Promise<void> {
    return invoke<void>('show_item_menu', { itemKey, x, y });
  }

  showDockMenu(x: number, y: number): Promise<void> {
    return invoke<void>('show_dock_menu', { x, y });
  }

  on<E extends BackendEvent>(event: E, handler: EventHandler<E>): Unsubscribe {
    // `listen` resolves asynchronously; the returned function must work
    // whether or not the registration has completed yet.
    let disposed = false;
    let unlisten: UnlistenFn | undefined;
    void listen<BackendEvents[E]>(event, (e) => {
      if (!disposed) {
        handler(e.payload);
      }
    }).then(
      (fn) => {
        if (disposed) {
          fn();
        } else {
          unlisten = fn;
        }
      },
      (err: unknown) => console.error(`listen(${event}) failed:`, err),
    );
    return () => {
      disposed = true;
      unlisten?.();
      unlisten = undefined;
    };
  }
}
