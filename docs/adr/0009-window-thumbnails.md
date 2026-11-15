# ADR-0009: Window previews draw DWM thumbnails into the dock window

Date: 2026-11-08 · Status: proposed

## Context

When the pointer rests on an app's icon, should the dock show live
pictures of that app's windows, as the stock taskbar does? The proposal
under study is DWM thumbnails (`DwmRegisterThumbnail`) in a separate
preview window. The [options](../research/window-thumbnails/options.md)
compare it with no thumbnails, DWM thumbnails in the dock window itself,
and captured bitmaps. If the assumptions below hold, window previews are
built as recommended.

**What DWM offers.** A DWM thumbnail is not a snapshot. It is a live link
between a source window and a rectangle in a destination window, and DWM
draws it while it composes the screen ([overview][ovw]).
`DwmRegisterThumbnail` creates the link and returns a handle. Nothing shows
until `DwmUpdateThumbnailProperties` sets a destination rectangle and turns
the thumbnail visible. `DwmQueryThumbnailSourceSize` gives the source size.
`DwmUnregisterThumbnail` ends the link. Both windows must be top-level
windows, or the call returns `E_INVALIDARG`. The destination must belong to
the calling process. The handle is unique only within the process, and that
process must unregister it ([register][reg], [update][upd],
[size][qsz], [unregister][unreg]). The properties are a destination
rectangle, an optional source rectangle, an opacity, a visibility flag and
"client area only" ([`DWM_THUMBNAIL_PROPERTIES`][props],
[`DWM_TNP_*`][tnp]). No pixels pass through the calling process.

**What Stem has without previews.**

- Each dock window spans the length of one edge of its monitor's work area
  (`settings.position`; this ADR describes the default, `bottom`). Its
  thickness, the height on `bottom`, is the content size the UI reports
  (`set_dock_content_size`). It is transparent, topmost, `WS_EX_TOOLWINDOW`
  and `WS_EX_NOACTIVATE` (tao keeps the bit from `focusable(false)`). The
  platform subclass returns `MA_NOACTIVATE` for `WM_MOUSEACTIVATE`, and
  the WebView2 child windows have `WS_EX_NOACTIVATE` too
  (`windows/dock_window.rs`). Clicking the dock never takes the foreground.
- Click-through: the hit-test thread tracks the cursor and the
  subclass toggles `WS_EX_LAYERED | WS_EX_TRANSPARENT` on the window's own
  thread. The window becomes clickable when the cursor enters `panel` and
  click-through again only after it leaves `envelope`. When the window
  turns click-through under a pointer that does not move, no
  `pointerleave` arrives (`Dock.svelte`).
- The window can grow: during a drag it grows to the work area's height and
  shrinks back afterwards. That path resizes the window.
- The engine stores, per window, whether it is minimised, cloaked, on the
  current virtual desktop, and its frame (`RawWindow`). The snapshot gives
  the UI each window's id, title, minimised, active and attention flags,
  monitor and icon (`WindowInfo`).
- With several windows of one app, a click cycles them, the right-click menu
  lists them by title, and the tooltip shows the app's name after 300 ms.
- The locked `windows` crate (0.62.2) has the four functions and
  `DWM_THUMBNAIL_PROPERTIES` under `Win32_Graphics_Dwm`, which
  `stem-platform` enables. The thumbnail probe
  (`windows/probe/thumbnails.rs`) calls all four, and it compiles for
  `x86_64-pc-windows-msvc`. The struct is `#[repr(C, packed(1))]`, so it
  must be built by value and its fields never borrowed.

## Recommendation

