/**
 * Browser demo hook: in mock mode each page exposes its `MockBackend` so it
 * can be scripted from the devtools console and from Playwright.
 */

import type { MockBackend } from './mock';

declare global {
  interface Window {
    __dockMock?: MockBackend;
  }
}

export function exposeMock(backend: MockBackend): void {
  window.__dockMock = backend;
}
