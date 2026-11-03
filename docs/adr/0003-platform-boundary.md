# ADR-0003: One platform boundary; `unsafe` only behind it

Date: 2026-11-03 · Status: accepted

## Context

The Win32 code is where bugs are hardest to find: leaked handles, hung
cross-process calls, UIPI failures, DPI mistakes. It is also the code that
cannot run on the hosts where most development happens (ADR-0004). It must
be small, isolated and easy to read.

## Decision

- `crates/stem-platform` exposes a `Platform` trait whose signatures use
  only `stem-core` types. It has a scripted `MockPlatform` and a Windows
  implementation under `src/windows/`.
- `unsafe` is allowed only in `crates/stem-platform/src/windows/`. Every
  block has a `// SAFETY:` comment, and `clippy::undocumented_unsafe_blocks`
  is denied for every crate of the workspace. `stem-core` and
  `stem-service` are `#![forbid(unsafe_code)]`, and so is `stem-platform`
  when it is built for a host other than Windows.
- Blocking or message-loop work runs on threads that the platform crate
  starts.
- Cross-process window messages use `SendMessageTimeoutW`.
- The Windows module is split by concern (enumeration, events, icons,
  actions, the dock window and so on), with fixed function signatures
  between the parts, so each part can be written and changed on its own.

## Consequences

- Most logic is testable with the mock (engine, service, UI).
- Adding a platform capability takes these steps: core type → trait
  method (its default returns `Unsupported`) → mock → Windows → manual test
  on Windows.
- The cost is some indirection and a few more types; accepted.
