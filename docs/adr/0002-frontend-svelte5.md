# ADR-0002: Svelte 5 + TypeScript + Vite for the frontend

Date: 2026-11-03 · Status: accepted

## Context

The dock UI is small (a strip of icons), animation-heavy and always
running, so it must use little CPU and memory. AI coding agents write most
of it, so the framework must be widely known and hard to misuse.

Alternatives: React (the most familiar to agents; more runtime overhead and
more boilerplate for a tiny UI), SolidJS (fast; less familiar to agents),
plain TypeScript (no help with reactivity, more code for the settings UI).

## Decision

Svelte 5 with runes, TypeScript in strict mode, Vite 8, two pages
(`dock.html`, `settings.html`). Vitest for unit tests, Playwright for
screenshots, ESLint and Prettier. No component libraries.

## Consequences

- The compiled output is small, and animation written in CSS fits the
  framework.
- Components use runes syntax (`$state`, `$derived`, `$props`).
  `svelte.config.js` compiles in runes mode, and Svelte 4 syntax
  (`export let`, `$:`) is a lint error (`eslint.config.js`).
- Types that cross IPC are generated from Rust with ts-rs into
  `src/lib/ipc/generated/`; there are no hand-written copies.
