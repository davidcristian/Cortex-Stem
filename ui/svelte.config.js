import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

/** @type {import('@sveltejs/vite-plugin-svelte').SvelteConfig} */
export default {
  preprocess: vitePreprocess(),
  compilerOptions: {
    // Runes only (docs/development/frontend-rules.md, ADR-0002): legacy
    // `export let` / `$:` syntax is a compile error, which `svelte-check`
    // and `svelte/valid-compile` both report.
    runes: true,
  },
};
