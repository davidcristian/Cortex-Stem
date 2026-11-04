# IPC contract: Rust ⇄ frontend

**Contract version: 1.7 (file drops).**
The files in this folder are the contract between the Rust app (`src-tauri`) and the pages in
`src/`. A change to the contract follows the contract change rules in `AGENTS.md` ("IPC contract
change control") and is logged in the [changelog](changelog.md).

## Principles

- **Rust is the source of truth.** The UI renders a `DockState` snapshot and reports *intent*
  ("the user clicked item X"). Click semantics, ordering, grouping and filtering are in
  `stem-core` and `stem-service`.
- **Payload types are `stem-core` types.** serde serialises them with `camelCase` fields, and
  ts-rs mirrors them in `src/lib/ipc/generated/*.ts` (`cargo test -p stem-core --features ts`).
  The Rust doc comments document the types; these files document the *flows*.
- **Commands** are `invoke(name, args)`. Argument keys are the Rust parameter names in
  `camelCase` (Tauri converts them). A failed command rejects with a `string`.
- **Events** are `listen(name, handler)`. Each payload is a full snapshot, not a diff.
  `DockState.revision` never decreases, and the UI ignores a snapshot older than the one it
  shows.
- The frontend never calls `invoke` or `listen` directly. It uses the `Backend` interface in
  `src/lib/ipc/backend.ts`. `MockBackend` implements the same contract for runs in a browser and
  in Playwright.

## Files

| File | What it covers |
| --- | --- |
| [windows.md](windows.md) | The app's windows and their pages |
| [events.md](events.md) | The events that Rust sends to the pages, and the startup sequence |
| [commands.md](commands.md) | The commands that the pages call |
| [sizing-protocol.md](sizing-protocol.md) | How Rust sizes a dock window |
| [hit-region.md](hit-region.md) | Where a dock window takes clicks, and the reveal strip of auto-hide |
| [context-menus.md](context-menus.md) | The native right-click menus and their entries |
| [icon-keys.md](icon-keys.md) | The `shell:` icon keys and the icons that `get_icon` returns |
| [file-drags.md](file-drags.md) | Files dragged from Explorer: pinning them |
| [app-shell-settings.md](app-shell-settings.md) | The settings that the Tauri layer applies itself |
| [changelog.md](changelog.md) | Every contract version and what it changed |
