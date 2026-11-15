# Report provenance (v1.18)

The page measures its layout, then reports it. Rust handles a report later, on its main thread,
possibly after it has moved or resized the window. Two fields let Rust read every geometry report
against the layout it was measured in:

- `edge` on `set_dock_content_size`, `set_dock_hit_region` and `set_dock_panel_rect` is the
  `Settings.position` that the page had laid itself out for when it measured. Rust stores, for
  each dock window, the edge it last placed that window on. A report that names another edge was
  measured before the page received a position change, and Rust drops it (one `debug` line). The
  page's next reports, laid out for the new edge, are applied as usual.
- `viewport` on `set_dock_panel_rect` is the page's viewport that the rect is relative to
  (`{ width, height }`, integers in CSS px: `window.innerWidth` × `window.innerHeight`).

  When the window's viewport has changed since (because the content size that the page sent in
  the same tick was applied first), Rust re-anchors the rect to the window's current viewport
  before it converts it. Across the edge the rect keeps its distance to the viewport's edge
  side; along the edge it keeps its offset from the viewport's centre. This is the page's own
  layout (the panel anchored to the edge side and centred along it, see
  [Sizing protocol](sizing-protocol.md)), so the result is where the panel is now. The engine's
  panel (`overlapped`), the reserved band (`reserveScreenSpace`: its thickness is the distance
  from the panel's far side to the viewport's edge side, read against `viewport`) and the
  backdrop prototype all use the re-anchored rect. None of them waits for the page to report an
  unchanged rect again.
- A negative, zero or non-finite `viewport` value rejects the report. The `viewport` type is
  defined in the app layer; no `stem-core` type crosses IPC for it.
- `MockBackend` records both fields and converts with them in the same way.
