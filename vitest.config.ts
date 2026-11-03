import { defineConfig, mergeConfig } from 'vitest/config';

import viteConfig from './vite.config.ts';

// Unit tests cover the backend abstraction, the mock, the store and pure
// helpers. Nothing here needs a DOM; browser-only code is exercised by the
// Playwright screenshots instead (`pnpm e2e:screenshots`).
export default mergeConfig(
  viteConfig,
  defineConfig({
    test: {
      include: ['src/**/*.test.ts'],
      environment: 'node',
      clearMocks: true,
    },
  }),
);
