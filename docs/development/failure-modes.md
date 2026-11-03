# Known agent failure modes

These are mistakes to avoid when working in this repository
([AGENTS.md §14](../../AGENTS.md#14-known-agent-failure-modes-to-avoid)).

- Invented `windows` crate paths or features: compile to prove them.
- Svelte 4 syntax in a Svelte 5 project.
- Calling Win32 from a Tauri command handler, which blocks the UI thread.
- "Fixing" a flaky check by deleting it.
- Treating `cargo check` success as proof that a Win32 flow works.
- Implementing behaviour in the frontend that belongs in Rust (click semantics, ordering,
  filtering).
- Reporting work as done without running the listed commands.
- Expanding the task because the surrounding code "looked wrong". Note it; do not fix it.
- Windows paths in tests on Linux: `std::path::Path` does not treat `\` as a separator there, so
  `file_stem()` and `parent()` give different answers than on Windows. Code that must behave the
  same in tests on any host uses explicit string handling (see `stem_core::normalize_path`).
- ts-rs: `#[ts(export)]` writes to `TS_RS_EXPORT_DIR` (set in `.cargo/config.toml`); do not add
  `export_to` paths. `#[serde(transparent)]` newtypes export as their inner type.
- `--workspace` on Linux includes `stem-app`, which cannot build there; use the per-crate commands
  of the [verification ladder](verification-ladder.md) (or `scripts/check-all.sh`).
- Sharing one `CARGO_TARGET_DIR` between git worktrees of this repository. Cargo hashes
  workspace-member paths relative to the workspace root, so two worktrees produce identical
  artifact names and reuse each other's compiled tests and lint results without a warning. Every
  worktree uses its own `target/`.
