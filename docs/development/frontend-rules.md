# Frontend rules

These rules apply to the Svelte 5 and TypeScript frontend in `ui/`
([AGENTS.md §7](../../AGENTS.md#7-frontend-rules-ui)).

- Svelte 5 **runes** (`$state`, `$derived`, `$effect`, `$props`); no legacy `export let` or `$:`
  syntax (`ui/svelte.config.js` sets `runes: true`).
- TypeScript strict. ESLint's `no-explicit-any` fails on every `any`, so each one needs an
  `eslint-disable` comment that says why.
- All backend access goes through `ui/src/lib/ipc/backend.ts` (the `Backend` interface).
  `MockBackend` implements every method; it is what runs in the browser and in Playwright.
- Types that cross IPC are imported from `ui/src/lib/ipc/generated/` (exported by `stem-core`
  through ts-rs). Never edit generated files by hand.
- Animations are CSS first and change only `transform` and `opacity`. Avoid forced layout (reading
  layout right after a style change): the dock runs all the time and must idle at about 0 % CPU,
  which `ui/e2e/idle.spec.ts` checks.
- Dock items are keyboard-reachable and have `role` and `aria-label`.
- The dev server port is **1420** (Tauri's `devUrl`); the build output is `ui/dist/`. The package
  manager is **pnpm**. Its commands run in `ui/`, where Corepack picks the pnpm version that
  `ui/package.json` sets. The script names `dev`, `build`, `check`, `lint`, `format`, `test` and
  `e2e:screenshots` are a contract.
