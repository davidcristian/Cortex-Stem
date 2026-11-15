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
| 3c | `STEM_UPDATE_SETTINGS_REFERENCE=1 cargo test -p stem-core --test settings_reference`, then `git status --porcelain -- docs/SETTINGS.md` prints nothing | `docs/SETTINGS.md` is regenerated from `settings.rs` and committed |
| 4 | `cargo clippy --workspace --all-targets --target x86_64-pc-windows-msvc -- -D warnings`, plus `cargo clippy -p stem-platform --all-targets --features probes --target x86_64-pc-windows-msvc -- -D warnings` for the diagnostic probes (and, on a Windows host, `cargo test -p stem-platform --features probes --lib windows::probe::`) | the **real** Windows code, the Tauri crate and the probes included, type-checks and lints |
| 5 | `pnpm install --frozen-lockfile && pnpm check && pnpm lint && pnpm test` | frontend types; lint (ESLint, Prettier, `scripts/prosecheck.mjs` and `scripts/linecap.mjs`); unit tests |
| 6 | `pnpm build` | the production frontend bundle builds |
| 7 | `pnpm e2e:screenshots` | the pages render in mock mode (screenshots in `e2e/output/`) |
| 8 | GitHub Actions `windows` job | `pnpm tauri build` links and bundles on Windows; the probe unit tests pass |
| 9 | A person on Windows runs the manual tests ([AGENTS.md §11](../../AGENTS.md#11-manual-tests-on-windows)) | the behaviour works |

## Scripts and CI

`scripts/check-all.sh` runs rungs 1 to 7 on Linux, and CI's `linux` job runs the same commands as
separate steps. `scripts/check-all.ps1` runs the same rungs on a Windows host, then
`pnpm tauri build --debug`.

## Rung 4 on a host other than Windows

On a non-Windows host rung 4 needs `llvm-rc` (package `llvm`, or
`RC_x86_64_pc_windows_msvc=/path/to/llvm-rc-NN`), because tauri-build compiles a Windows resource
file in its build script.

## The Rust toolchain

`rust-toolchain.toml` sets the toolchain to an exact version, so every machine and CI use the same
compiler; a floating `stable` differs between machines and can fail the build on a new lint or
deprecation. Change the version on purpose, in its own commit, after running the whole ladder.
