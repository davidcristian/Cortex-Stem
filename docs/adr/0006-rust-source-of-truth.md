# ADR-0006: Rust owns state and semantics; the UI renders snapshots

Date: 2026-11-03 · Status: accepted

## Context

When behaviour is split between a webview and a backend, two copies of a
rule can come to differ, and logic in the webview cannot be tested with
the backend's tests. The engine's rules (grouping, ordering, what a click
does) must be unit-tested and must not depend on the frontend's timing.

## Decision

- `stem-core::DockEngine` is the single reducer of platform events and
  settings into `DockState`, and the single place that decides what a
  click does (`primary_action`).
- The frontend receives full `DockState` snapshots (`dock://state`) with a
  monotonic `revision`, and sends intent (`primary_click(itemKey)`), not
  decisions.
- Payload types are the Rust types, serialised with serde in `camelCase` and
  exported to TypeScript with ts-rs. There are no hand-maintained
  TypeScript copies.
- The UI keeps only presentation state (hover, magnification, animation,
  icon cache).

## Consequences

- Behaviour tests live in Rust and run on Linux.
- Snapshots are a few KB, so sending them whole is simpler than sending
  diffs. If needed, the service coalesces the notifications it sends.
- Contract changes follow the IPC change control in `AGENTS.md`.
