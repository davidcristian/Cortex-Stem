# Hit region

The dock window must not take clicks meant for the desktop or for the windows behind its
transparent parts. The UI tells Rust where the visible dock is:

- `set_dock_hit_region({ panel, envelope })`: `panel` is the panel's bounding rect at rest.
  `envelope` is the largest rect that a magnified icon (or a lifted, dragged icon) can occupy,
  and it contains `panel`. Both are integers in CSS px relative to the calling window's viewport
  (`getBoundingClientRect` rounded outwards). Rust multiplies them by the window's scale factor.
- Rust makes the window click-through while the cursor is outside `panel`, clickable once the
  cursor enters `panel`, and click-through again only after the cursor has left `envelope`. This
  hysteresis (a larger rect to leave than to enter) keeps icons that grew past the panel
  clickable. Keyboard input is unaffected.
- The UI reports whenever the panel's **rest** geometry changes (startup, items added or
  removed, icon size, separators), never per magnification frame. While a drag-to-reorder is in
  progress, the UI reports the whole viewport as `envelope` and restores the real one afterwards.
- Only dock windows may call it. Rust rejects non-dock windows and negative sizes. On a platform
  without the feature (the mock, non-Windows hosts), the call is recorded or ignored and the
  window stays fully clickable.
