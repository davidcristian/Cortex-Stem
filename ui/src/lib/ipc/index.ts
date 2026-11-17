/**
 * Backend selection. Inside the Tauri app `window.__TAURI_INTERNALS__`
 * exists and the real backend is used; in a plain browser (and under
 * Playwright) the mock stands in. `?mock=1` forces the mock anywhere,
 * `?mock=many` picks the 20-app variant, `?scenario=<name>` any scenario
 * (`default`, `many`, `twoMonitors`), and the page's `?monitor=<id>` (the
 * app's own parameter for a secondary dock window, docs/ipc/windows.md)
 * tells the mock which dock window it stands for.
 */

import type { Backend } from './backend';
import type { MonitorId } from './generated/MonitorId';
import { isMockScenario, MockBackend, type MockScenario } from './mock';
import { TauriBackend } from './tauri';

export type { Backend, BackendEvent, BackendEvents, Unsubscribe } from './backend';
export { isShellIconKey } from './backend';
export { MockBackend } from './mock';
export { TauriBackend } from './tauri';

export type BackendChoice =
  { kind: 'tauri' } | { kind: 'mock'; scenario: MockScenario; monitor: MonitorId | null };

/** Pure decision, so it can be unit-tested without a DOM. */
export function chooseBackend(search: string, hasTauriInternals: boolean): BackendChoice {
  const params = new URLSearchParams(search);
  const mock = params.get('mock');
  if (mock === null && hasTauriInternals) {
    return { kind: 'tauri' };
  }
  const named = params.get('scenario');
  const scenario: MockScenario = isMockScenario(named)
    ? named
    : mock === 'many'
      ? 'many'
      : 'default';
  const monitor = params.get('monitor');
  return { kind: 'mock', scenario, monitor: monitor === null || monitor === '' ? null : monitor };
}

export function createBackend(): Backend {
  const choice = chooseBackend(window.location.search, '__TAURI_INTERNALS__' in window);
  if (choice.kind === 'tauri') {
    return new TauriBackend();
  }
  return new MockBackend({ scenario: choice.scenario, monitor: choice.monitor });
}
