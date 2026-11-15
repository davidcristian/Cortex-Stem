/**
 * Browser demo hook: in mock mode each page exposes its `MockBackend` so it
 * can be scripted from the devtools console and from Playwright.
 *
 * The dock page also takes `?position=bottom|top|left|right` in mock mode:
 * the mock's settings start with the dock on that edge, as a
 * settings file with that position would, so a browser and Playwright can
 * show every edge without a settings round trip. Anything else, or none,
 * leaves the scenario's own (`bottom`).
 */

import type { DockPosition } from './generated/DockPosition';
import type { MockBackend } from './mock';

declare global {
  interface Window {
    __dockMock?: MockBackend;
  }
}

export function exposeMock(backend: MockBackend): void {
  window.__dockMock = backend;
}

const POSITIONS: readonly DockPosition[] = ['bottom', 'top', 'left', 'right'];

/** The edge `?position=` names in `search` (a `location.search`), or `null`. */
export function demoPosition(search: string): DockPosition | null {
  const value = new URLSearchParams(search).get('position');
  return POSITIONS.find((p) => p === value) ?? null;
}

/**
 * Apply the demo's query parameters to `backend` before the page starts:
 * `?position=` puts the dock on that edge (`MockBackend.setPosition`).
 */
export function applyDemoQuery(backend: MockBackend, search: string): void {
  const position = demoPosition(search);
  if (position !== null) {
    backend.setPosition(position);
  }
}
