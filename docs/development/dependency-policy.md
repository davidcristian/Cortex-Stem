# Dependency policy

The dependency policy says what a change that adds a Rust crate, a `windows` crate feature or a
frontend package must do ([AGENTS.md §12](../../AGENTS.md#12-dependency-policy)).

- Prefer the standard library, then crates already in the workspace.
- A new crate needs a one-line justification in the report, its version declared once in
  `[workspace.dependencies]` (`Cargo.lock` records the exact versions), and no duplicate
  functionality.
- `windows` crate features are enabled per symbol as needed. Keep the list in
  `crates/stem-platform/Cargo.toml` sorted and minimal; features only the diagnostic probes use go
  in its `probes` feature.
- Frontend: no UI component libraries. Vite, Svelte, TypeScript, Vitest, Playwright, ESLint,
  Prettier and the `@tauri-apps/*` packages are the set.
