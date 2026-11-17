import { existsSync } from 'node:fs';

import { defineConfig } from '@playwright/test';

// The dev hosts have a Chromium matching this Playwright version at a fixed
// path; CI installs its own browser and leaves the path absent.
const PREINSTALLED_CHROMIUM = '/opt/pw-browsers/chromium';
// Same override as vite.config.ts (STEM_DEV_PORT), so a run in a second
// worktree never reuses another checkout's dev server on 1420.
const PORT = Number(process.env.STEM_DEV_PORT ?? 1420);

export default defineConfig({
  testDir: 'e2e',
  outputDir: 'test-results',
  fullyParallel: false,
  workers: 1,
  retries: 0,
  timeout: 60_000,
  reporter: [['list']],
  use: {
    baseURL: `http://localhost:${PORT}`,
    browserName: 'chromium',
    viewport: { width: 1280, height: 240 },
    deviceScaleFactor: 2,
    colorScheme: 'dark',
    ...(existsSync(PREINSTALLED_CHROMIUM)
      ? { launchOptions: { executablePath: PREINSTALLED_CHROMIUM } }
      : {}),
  },
  webServer: {
    command: 'pnpm dev',
    url: `http://localhost:${PORT}/dock.html?mock=1`,
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
