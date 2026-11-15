# Research: work items for window previews

These are the pieces of work that build the window previews of
[ADR-0009](../../adr/0009-window-thumbnails.md).

- **DWM thumbnail probe on Windows.** The probe is a throwaway program:
  `crates/stem-platform/examples/thumbnail-probe.rs` runs
  `stem_platform::windows::probe::thumbnails`. It registers two normal
  windows, a minimised one, a cloaked one, two with a display affinity, an
  elevated one, a Store app's window and a window on another desktop into a
  plain test window, and records what each shows, the source sizes and the
  call times. One run on Windows settles
  the assumptions it covers
  ([`docs/research/thumbnail-probe/`](../thumbnail-probe/index.md)).
- **Platform thumbnails.** `Platform::set_window_thumbnails`,
  `windows/thumbnails.rs` in the dock subclass, pure `rules::thumbnail`
  (fit, crop, diff), refit on source moves, hide before click-through.
- **Core, service and the next IPC minor.** `WindowInfo.preview` from a pure
  rule, `behavior.windowPreviews`, `set_window_previews` with service
  checks and the app-layer conversion, `MockBackend` and the mock scenario.
- **The preview strip in the UI.** Pure `preview.ts` machine,
  `PreviewStrip.svelte`, the reserved band, the envelope, the `preview`
  auto-hide hold, the hidden tooltip, the settings row, and Playwright
  screenshots of the three kinds of card.
- **Keyboard access to the strip.** The up arrow on a
  focused item opens its strip, the arrows move between cards, Enter
  activates, Delete closes; cards are buttons named by their titles.
- **Dimmed pictures of minimised windows.** Only if the probe finds
  that DWM keeps a usable last frame: register minimised windows at reduced
  opacity instead of showing the icon.
