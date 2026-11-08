# Architecture

Stem is a floating dock for Windows 11. The files in this folder describe how its
parts fit together. [`docs/ipc/`](../ipc/index.md) defines the exact contract
between Rust and the UI, [`docs/PLAN.md`](../PLAN.md) lists the milestones, and
the records in `docs/adr/` give the reasons for the main decisions.

## Files

- [layers.md](layers.md): the layers of the code, what each crate depends on and how it
  is tested.
- [data-flow.md](data-flow.md): how events from Windows reach the UI, how a click reaches
  Windows, and which facts about the screen the engine publishes.
- [threads.md](threads.md): every thread Stem starts, what runs on it, and the rules for
  which work runs where.
- [identity.md](identity.md): how windows and pins are grouped into apps, and how an app
  gets its name.
- [icons.md](icons.md): how icons are loaded, sized and cached.
- [dock-window.md](dock-window.md): the dock's native window, its flags and focus,
  click-through outside the panel and how it stays topmost.
- [dock-placement.md](dock-placement.md): where the dock window is placed, and DPI
  changes.
- [auto-hide.md](auto-hide.md): how the dock hides its panel and brings it back, and its
  behaviour over a fullscreen app.
- [theme.md](theme.md): how the dock and the settings window follow the theme.
- [dock-items.md](dock-items.md): the fixed items and the attention cue.
- [file-drops.md](file-drops.md): files dragged from Explorer onto the dock, pinned or
  moved to the Recycle Bin.
- [settings.md](settings.md): how settings are stored and how the app shell applies the
  ones that belong to it.
- [error-handling.md](error-handling.md): the error type of each layer and how a crash is
  reported.
- [performance.md](performance.md): the performance budgets.
- [security.md](security.md): what the webview and Stem's commands may do.
- [development-hosts.md](development-hosts.md): how the Windows-only product is developed
  and checked.
