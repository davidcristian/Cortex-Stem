# Repository map

The repository map shows what each top-level directory and each crate of Stem holds. Who may edit
which path is set by the ownership rule in
[AGENTS.md §2](../../AGENTS.md#2-repository-map-and-ownership).

```text
Cortex-Stem/
├── AGENTS.md, CLAUDE.md    The engineering rules. Orchestrator-owned.
├── Cargo.toml              Workspace root; every third-party Rust crate is declared here.
├── crates/stem-core/       Pure domain logic: no platform code, no unsafe, no tauri. Fully
│                           tested on Linux.
├── crates/stem-platform/   `Platform` trait and implementations: src/lib.rs (trait, errors,
│                           `create_platform()`), src/mock.rs (scripted fake, every host),
│                           src/windows/ (all unsafe and Win32 code).
├── crates/stem-service/    Engine thread, identity resolution, settings persistence, command
│                           semantics. No tauri. Tested on Linux with the mock.
├── src-tauri/              App crate `stem-app` (binary `stem`): a thin layer that creates
│                           windows, maps commands to the service and sends events.
├── src/                    Svelte 5 and TypeScript frontend (Vite): dock/ and settings/
│                           windows, lib/ipc/ (the `Backend` interface, Tauri and mock
│                           implementations), lib/ipc/generated/ (ts-rs output; never edit).
├── e2e/                    Playwright specs: mock-mode screenshots.
├── scripts/                check-all.sh (the ladder), prosecheck.mjs and linecap.mjs (run by
│                           `pnpm lint`).
├── docs/                   architecture/ (read before writing code), ipc/ (the IPC contract,
│                           AGENTS.md §8), development/ (the detail behind AGENTS.md),
│                           PLAN.md (milestones), adr/.
└── .github/workflows/      CI (Linux checks, Windows build).
```
