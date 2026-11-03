# Verification ladder

The verification ladder is the ordered list of checks a change to Stem must pass, rungs 1 to 9.
The rules for running it, and which rungs a task needs, are in
[AGENTS.md §5](../../AGENTS.md#5-verification-ladder).

## The rungs

| Rung | Command | What it proves |
| --- | --- | --- |
| 1 | `cargo fmt --all --check` | formatting |
| 2 | `cargo clippy -p stem-core -p stem-platform -p stem-service --all-targets -- -D warnings` | lints on the host target (the Tauri crate needs WebKitGTK to build on Linux, so rung 4 covers it) |
| 3 | `cargo test -p stem-core -p stem-platform -p stem-service` | every Linux-runnable test (core, mock, service) |
| 3b | `cargo test -p stem-core --features ts`, then `git status --porcelain -- src/lib/ipc/generated` prints nothing | the TypeScript bindings are regenerated and committed |
| 4 | `cargo clippy --workspace --all-targets --target x86_64-pc-windows-msvc -- -D warnings` | the **real** Windows code, the Tauri crate included, type-checks and lints |
| 5 | `pnpm install --frozen-lockfile && pnpm check && pnpm lint && pnpm test` | frontend types; lint (`scripts/prosecheck.mjs` and `scripts/linecap.mjs`); unit tests |
| 6 | `pnpm build` | the production frontend bundle builds |
| 7 | `pnpm e2e:screenshots` | the dock renders in mock mode (screenshots in `e2e/output/`) |
| 8 | GitHub Actions `windows` job | `pnpm tauri build` links and bundles on Windows |
| 9 | A person on Windows runs the manual tests ([AGENTS.md §11](../../AGENTS.md#11-manual-tests-on-windows)) | the behaviour works |

## Scripts and CI

`scripts/check-all.sh` runs rungs 1 to 7 on Linux, and CI's `linux` job runs the same commands as
separate steps. `scripts/check-all.ps1` runs the same rungs on a Windows host, then
`pnpm tauri build --debug`.
