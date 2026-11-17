# Layers

Stem's code is in five parts: the Svelte frontend, the thin Tauri shell, the service, the pure
core and the platform. The diagram shows how they call each other, and the table shows what
each part depends on and how it is tested.

```
┌────────────────────────────────────────────────────────────────────────┐
│  src/  (Svelte 5 + TypeScript, WebView2)                               │
│  dock.html  settings.html     Backend interface → TauriBackend | Mock   │
└───────────────▲───────────────────────────────┬────────────────────────┘
                │ events (dock://*)             │ commands (invoke)
┌───────────────┴───────────────────────────────▼────────────────────────┐
│  src-tauri/  crate stem-app  — THIN SHELL                              │
│  windows.rs + windows/ (create/position dock windows)                  │
│  commands.rs + commands/   lib.rs + app.rs + app/ (wiring)             │
└───────────────────────────────┬────────────────────────────────────────┘
                                │ Rust API
┌───────────────────────────────▼────────────────────────────────────────┐
│  crates/stem-service — orchestration (no OS, no tauri)                 │
│  DockService: engine thread, identity resolution, settings persistence,│
│  command semantics, listeners                                          │
└──────────────┬───────────────────────────────┬─────────────────────────┘
               │ EngineInput / DockState       │ Platform trait
┌──────────────▼───────────────┐  ┌────────────▼──────────────────────────┐
│ crates/stem-core (pure)       │  │ crates/stem-platform                  │
│ model · settings · events ·   │  │ trait Platform · mock · windows/ (the │
│ DockEngine (state machine)    │  │ only unsafe/Win32 code in the repo)   │
└───────────────────────────────┘  └───────────────────────────────────────┘
```

| Crate / dir      | Depends on                   | Runs on Linux? | Tested how |
|------------------|------------------------------|----------------|------------|
| `stem-core`      | serde, serde_json, thiserror | yes            | unit tests (engine, settings) |
| `stem-platform`  | core; `windows` on Windows   | mock: yes      | mock tests; the Windows code is type-checked for the Windows target |
| `stem-service`   | core, platform               | yes (mock)     | integration tests with `MockPlatform` |
| `stem-app`       | all + tauri                  | type-check only (a Linux build needs WebKitGTK) | `cargo clippy --target x86_64-pc-windows-msvc`, CI build on Windows |
| `src/`           | `@tauri-apps/api`            | yes (mock)     | Vitest, Playwright screenshots in mock mode |
