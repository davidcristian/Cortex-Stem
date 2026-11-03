/**
 * Backend selection. Inside the Tauri app `window.__TAURI_INTERNALS__`
 * exists and the real backend is used; in a plain browser (and under
 * Playwright) the mock stands in. `?mock=1` forces the mock anywhere,
 * `?mock=many` picks the 20-app variant.
 */

import type { Backend } from './backend';
import { MockBackend, type MockScenario } from './mock';
import { TauriBackend } from './tauri';

export type { Backend, BackendEvent, BackendEvents, Unsubscribe } from './backend';
export { isShellIconKey } from './backend';
export { MockBackend } from './mock';
export { TauriBackend } from './tauri';

export type BackendChoice = { kind: 'tauri' } | { kind: 'mock'; scenario: MockScenario };

/** Pure decision, so it can be unit-tested without a DOM. */
export function chooseBackend(search: string, hasTauriInternals: boolean): BackendChoice {
  const mock = new URLSearchParams(search).get('mock');
  if (mock === null && hasTauriInternals) {
    return { kind: 'tauri' };
  }
  return { kind: 'mock', scenario: mock === 'many' ? 'many' : 'default' };
}

export function createBackend(): Backend {
  const choice = chooseBackend(window.location.search, '__TAURI_INTERNALS__' in window);
  if (choice.kind === 'tauri') {
    return new TauriBackend();
  }
  return new MockBackend({ scenario: choice.scenario });
}
