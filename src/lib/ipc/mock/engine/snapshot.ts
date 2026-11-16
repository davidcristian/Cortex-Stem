/** The engine's renderable snapshot (`DockEngine::snapshot`) and the docks' facts. */

import type { AppId } from '$lib/ipc/generated/AppId';
import type { DockFacts } from '$lib/ipc/generated/DockFacts';
import type { DockItem } from '$lib/ipc/generated/DockItem';
import type { DockItemKind } from '$lib/ipc/generated/DockItemKind';
import type { DockState } from '$lib/ipc/generated/DockState';
import type { IconKey } from '$lib/ipc/generated/IconKey';
import type { MonitorId } from '$lib/ipc/generated/MonitorId';
import type { WindowInfo } from '$lib/ipc/generated/WindowInfo';

import { appIdFor } from '../model';
import { batteryTitle, volumeTitle } from '../readings';
import { EngineState, type Tracked } from './state';

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

export class EngineSnapshot extends EngineState {
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
