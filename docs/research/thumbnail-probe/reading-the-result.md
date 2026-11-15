# Thumbnail probe: reading the result

Each assumption below says what in the [thumbnail probe](index.md)'s
[output](output.md) settles it, and what each outcome means for
[ADR-0009](../../adr/0009-window-thumbnails.md) and its plan. The slots and
phases named here are described in [What the probe does](what-it-does.md).

- **A1, DWM draws above the destination's content.** Partly settled. A
  picture that covers the grey fill and the "no picture here" text shows
  that DWM draws the thumbnail above the destination's own GDI content. The
  dock's content is WebView2, composed differently, so A1 still needs a
  check on the dock window itself.
- **A2, `rcDestination` is in client coordinates.** Compare each picture
  with its yellow outline, which is drawn in client coordinates. A picture
  exactly inside its outline means client coordinates. A picture shifted up
  and left by about the caption height and border means window coordinates.
  The probe window has a caption. The dock window has none (tao removes the
  frame in `WM_NCCALCSIZE`), so on the dock the two coordinate systems are
  equal either way. Only an offset would need a note in the platform code.
- **A3, a minimised source gives no usable picture.** Slot 3. Grey or black
  means no picture, and the icon card stays. A picture whose clock does not
  move means DWM keeps a last frame, and dimmed pictures of minimised
  windows become possible. A ticking clock would mean a live picture even
  while minimised. Note the `size (DWM)` column for slot 3 too.
- **A4, a cloaked source gives no usable picture.** Slots 4 (cloaked by its
  owner, expected `cloak 0x1`), 8 (Calculator: `iconic` when minimised, a
  `cloak` value when the shell has cloaked it) and 9 (another desktop,
  expected `cloak 0x2`, the shell's). The `state` column shows what each
  one was. A live picture in 4 or 9 means the `Elsewhere` rule in ADR-0009
  may show pictures instead of icon cards (one line to change). No picture
  keeps the rule.
- **A5, thumbnails on a layered destination are unreliable.** Phase 3. If
  the pictures vanish or freeze in 3.1 or 3.2 and come back only in 3.4, the
  ADR's rule (unregister before the window turns layered, register afresh
  after) is required. If they come back in 3.3, removing the style is
  enough. If nothing changes at all, the rule does no harm but is not
  needed. Failed updates in 3.2 are recorded in the A5 step 2 line. Stem's
  click-through sets `WS_EX_LAYERED | WS_EX_TRANSPARENT` without
  `SetLayeredWindowAttributes`. The probe sets alpha 255 and no
  `WS_EX_TRANSPARENT`, which only changes hit-testing. The real dock needs
  its own check.
- **A6, the calls are quick and have no thread affinity beyond the
  process.** The `A6` lines. No call over 16 ms keeps the DWM calls on the
  dock window's thread, as the ADR plans. Slow calls move them to a
  platform thread. The A6 worker-thread line, with `ok` results and slot 2
  showing its picture after phase 2, shows that registering, updating and
  unregistering work from a thread that does not own the destination. The
  probe window keeps pumping its messages during phase 2, as the dock's UI
  thread would, so a worker call that waits for the destination's thread
  shows as a slow worker call rather than a hang. A line
  `A6 worker thread: did not finish within … ms: still in <call>` means
  that call blocks when made from another thread; Stem then keeps every DWM
  call on the dock window's thread. The probe stops after that line
  (summary first), so that run does not settle A5 and A11.
- **A7, the source size includes the invisible resize borders, and cropping
  to the frame removes them.** Slots 1 and 2 in the table. `size (DWM)`
  equal to `window` and larger than `frame` means the source includes the
  borders. Then slot 1 should show no margin and slot 2 a transparent margin
  of a few pixels on the left, right and bottom. `size (DWM)` equal to
  `frame` means no crop is needed. A crop line saying the frame `is outside
  the …x… source` means the rectangles disagree, for example for a source
  at another scale. Look at slots 7 to 9 for the same columns.
- **A8, an elevated source can be shown by a process that is not
  elevated.** Slot 7, with its line saying `elevated` and the summary saying
  the probe is `not elevated`. `register ok` and a picture confirm A8. A
  failure (`0x80070005`) or an empty picture means elevated windows need the
  icon card in the preview rule.
- **A9, `WDA_MONITOR` and `WDA_EXCLUDEFROMCAPTURE` sources show no
  content.** Slots 5 and 6. Record two things: what you see on the monitor
  and what your screenshot shows, because a screenshot is itself a capture
  and may hide the picture differently. The ADR accepts either outcome,
  since the source app sets its display affinity on purpose. The record
  shows what the card will look like.
- **A10, a registration costs DWM little.** Not settled by the probe.
  ADR-0009's manual test plan records the CPU of `dwm.exe` with a preview
  strip open. Optionally, note `dwm.exe`'s CPU in Task Manager (Details)
  while the grid is open and after the probe has exited.
- **A11, a second registration of one source may stop the first.** Phase 4.
  Slots 1 and 10 both live in 4.1, and slot 1 still live in 4.2, mean that
  on this build a second registration does no harm. The ADR's rule (one
  registration per source, process-wide) stays as a safety measure. Slot 1
  going grey or frozen means the rule is required.

## Limits

- A run covers one machine, one build and one display scale. The summary
  prints the build and the DPI. To cover another scale, change the display
  scale and run the probe again.
- The destination is drawn with GDI, not WebView2, and has a caption. A1
  and A2 are settled for that window; the dock needs its own check.
- The found windows depend on what is open. A slot that says "not found"
  tells you what to open; run the probe again.
- The probe does not measure GPU cost (A10) and does not test a destination
  on a second monitor. To see that, move the probe window to another
  monitor during a pause (the grid keeps its physical size).
