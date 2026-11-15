# IPC contract: Rust ⇄ frontend

**Contract version: 1.25 (the clock's regional format).**
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
  shows. A page listens on its own window (`TauriBackend` passes the target
  `{ kind: 'WebviewWindow', label }`). So an event that Rust sends to one window (`emit_to`,
  written "to one dock window" in these files) reaches only that window's page. A broadcast
  (`emit`) reaches every page, and an event sent to every dock window (`emit_filter`) reaches
  every dock page. A listener without a target (`{ kind: 'Any' }`, the default of `listen`) would
  also receive the events sent to other windows.
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
| [scrolling-over-an-item.md](scrolling-over-an-item.md) | What the mouse wheel does over a dock item |
| [widget-items.md](widget-items.md) | The clock and battery items, and the clock's regional format |
| [icon-keys.md](icon-keys.md) | The `shell:` icon keys and the icons that `get_icon` returns |
| [hidden-apps.md](hidden-apps.md) | Hiding a running app from every dock and showing it again |
| [file-drags.md](file-drags.md) | Files dragged from Explorer: pinning them, and dropping them on the Recycle Bin |
| [themes.md](themes.md) | The theme presets and the user's `dock.css` |
| [about.md](about.md) | The settings page's About section |
| [app-shell-settings.md](app-shell-settings.md) | The settings that the Tauri layer applies itself |
| [changelog.md](changelog.md) | Every contract version and what it changed |
