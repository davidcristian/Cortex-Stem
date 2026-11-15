# Frontend rules

These rules apply to the Svelte 5 and TypeScript frontend in `src/`
([AGENTS.md §7](../../AGENTS.md#7-frontend-rules-src)).

- Svelte 5 **runes** (`$state`, `$derived`, `$effect`, `$props`); no legacy `export let` or `$:`
  syntax (`svelte.config.js` sets `runes: true`).
- TypeScript strict. ESLint's `no-explicit-any` fails on every `any`, so each one needs an
  `eslint-disable` comment that says why.
- All backend access goes through `src/lib/ipc/backend.ts` (the `Backend` interface).
  `MockBackend` implements every method; it is what runs in the browser and in Playwright.
- Types that cross IPC are imported from `src/lib/ipc/generated/` (exported by `stem-core` through
  ts-rs). Never edit generated files by hand.
- Animations are CSS first and change only `transform` and `opacity`. Avoid forced layout (reading
  layout right after a style change): the dock runs all the time and must idle at about 0 % CPU,
  which `e2e/idle.spec.ts` checks.
- Dock items are keyboard-reachable and have `role` and `aria-label`.
- The dev server port is **1420** (Tauri's `devUrl`); the build output is `dist/`. The package
  manager is **pnpm**. The script names `dev`, `build`, `check`, `lint`, `format`, `test` and
  `e2e:screenshots` are a contract.
