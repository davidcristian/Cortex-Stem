/**
 * `MockBackend`: the whole IPC contract (docs/ipc/ v1.32) without Rust, for
 * browser runs and Playwright screenshots.
 *
 * It mirrors the Rust pieces the UI would otherwise be talking to, one module
 * in `mock/` per piece so that each can be compared with its Rust side:
 *
 * 1. the demo scenario of `crates/stem-platform/src/mock.rs` (windows with
 *    their frames, monitor, recycle bin, deterministic icon colours), plus a
 *    second monitor with two windows (`twoMonitors`): `scenarios.ts`,
 *    `identity.ts` and `icons.ts`;
 * 2. the ordering and click rules of `crates/stem-core/src/engine.rs`, the
 *    overlap rule of `crates/stem-core/src/overlap.rs`, and the
 *    per-dock facts of `DockState.docks`: `engine.ts` and `engine/`, with
 *    `overlap.ts`, `model.ts`, `settings.ts` and `readings.ts`;
 * 3. the command semantics and lazy identity resolution of
 *    `crates/stem-service/src/lib.rs`, and the app layer's conversion of the
 *    dock panel to screen coordinates and its registration under the dock
 *    window's label and monitor (`src-tauri/src/windows.rs`); one backend
 *    serves one page, so it knows which dock window it stands for
 *    (`MockOptions.monitor`): `backend.ts` and `backend/`, with `panel.ts`
 *    and `app-files.ts`;
 * 4. which dropped paths go to the Recycle Bin
 *    (`crates/stem-platform/src/rules/recycle.rs`): `recycle.ts`.
 *
 * Behaviour decisions belong to Rust (ADR-0006). Nothing here is used by the
 * UI directly; the UI only ever sees `DockState` snapshots and `Settings`.
 * This module re-exports what the pages and the tests import.
 */

export {
  demoAppInfo,
  DEMO_USER_CSS_PATH,
  MOCK_USER_CSS_TEMPLATE,
  readUserCss,
  USER_CSS_MAX_BYTES,
  type MockUserCssFile,
} from './mock/app-files';
export { MockBackend } from './mock/backend';
export type { ItemAction, ScrollEffect } from './mock/engine';
export { shownState } from './mock/engine/inputs';
export { colorFor, mockIconDataUrl } from './mock/icons';
export { appsFolderKnows, aumidFallbackName } from './mock/identity';
export {
  appIdFor,
  appIdForAppUserModelId,
  appIdForExe,
  appIdForWindowClass,
  fileStem,
  normalizePath,
} from './mock/model';
export {
  isMockScenario,
  MOCK_SCENARIOS,
  type MockCall,
  type MockOptions,
  type MockScenario,
} from './mock/options';
export { overlapped, rectsIntersect } from './mock/overlap';
export { dockLabel, panelOnScreen, reanchorPanel, VIEWPORT_SLACK_CSS } from './mock/panel';
export {
  batteryTitle,
  DEMO_POWER,
  DEMO_VOLUME,
  VOLUME_STEP,
  volumeStep,
  volumeTitle,
} from './mock/readings';
export { nothingToRecycle, recyclePlan, type RecyclePlan, type RecycleSkip } from './mock/recycle';
export { SECOND_MONITOR_ID } from './mock/scenarios';
export { defaultSettings, reorderPinned, validateSettings } from './mock/settings';
