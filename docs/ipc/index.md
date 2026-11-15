# IPC contract: Rust ⇄ frontend

**Contract version: 1.21 (quick settings and notification items).**
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
| [windows.md](windows.md) | The app's windows and their pages, and the dock windows per monitor |
| [events.md](events.md) | The events that Rust sends to the pages, and the startup sequence |
| [state-snapshot.md](state-snapshot.md) | `DockState`: the per-dock facts |
| [commands.md](commands.md) | The commands that the pages call |
| [sizing-protocol.md](sizing-protocol.md) | How Rust sizes a dock window, and the layout on each edge |
| [report-provenance.md](report-provenance.md) | The `edge` and `viewport` fields that tie a geometry report to the layout it was measured in |
| [hit-region.md](hit-region.md) | Where a dock window takes clicks, and the reveal strip of auto-hide |
| [dock-panel.md](dock-panel.md) | The resting panel rect that the `overlapped` fact is computed from |
| [toggle-while-hidden.md](toggle-while-hidden.md) | How the tray click and the toggle shortcut bring back a panel that auto-hide slid out |
| [keyboard-entry.md](keyboard-entry.md) | The focus shortcut that gives a dock window the keyboard focus, and Escape |
| [context-menus.md](context-menus.md) | The native right-click menus and their entries |
| [icon-keys.md](icon-keys.md) | The `shell:` icon keys and the icons that `get_icon` returns |
| [file-drags.md](file-drags.md) | Files dragged from Explorer: pinning them, and dropping them on the Recycle Bin |
| [themes.md](themes.md) | The theme presets and the user's `dock.css` |
| [about.md](about.md) | The settings page's About section |
| [app-shell-settings.md](app-shell-settings.md) | The settings that the Tauri layer applies itself |
| [changelog.md](changelog.md) | Every contract version and what it changed |
