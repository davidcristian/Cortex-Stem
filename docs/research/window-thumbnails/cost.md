# Research: the cost of window previews

This page lists what the window previews of
[ADR-0009](../../adr/0009-window-thumbnails.md) cost while no preview
strip is open and while one is open. A1 to A11 name the assumptions listed
in the ADR's [Assumptions](../../adr/0009-window-thumbnails.md#assumptions).

- DWM draws the images on the GPU while it composes. No pixels pass through
  Stem. No capture, no copies, no timers.
- Opening costs one register call, one size query and one update per card.
  Closing costs one unregister per card. Nothing is registered while no
  strip is open, so the idle cost stays at about 0 % CPU
  ([Performance budgets](../../architecture/performance.md)).
- While a strip is open, an image is refitted only when its source moves or
  resizes. The events thread already re-queries moved windows 150 ms after a
  burst. For a source that has a registration, it also posts a
  refit to the dock window that holds it. No poll.
- The webview repaints only the strip's frame, on open, close and hover.
- A live source, such as a video, costs DWM some GPU time while its strip is
  open. Step 16 of the [manual test plan](manual-test-plan.md) measures it
  (A10).
- The band adds a transparent surface: about 0.8 MB at 1920 px and 100 %,
  about 3.4 MB at 3840 px and 200 % (an estimate: one 4-byte pixel per
  physical pixel).
