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
├── crates/stem-platform/   `Platform` trait and implementations: src/lib.rs and src/platform/
│                           (trait, errors, `create_platform()`), src/mock.rs and src/mock/
│                           (scripted fake, every host), src/rules/ (pure rules the Windows
│                           code calls), src/windows/ (all unsafe and Win32 code).
├── crates/stem-service/    Engine thread, identity resolution, settings persistence, command
│                           semantics. No tauri. Tested on Linux with the mock.
├── src-tauri/              App crate `stem-app` (binary `stem`): a thin layer that creates
│                           windows, maps commands to the service and sends events.
├── ui/                     Svelte 5 and TypeScript frontend (Vite, pnpm): package.json and the
│                           configs, src/dock/ and src/settings/ windows, src/lib/ipc/ (the
│                           `Backend` interface, Tauri and mock implementations),
│                           src/lib/ipc/generated/ (ts-rs output; never edit), e2e/
│                           (Playwright specs: mock-mode screenshots and the idle check).
├── scripts/                check-all.sh and check-all.ps1 (the ladder), prosecheck.mjs and
│                           linecap.mjs (run by `pnpm lint`).
├── docs/                   architecture/ (read before writing code), ipc/ (the IPC contract,
│                           AGENTS.md §8), development/ (the detail behind AGENTS.md),
│                           SETTINGS.md (generated, rung 3c), PLAN.md (milestones), adr/.
└── .github/workflows/      CI (Linux checks, Windows build) and the release build.
```
