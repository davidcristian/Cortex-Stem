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

- creates and positions the dock windows and the settings window;
- maps commands to `DockService` methods;
- forwards `ServiceEvent`s as Tauri events;
- wires the plugins (single-instance, logging, autostart, global shortcut)
  and the tray icon, and applies the settings that belong to the shell
  (autostart, hiding the taskbar, reserving screen space, the shortcuts,
  click-through);
- reports a crash of the previous run at the next start, and reads the
  user's `dock.css`.

## Consequences

- A command's behaviour is tested without Tauri; the Tauri command only
  validates and clamps its arguments and calls the service
  (`src-tauri/src/commands/`).
- `src-tauri` holds only adapter code, so most of the risk of the Windows
  build is in `stem-platform/src/windows`, which is where the manual tests
  on Windows focus.
- Listener callbacks run on the service's worker thread (on the caller's
  thread for `Bounce`); the Tauri layer only calls `emit`, which is
  thread-safe.