**Implement
[option 3](../research/window-thumbnails/options.md#3-the-dock-window-is-the-destination-recommended)**,
behind a new setting that is on by default. Before that, run the thumbnail
probe on Windows once (see Assumptions) to settle the open DWM questions.
If the probe shows that DWM draws nothing into the dock window at all,
[option 2b](../research/window-thumbnails/options.md#2-a-separate-topmost-tool-window-the-proposal)
is the fallback, and a new ADR supersedes this one.

In option 3, the dock page draws the frame of the strip (the row of
preview cards, one per window) above the panel, in a band the dock window
keeps free for it. The platform registers the thumbnails into the dock
window, at the rectangles the page reports.

How the strip behaves, what its cards show, how it handles per-monitor
DPI and what it costs are in
[Research: window previews](../research/window-thumbnails/index.md), with
the manual test plan, the work items and the Win32 APIs the design relies
on.

### Who owns the thumbnail

- The dock window that shows the strip is the destination. It never
  activates (Context). A click on a card is a click in that page. It takes
  the same path as a click on an icon, so the user's app keeps the
  foreground. The card then calls `window_action Activate` for its window.
  The dock received the click, which is one of the conditions under which
  `SetForegroundWindow` is granted (`windows/actions.rs`).
- No window is shown or hidden per hover.
- The registrations live in the dock window's subclass state. They are made
  on the window's own thread (the Tauri main thread), from a posted message,
  as the pass-through toggles are. The platform method stores the latest
  requested list and posts a wake-up message. The subclass applies the latest
  list, so a burst of reports costs one pass.
- The subclass hides and unregisters every thumbnail **before** it makes the
  window click-through, and again on `WM_NCDESTROY`. Both run on the same
  thread, so the order is guaranteed. No thumbnail ever sits on a layered
  window (A5).
- DWM calls are not documented to block. The subclass already calls
  `SetWindowPos` on this thread, and `prepare` calls
  `DwmSetWindowAttribute` there. Each pass is timed, and a pass slower than
  16 ms is logged at debug (A6). If the probe finds slow calls, the calls
  move to a platform thread, and the hide is posted before the pass-through
  instead.
- At most one registration exists per source window, process-wide (A11).

### Where the strip goes

- Above the hovered icon, centred on the icon's **rest** position (the
  magnified position moves with the pointer), and kept inside the window
  minus the side slack. Its bottom edge sits 8 CSS px above a fully
  magnified icon's top, where the tooltip would be. That item's tooltip
  does not show while its strip is open.
- While previews are on, the dock window keeps a band free above the
  current headroom. It is about 110 CSS px at the default card size. A card
  is about 150 CSS px tall (8 padding, a 20 px title row, a 6 px gap, a
  108 px image, 8 padding), and 48 px of headroom already exists for the
  tooltip. The UI includes the band in the content size it reports.
  Opening a strip therefore changes no window size and needs no
  `SetWindowPos`.
- The band is transparent. With click-through on, it takes no clicks: the
  hit-test thread keeps the window click-through outside the panel and the
  envelope.
- With `behavior.clickThrough` off, the whole window takes clicks, so a
  reserved band would swallow clicks across the screen. Previews are
  therefore off while click-through is off. Click-through off is a
  diagnostic switch (`settings.rs`). The settings row says so.

### Who decides what (ADR-0006, ADR-0007)

- **`stem-core`, pure, tested on Linux:** each window's preview kind, from
  the `RawWindow` facts. `Live` when not minimised and not cloaked,
  `Minimized` when minimised, `Elsewhere` otherwise (cloaked, another
  desktop). The order of the cards is the item's window order. The new
  setting.
- **`stem-service`:** `set_window_previews(label, slots)` keeps only the
  windows the engine lists with `Live`. A window gone since the last
  snapshot is dropped and logged at debug. It hands the rest to the
  platform. Tested with `MockPlatform`, which records the calls.
- **`src-tauri`, thin:** one command for dock windows only. It checks the
  rectangles (finite, positive size, inside the viewport, at most 32), turns
  CSS px into physical px with `scale_rect`, and calls the service.
- **`stem-platform`:** a trait method,
  `set_window_thumbnails(handle, &[ThumbnailSlot { window, rect }])`. Its
  default returns `Unsupported("window thumbnails")`, and the mock
  records the calls.
  The Windows code lives in `windows/thumbnails.rs` and the dock subclass,
  with a pure `rules::thumbnail` module (fit, crop, and the diff between
  the registered and the requested sets).
- **UI:** the hover machine, the strip, the band, the envelope and the
  reports. It renders the snapshot and reports intent. It decides nothing
  about which windows exist or what a click does beyond the two existing
  window commands.

### Contract changes (the next IPC minor)

- `WindowInfo.preview: 'live' | 'minimized' | 'elsewhere'`.
- `set_window_previews { previews: { windowId: WindowId, rect: Rect }[] }`:
  CSS px, relative to the calling window's viewport. `[]` means none. Dock
  windows only. It resolves at once; DWM work happens later on the window's
  thread.
- `Settings.behavior.windowPreviews: boolean`, default `true`. It has no
  effect while `clickThrough` is off.
- `MockBackend` implements and records the command. The mock scenario gets a
  minimised and a cloaked window, so the screenshots show all three kinds
  of card.

## Consequences

- While previews are on, the dock window is taller by the band. Nothing on
  screen changes until a strip opens.
- Previews depend on click-through.
- CSS cannot clip, animate or draw over the DWM image. It has square
  corners, it appears after the frame's fade, and the close button sits in
  the title row, not on the image.
- A window that protects its content with a display affinity shows an empty
  or black image (A9). That is the app's choice, as in the stock taskbar.
- A strip shows other apps' window contents, as the stock taskbar does.
  Nothing is captured, stored or logged.
- Touch and pen get no strip (there is no hover). Click and the menu still
  work.
- No new thread, no new dependency and no new `windows` feature
  (`Win32_Graphics_Dwm` is already enabled). Building it changes
  [`docs/architecture/dock-window.md`](../architecture/dock-window.md) and
  [`docs/ipc/`](../ipc/index.md).
- If DWM turns out to draw below the web content (A1), the page leaves the
  image area transparent. That is one CSS rule; the empty bars beside or
  above an image then show what is behind the dock.

## Assumptions

None of these is confirmed by Microsoft's documentation. The thumbnail
probe settles A3, A4, A6 to A9 and A11. The
[manual test plan](../research/window-thumbnails/manual-test-plan.md)
checks the rest.

The probe runs with
`cargo run -p stem-platform --example thumbnail-probe --features probes`.
[`docs/research/thumbnail-probe/`](../research/thumbnail-probe/index.md)
says how to run it and what each result means for A1 to A11. It also gives
partial evidence for A1 and A2 (on a GDI window with a caption) and tests
A5.

- **A1.** DWM draws a thumbnail above the WebView2 content of the dock
  window. If it draws below, the image area must stay transparent.
- **A2.** `rcDestination` is in the destination's client coordinates, and
  these equal window coordinates for the dock window.
- **A3.** A minimised source gives no usable image. One third-party note
  ([versailles #59](https://github.com/crasyK/versailles/issues/59), not
  official) says DWM keeps the last frame.
- **A4.** A cloaked source (another desktop, a suspended Store app) gives no
  usable image. The `DWMWA_CLOAK` documentation says a cloaked window is
  still composed, so this may be wrong, and cloaked windows may get live
  pictures.
- **A5.** Thumbnails on a `WS_EX_LAYERED` destination are unreliable (the
  same third-party note). Stem never leaves one on a layered window.
- **A6.** DWM thumbnail calls return quickly and have no thread affinity
  beyond the process. The documentation only says that the registering
  process must unregister.
- **A7.** The source size includes the invisible resize borders. Cropping
  with `rcSource` to the extended frame bounds removes them.
- **A8.** An elevated source (Task Manager) can be shown by a
  medium-integrity process. The documentation restricts only the
  destination.
- **A9.** A window with `WDA_MONITOR` or `WDA_EXCLUDEFROMCAPTURE` shows no
  content in a thumbnail. The documentation says such a window has no
  content, or does not appear, "everywhere else" than on a monitor
  ([`SetWindowDisplayAffinity`][wda]).
- **A10.** A registration costs DWM little. GPU time grows with the number
  of visible, changing sources.
- **A11.** A second registration of the same source may stop the first one.
  This was reported once, on Windows 10 1903, without a Microsoft answer
  ([Q&A thread][qa1903]). Stem keeps one registration per source.

[ovw]: https://learn.microsoft.com/en-us/windows/win32/dwm/thumbnail-ovw
[reg]: https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/nf-dwmapi-dwmregisterthumbnail
[upd]: https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/nf-dwmapi-dwmupdatethumbnailproperties
[qsz]: https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/nf-dwmapi-dwmquerythumbnailsourcesize
[unreg]: https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/nf-dwmapi-dwmunregisterthumbnail
[props]: https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ns-dwmapi-dwm_thumbnail_properties
[tnp]: https://learn.microsoft.com/en-us/windows/win32/dwm/dwm-tnp-constants
[wda]: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowdisplayaffinity
[qa1903]: https://learn.microsoft.com/en-us/answers/questions/3229922/dwmregisterthumbnail-is-not-working-as-expected-in
