# Backdrop research: remaining work

This page lists what the [backdrop research](index.md) leaves to build.

- **Backdrop window prototype.** The window of
  [option 6](backdrop-window.md) for the primary dock, sized at rest from
  `set_dock_panel_rect` and driven by `appearance.backdrop` in
  `settings.json`, with no UI or IPC change. Its results settle A2 to A10
  ([Assumptions](assumptions.md)) through steps 2 to 15 of the
  [manual test plan](manual-test-plan.md).
- **An ADR (next free number): backdrop window.** It records
  [option 6](backdrop-window.md) or the decision not to ship, the z-order rule, the shape, and the main-thread
  exception to the threading rule in `AGENTS.md`. It is written once the
  prototype's test results are in.
- **Backdrop window in the platform.**
  `Platform::set_dock_backdrop` takes rect, radius, material and
  theme (or `None`), the dock subclass and its topmost re-assert restack
  the backdrop, the mock records the calls, and other platforms return
  `Unsupported`.
- **Live panel box and effective backdrop (IPC change).** The UI reports
  the live panel box while it changes and `null` during the slide. Rust
  reports the effective backdrop from a pure `stem-core` rule. Both
  backends and the [IPC contract](../../ipc/index.md) change.
- **Panel over a native material (UI).** No `backdrop-filter` and a
  lighter tint when the effective backdrop is native; *Acrylic* and *Mica*
  enabled where supported; the Playwright assertions and the user guide
  updated.
- **A name for the built-in look that says what it draws.** If step 1 of
  the [manual test plan](manual-test-plan.md) shows no blur, rename *Blur (built in)* in the settings page, the user
  guide and the README.
- **Remove `window-vibrancy` from `[workspace.dependencies]`.** No
  workspace crate uses it; Tauri keeps its own copy.
