# What the thumbnail probe does

This page describes the windows the [thumbnail probe](index.md) opens and
reads, the ten slots of its grid, and the phases it steps through.

## What it opens and touches

- **One destination window**, *Stem thumbnail probe*, at the top left of
  the primary monitor's work area. It is a plain, normal window: not
  topmost, not a tool window, not resizable (the grid does not reflow). It
  has a header line (the phase), a legend, and ten slots in a 5 × 2 grid.
  Each slot has a title line, a status line and a 16:10 image area. GDI
  paints the image area grey with "no picture here" and frames it in blue.
  Where a thumbnail is placed, a 2-pixel yellow outline is drawn just
  outside the rectangle DWM was asked to draw into (`rcDestination`).
- **Six source windows of its own**, cascaded at the bottom right, titled
  *Stem probe source N - key*. Each is a normal, resizable window filled
  with its own colour inside a white border (4 px at 100 % scale). It shows
  its number, its key and a clock (`t = 12.3 s`) that repaints every
  250 ms, so a live picture can be told from a frozen one. One is minimised
  to the taskbar after 0.8 s. One is cloaked and so invisible. The two with
  a display affinity look normal on the monitor. Their close buttons do
  nothing; they close with the probe. Do not resize them during the run
  (the crop would be stale).
- **Other apps' windows are only read.** The probe finds Task Manager, the
  Store app and the window on the other desktop through the platform's own
  enumeration (`windows::enumerate`, the same facts Stem uses). It reads
  their class, title, process, cloak state, display affinity and
  rectangles, and opens their process with
  `PROCESS_QUERY_LIMITED_INFORMATION` to read the token's elevation. It
  never minimises, moves, closes, activates or sends a message to them. A
  thumbnail registration does not change its source.
- If the probe dies, nothing remains. Windows destroys a process's windows
  when it ends, and each thumbnail ends with its destination window.

## The slots

| # | Key | Source | What the probe does to it | Tests |
|---|-----|--------|---------------------------|-------|
| 1 | normal | own window | nothing; `rcSource` cropped to the frame | A1, A2, A7 |
| 2 | no crop | own window | nothing; the whole source, no `rcSource`; registered again from a worker thread in phase 2 | A7, A6 |
| 3 | minimised | own window | shown for 0.8 s, then `SW_SHOWMINNOACTIVE` | A3 |
| 4 | cloaked | own window | `DwmSetWindowAttribute(DWMWA_CLOAK, TRUE)` before it is shown | A4 |
| 5 | exclude | own window | `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)` | A9 |
| 6 | monitor | own window | `SetWindowDisplayAffinity(WDA_MONITOR)` | A9 |
| 7 | elevated | Task Manager (class `TaskManagerWindow`) | read only | A8 |
| 8 | store app | Calculator (AUMID `Microsoft.WindowsCalculator…`), else another minimised or cloaked `ApplicationFrameWindow` | read only | A4 |
| 9 | other desktop | a window the shell places on another desktop (`IsWindowOnCurrentVirtualDesktop` false), Notepad first | read only | A4 |
| 10 | second | slot 1's source again | registered in phase 4 only | A11 |

For each registration the probe calls `DwmRegisterThumbnail` (destination =
the probe window) and `DwmQueryThumbnailSourceSize`, and reads
`GetWindowRect` and `DWMWA_EXTENDED_FRAME_BOUNDS`. It computes `rcSource`:
the frame relative to the window rectangle's origin. It uses `rcSource`
only when the frame lies inside the source size DWM reports and the window
is not minimised. It then fits the picture into the slot (aspect ratio
kept, centred, never above 1:1), and calls `DwmUpdateThumbnailProperties`
with `rcDestination`, `rcSource` (when cropping), `opacity` 255,
`fVisible` = TRUE and `fSourceClientAreaOnly` = FALSE. Every call is timed
with `QueryPerformanceCounter`, and its `HRESULT` is kept. The probe is
per-monitor-v2 DPI aware like Stem, so every rectangle is in physical
pixels.

## The phases

| Phase | What happens | Look at |
|-------|--------------|---------|
| 1 | slots 1 to 9 registered on the window's own thread | every slot: picture, black, or grey; ticking or frozen clock; picture inside its yellow outline; slots 5 and 6 on the monitor and in a screenshot |
| 2 | a worker thread updates slot 1, unregisters slot 2 and registers it again | slots 1 and 2 still show their pictures |
| 3.1 | `WS_EX_LAYERED` set on the probe window, `SetLayeredWindowAttributes(alpha 255, LWA_ALPHA)` | do the pictures stay, flicker, freeze or vanish |
| 3.2 | every thumbnail updated again while layered | any change from 3.1 |
| 3.3 | `WS_EX_LAYERED` removed, `RedrawWindow` | do the pictures come back on their own |
| 3.4 | every thumbnail unregistered and registered afresh (what Stem does after click-through) | every picture back as in phase 1 |
| 4.1 | slot 1's source registered a second time into slot 10 | do slots 1 and 10 both show a live picture |
| 4.2 | the second registration unregistered | does slot 1 still show its live picture |
| end | the summary is printed; Enter cleans up and exits | the probe's windows close |
