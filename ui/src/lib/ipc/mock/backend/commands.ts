/**
 * The `Backend` queries of `MockBackend`, and its commands on dock items and
 * apps: clicks, the wheel, window actions, launches, pins, hidden apps, icons.
 */

import { isShellIconKey } from '$lib/ipc/backend';
import type { AppId } from '$lib/ipc/generated/AppId';
import type { DockState } from '$lib/ipc/generated/DockState';
import type { IconKey } from '$lib/ipc/generated/IconKey';
import type { LaunchTarget } from '$lib/ipc/generated/LaunchTarget';
import type { MonitorInfo } from '$lib/ipc/generated/MonitorInfo';
import type { ScrollDirection } from '$lib/ipc/generated/ScrollDirection';
import type { Settings } from '$lib/ipc/generated/Settings';
import type { ThemeInfo } from '$lib/ipc/generated/ThemeInfo';
import type { WindowAction } from '$lib/ipc/generated/WindowAction';
import type { WindowId } from '$lib/ipc/generated/WindowId';

import { mockIconDataUrl } from '../icons';
import { fallbackDisplayName, identityForTarget } from '../identity';
import { clamp, reorderPinned } from '../settings';
import { MockBackendHooks } from './hooks';

export class MockBackendCommands extends MockBackendHooks {
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
