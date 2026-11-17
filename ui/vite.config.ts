import { fileURLToPath, URL } from 'node:url';

import { svelte } from '@sveltejs/vite-plugin-svelte';
import { defineConfig } from 'vite';

// Multi-page app: one HTML entry per Tauri window (docs/ipc/windows.md).
// The dev server port is Tauri's `devUrl` (src-tauri/tauri.conf.json) and
// the build output is its `frontendDist`. STEM_DEV_PORT moves the dev server
// for browser-only runs (Playwright in a second worktree while a dev build
// holds 1420); Tauri's devUrl is fixed, so never set it for `tauri dev`.
export const DEV_PORT = Number(process.env.STEM_DEV_PORT ?? 1420);

export default defineConfig({
  plugins: [svelte()],
  resolve: {
    alias: {
      $lib: fileURLToPath(new URL('./src/lib', import.meta.url)),
    },
  },
  clearScreen: false,
  server: {
    port: DEV_PORT,
    strictPort: true,
  },
  // Tauri exposes build facts as TAURI_ENV_* variables.
  envPrefix: ['VITE_', 'TAURI_ENV_*'],
  build: {
    // The only runtime is WebView2 (Chromium), see ADR-0001.
    target: 'chrome105',
    outDir: 'dist',
    emptyOutDir: true,
    // Vite 8 minifies with its built-in Oxc minifier; debug builds keep the
    // output readable.
    minify: !process.env.TAURI_ENV_DEBUG,
    sourcemap: Boolean(process.env.TAURI_ENV_DEBUG),
    rollupOptions: {
      input: {
        dock: fileURLToPath(new URL('./dock.html', import.meta.url)),
        settings: fileURLToPath(new URL('./settings.html', import.meta.url)),
      },
    },
  },
});
