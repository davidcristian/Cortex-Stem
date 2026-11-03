# ADR-0007: A `stem-service` crate keeps the Tauri crate thin

Date: 2026-11-03 · Status: accepted

## Context

The Tauri crate cannot be built or unit-tested on Linux hosts (it needs
WebKitGTK to link). Orchestration logic placed there (engine thread,
identity resolution, settings persistence, command semantics) could not be
tested on the Linux hosts where most development happens.

## Decision

`crates/stem-service` holds all of that, depending only on `stem-core` and
the `Platform` trait. It is integration-tested on Linux with `MockPlatform`
and an in-memory settings store (`MemorySettingsStore`). `src-tauri` only:

- creates and positions the dock window and the settings window;
- maps commands to `DockService` methods;
- forwards `ServiceEvent`s as Tauri events;
- wires the plugins (single-instance, logging) and the tray icon.

## Consequences

- A command's behaviour is tested without Tauri; the Tauri command is one
  line that calls the service.
- `src-tauri` holds only adapter code, so most of the risk of the Windows
  build is in `stem-platform/src/windows`, which is where the manual tests
  on Windows focus.
- Listener callbacks run on the service's worker thread (on the caller's
  thread for `Bounce`); the Tauri layer only calls `emit`, which is
  thread-safe.
