/**
 * What the dock page reports to `MockBackend` about its window: its content
 * size, hit region, resting panel, auto-hide flag and hover.
 */

import type { Viewport } from '$lib/dock/geometry';
import type { DockPosition } from '$lib/ipc/generated/DockPosition';
import type { MonitorInfo } from '$lib/ipc/generated/MonitorInfo';
import type { Rect } from '$lib/ipc/generated/Rect';

import { panelOnScreen, reanchorPanel, viewportError } from '../panel';
import { MockBackendCommands } from './commands';

export class MockBackendReports extends MockBackendCommands {
  /**
   * Whether a report the page measured for `edge` belongs to where the
   * app layer has the window (`DockWindowState::stale_for`): the
   * mock's window moves with `Settings.position` at once, so a report for
   * another edge was measured before the page saw the change, and is
   * dropped (logged; it stays recorded in `calls`).
   */
  private reportIsCurrent(command: string, edge: DockPosition): boolean {
    const placed = this.engine.settings.position;
    if (edge === placed) {
      return true;
    }
    this.log(
      `${command} measured for the ${edge} edge dropped; the window is on the ${placed} edge`,
    );
    return false;
  }

  /** `set_dock_content_size`: a browser window is not resized; recorded and logged. */
  setDockContentSize(width: number, height: number, edge: DockPosition): Promise<void> {
    return this.command(() => {
      this.record({ type: 'setDockContentSize', width, height, edge });
      if (this.reportIsCurrent('set_dock_content_size', edge)) {
        this.log(`set_dock_content_size(${Math.round(width)} × ${Math.round(height)}, ${edge})`);
      }
    });
  }

  /** `set_dock_hit_region`: a browser has no OS hit-testing; recorded and logged. */
  setDockHitRegion(panel: Rect, envelope: Rect, edge: DockPosition): Promise<void> {
    return this.command(() => {
      this.record({
        type: 'setDockHitRegion',
        panel: { ...panel },
        envelope: { ...envelope },
        edge,
      });
      if (this.reportIsCurrent('set_dock_hit_region', edge)) {
        this.log(
          `set_dock_hit_region(panel ${panel.width}×${panel.height} at ${panel.x},${panel.y}; ` +
            `envelope ${envelope.width}×${envelope.height} at ${envelope.x},${envelope.y}; ${edge})`,
        );
      }
    });
  }

  /** The monitor this backend's page sits on, as the app layer would place its window. */
  private pageMonitorInfo(): MonitorInfo | undefined {
    const monitors = this.engine.monitors;
    return (
      (this.pageMonitor === null ? undefined : monitors.find((m) => m.id === this.pageMonitor)) ??
      monitors.find((m) => m.isPrimary) ??
      monitors[0]
    );
  }

  /**
   * `set_dock_panel_rect`: recorded with its provenance;
   * rejected like Rust for a non-positive or non-finite viewport; dropped
   * when measured for another edge than the window's (`reportIsCurrent`);
   * else re-anchored from the viewport it carries to the window's viewport
   * now (`reanchorPanel`; `MockOptions.viewportHeight` and `viewportWidth`,
   * else `window.innerWidth` × `innerHeight` read per call, else the work
   * area's size), converted to the page's monitor's screen coordinates the
   * way the app layer converts it for the real window on the configured edge
   * (`panelOnScreen`, at that monitor's scale), and handed to the engine
   * under this dock window's label and monitor, which emits a state
   * only when the docks' facts change.
   */
  setDockPanelRect(rect: Rect, edge: DockPosition, viewport: Viewport): Promise<void> {
    return this.command(() => {
      this.record({
        type: 'setDockPanelRect',
        rect: { ...rect },
        edge,
        viewport: { width: viewport.width, height: viewport.height },
      });
      const invalid = viewportError(viewport);
      if (invalid !== null) {
        throw invalid;
      }
      if (!this.reportIsCurrent('set_dock_panel_rect', edge)) {
        return;
      }
      const monitor = this.pageMonitorInfo();
      const workArea = monitor?.workArea ?? { x: 0, y: 0, width: 0, height: 0 };
      const browser = typeof window !== 'undefined';
      const now = {
        width:
          this.viewportWidth ??
          (browser && window.innerWidth > 0 ? window.innerWidth : workArea.width),
        height:
          this.viewportHeight ??
          (browser && window.innerHeight > 0 ? window.innerHeight : workArea.height),
      };
      const css = reanchorPanel(rect, edge, viewport, now);
      const screen = panelOnScreen(css, workArea, now, edge, monitor?.scaleFactor ?? 1);
      const label = this.pageLabel;
      this.log(
        `set_dock_panel_rect(${rect.width}×${rect.height} at ${rect.x},${rect.y} in ` +
          `${viewport.width}×${viewport.height}, ${edge}) -> ${css.x},${css.y} in ` +
          `${now.width}×${now.height} -> ` +
          `${screen.x},${screen.y} on screen (${label} on ${monitor?.id ?? 'no monitor'})`,
      );
      if (this.engine.setDockPanel(label, monitor?.id ?? null, screen)) {
        this.emitState();
      }
    });
  }

  /**
   * `set_dock_auto_hidden`: the app layer only keeps the flag for
   * its `toggle_dock`, which has no counterpart in a browser; recorded and
   * logged, so tests can see each change the UI reported.
   */
  setDockAutoHidden(hidden: boolean): Promise<void> {
    return this.command(() => {
      this.record({ type: 'setDockAutoHidden', hidden });
      this.autoHidden = hidden;
      this.log(`set_dock_auto_hidden(${hidden})`);
    });
  }

  // --- Backend: planned for M2 (docs/ipc/commands.md); the mock only records it.

  setDockHover(hovering: boolean): Promise<void> {
    return this.command(() => {
      this.record({ type: 'setDockHover', hovering });
    });
  }
}
