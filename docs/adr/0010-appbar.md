# ADR-0010: Reserve screen space with one AppBar per dock window, driven from its own thread

Date: 2026-11-15 · Status: proposed

## Context

This ADR decides how `system.reserveScreenSpace` ("Maximised windows stop
short of the dock") uses the AppBar messages. Windows reserves screen space
for a window through `SHAppBarMessage`: the window registers as an *AppBar*
(`ABM_NEW`), proposes a band on a screen edge (`ABM_QUERYPOS`, which the
shell moves past the taskbar and the bars already there), takes it
(`ABM_SETPOS`), and the shell takes the band out of the monitor's work
area, so maximised windows and the work area (`MonitorInfo.workArea`) stop
short of it. The shell then sends the bar a callback message through its
window (`ABN_POSCHANGED` when something on that edge changed,
`ABN_STATECHANGE`, `ABN_FULLSCREENAPP`, `ABN_WINDOWARRANGE`). Microsoft's
AppBar documentation says that the bar must report `WM_ACTIVATE`
(`ABM_ACTIVATE`) and `WM_WINDOWPOSCHANGED` (`ABM_WINDOWPOSCHANGED`), and
must give the band back (`ABM_REMOVE`).

The dock is not shaped like a classic AppBar. Each dock window sits on one
edge of its monitor's work area, spans it, and is as thick as the panel
**plus the magnification headroom** (for example 135 px for a 72 px band
at 100 % scale): the part away from the edge is transparent and
click-through. Reserving the whole window would push maximised windows
twice as far as the dock needs. And auto-hide lets the dock hide itself,
which a reservation would turn into an empty band.

What constrains the threading: `SHAppBarMessage` is a `SendMessage` to the
tray window with no timeout, and while Explorer handles `ABM_SETPOS` and
`ABM_REMOVE` it may *send* messages back to the bars' windows (their
`ABN_POSCHANGED`) and resizes maximised windows. The dock windows belong to
the Tauri main thread, and blocking Win32 calls must not run on that thread
or in a command handler (the Windows engineering rules in `AGENTS.md`).

## Decision

- **When.** A band is reserved only while `system.reserveScreenSpace` is on
  **and** `autoHide.mode` is `off` (`appbar::reservation_wanted`). An
  auto-hiding dock reserves nothing, so a hidden dock never leaves an empty
  band; a dock window hidden by the tray or the toggle shortcut gives its
  band back too and takes it again when shown. Changing either setting
  registers or removes the bands without a restart. The settings page
  greys the switch out while auto-hide is on (Rust applies the rule
  itself).
- **One AppBar per dock window**, on that window's edge
  (`settings.position`) of its own monitor, so *All monitors* reserves a
  band on every monitor with a dock.
- **The band is the panel, not the window.** Its thickness is the panel's
  resting thickness plus its margin to the edge, in physical px: the
  distance from the resting panel's far side to the window's edge side,
  taken in CSS px from the window's latest `set_dock_panel_rect` and its
  viewport at that moment (`rules::appbar::reserved_thickness`), scaled and
  rounded up when the request is made. Taken at the report, not at each
  placement, so a window that grows for a drag (whose page reports the
  panel's new place a frame later) never requests a thicker band. The
  window keeps its own thickness and reaches past the band into the work
  area, transparent and click-through there as it is without a band.
- **Query on the bounds, place on the grant.** `ABM_QUERYPOS` gets a band on
  the edge of the monitor's **bounds** (`rules::appbar::query_rect`), never
  on the work area's edge: the work area shrinks once the bar is
  registered, so a band proposed there would move inwards with every result
  (a feedback loop). The shell's adjustment, cut back to the thickness
  (`rules::appbar::set_rect`, as Microsoft's sample does), goes to
  `ABM_SETPOS`; the band the shell returns is checked
  (`rules::appbar::is_usable`: not empty, on that monitor) and reported. The
  app layer then places the window with its edge side on the edge side of
  the granted band (`rules::appbar::window_rect`) instead of the work
  area's edge, as long as the band. With the stock taskbar visible on that
  edge the band's edge side is the taskbar's inner edge, which is exactly
  where the window sits without a band, so registering moves nothing. A
  grant is used only while it matches the request in effect (same edge,
  same monitor bounds). All of these rules are pure and unit-tested on all
  four edges.
- **Steps** (`rules::appbar::steps`): a first request is `ABM_NEW` then
  query and set; another thickness on the same edge is a new query and
  set; another edge or another monitor is `ABM_REMOVE` first, then a new
  registration, so the shell gives the old band back before the new one is
  taken. On a position change the app layer already gives the band back
  when it withdraws the page's panel rect, and requests a band again only
  when the page reports a panel laid out for the new edge.
- **ABM_REMOVE** on quit, when the setting goes off, when auto-hide turns
  on, when a dock window is hidden or closes (a monitor unplugged, the mode
  back to the primary), on a position change before registering on the new
  edge, and on `WM_NCDESTROY` in case none of those ran.
- **Notifications.** `ABN_POSCHANGED` and `ABN_STATECHANGE` query again and
  set only when the result differs from the band last requested, which ends
  the ordinary exchange between bars at once; a bound
  (`rules::appbar::RequeryGuard`: 8 repositions in 2 s, then 10 s of
  ignoring) stops two bars that would keep moving each other. The pause
  ends by itself: when a notification during the pause found the band
  moved, the bar queries once more when the pause ends and follows the band
  if it is still elsewhere, so it never stays on a stale band because the
  shell sent no further notification after the pause.
  `ABN_FULLSCREENAPP` is a debug line only (fullscreen has its own detection);
  `ABN_WINDOWARRANGE` a trace line. `WM_ACTIVATE` and `WM_WINDOWPOSCHANGED`
  become `ABM_ACTIVATE` and `ABM_WINDOWPOSCHANGED` (one per window per
  batch). `TaskbarCreated` (an Explorer restart, which drops every
  registration) registers each bar again.
- **Settle.** A request waits until no newer one for the same window has
  come for 250 ms (`rules::appbar::Settle`): each `ABM_SETPOS` resizes every
  maximised window on the monitor, and the reports come in bursts (an
  icon-size slider, a DPI change, a panel rect still laid out for the old
  edge for a frame). Releases do not wait.
- **Reports.** The band the shell granted goes to the app layer after
  every request the platform applied, even one that changed nothing: a
  window that requests another edge and comes back within the settle has
  dropped the band it had in the app layer, and needs the report to get it
  back. Notifications report only a band that differs from the last report.
- **Retry.** A band that could not be taken (no tray window, `ABM_NEW`
  failing twice, or an `ABM_SETPOS` result that is not a band on the
  request's monitor, which is given back at once) is reported as none and
  requested again after 1 s, 5 s, 30 s and then every minute
  (`rules::appbar::Retry`), for as long as the window still requests it;
  a band taken, another request, a release or `TaskbarCreated` starts the
  delays over. The app layer does not request it again itself: without the
  retry, a display change during the request could leave the dock without a
  band for the rest of the session. Repeated failures cost at most one
  `ABM_SETPOS` and one `ABM_REMOVE` a minute; the first failure in a row is
  a warning in the log, the later ones debug lines.

### Threads

- The platform runs a thread, `stem-appbar`, that sends **every**
  `SHAppBarMessage` for the bars, in order. `Platform::set_dock_app_bar`
  only queues a request and returns, so the app layer calls it on the main
  thread, where the window getters it needs are direct calls; no
  `SHAppBarMessage` ever runs on the main thread or in a command handler.
  Hiding the taskbar uses one short-lived helper thread per call, awaited
  for at most 500 ms from Tauri's blocking pool. The AppBar's work is
  different: it is an exchange of messages, not one call. The shell's
  notifications arrive at any time and must be handled in order with the
  requests, and one long-lived queue keeps that order.
- The shell's notifications reach the dock window's subclass (`dock_window`)
  on the main thread, which only queues them for `stem-appbar` and
  returns. The band comes back through a sink (`AppBarSink`) on
  `stem-appbar`, which stores it and posts the placement to the main thread.
- **The deadlock this avoids.** `ABM_SETPOS` and `ABM_REMOVE` can make
  Explorer *send* messages to Stem's bar windows while it handles them. If
  the main thread, which created those windows, waited on a helper that
  waits on Explorer, Explorer would wait on the main thread forever (or
  until a timeout). So nothing on the main thread ever waits on `stem-appbar`,
  except at the quit: `Platform::release_app_bars` waits (at most 1 s)
  while dispatching the messages other threads send to the calling thread
  (`PeekMessageW(PM_NOREMOVE | PM_QS_SENDMESSAGE)` every 5 ms), so the
  shell's messages to the dock windows are handled during the wait. The
  quit gives the bands back before the taskbar's restore, so the shell has
  no bar of Stem's left to notify when the taskbar's auto-hide changes
  back.
- A hung Explorer holds only `stem-appbar`; requests queue behind it and the
  settle merges them when Explorer returns. The quit then gives up after 1 s
  with a warning; the bands are released if Explorer handles the messages
  before the process ends.
- `SHAppBarMessage` takes the bar's window as a value in its payload, and
  the shell addresses its notifications to that window, whichever thread
  sent the message; nothing in Microsoft's documentation ties the call to
  the window's own thread. Assumes that registrations made from
  `stem-appbar` behave like any other.
- Every decision `stem-appbar` makes (which message to send for which
  window, when to wait, retry or requery, what to report) is in
  `rules::appbar::Bars`, a state machine over an `AppBarShell` trait (one
  method per message, plus whether the tray exists). The thread implements
  the trait with `SHAppBarMessage` and only holds the queue and the clock;
  the tests script the trait (a taskbar that moves the band, failed calls,
  bad results, a missing shell), so the bookkeeping is proven on every host
  and only the messages themselves are left to test on Windows.

### An elevated Stem (UIPI)

Explorer runs at medium integrity. When Stem runs elevated (*Run as
administrator*), User Interface Privilege Isolation drops the messages a
lower-integrity process sends or posts to Stem's windows from `WM_USER`
up, registered messages included, unless the window lets them through.
Both the AppBar callback message (`WM_APP + 0x17`) and `TaskbarCreated` are
in that range: an elevated dock would keep its band but never receive
`ABN_POSCHANGED` (the band would not follow the taskbar's auto-hide or other
bars) nor re-register after an Explorer restart. The `SHAppBarMessage`
calls themselves go from the higher level to the lower one and are not
filtered. So `dock_window::prepare` lets exactly those two messages through
on each dock window (`ChangeWindowMessageFilterEx(MSGFLT_ALLOW)`,
`appbar::allow_shell_messages`), at any integrity level: at medium
integrity it only lets a low-integrity process send them as well, which
costs at most a bounded requery. A refusal is a debug line; the dock then
behaves as described above while elevated.

### After a crash

Microsoft's documentation does not say what Explorer does with a bar whose
window is destroyed without `ABM_REMOVE` (Stem killed from Task Manager, a
panic with `panic = "abort"`). Two outcomes are possible: Explorer detects
the dead window (when it next recomputes the bars, or at once) and frees
the band, or the band stays until Explorer restarts (sign-out, an Explorer
restart, a display change that rebuilds the bars). Stem keeps **no crash
marker** for its bands:

- Unlike the taskbar's state, which Windows persists across restarts and
  sign-ins, a band lasts at most until Explorer restarts, so the worst case
  is an empty band of one dock's thickness until the next sign-out or
  Explorer restart, not a setting the user has lost.
- A marker could only replay `ABM_REMOVE` for the dead window's handle
  value at the next start (the message takes nothing else). That is
  possible, since Explorer looks bars up by handle value, but it is a guess
  about Explorer's bookkeeping and a handle value Windows may by then have
  given to another window.
- A manual test on Windows (killing Stem) shows which outcome applies. If
  the band stays, the remedy is a marker: the window handles of the
  registered bars, written when a band is granted and deleted when it is
  given back, and at the next start `ABM_REMOVE` for each handle that is no
  longer a window of a live process. The next start's own registration is
  then placed at the screen edge instead of beside a dead band.

## Consequences

- Maximised windows stop at the dock's panel on every edge, beside a
  visible taskbar, on every monitor with a dock. The work area the platform
  reports shrinks by the band, and the page reads it as it reads a work
  area shrunk by a taskbar. A maximised window's frame, which the engine's
  overlap test reads, ends at the band.
- Known: the page's fullscreen exception (`docs/ipc/hit-region.md`)
  gives a dock no reveal strip while a fullscreen window is up when
  the monitor's work area stops short of its bounds on the dock's edge.
  With a band reserved that is always so, and since auto-hide is off
  whenever a band is reserved, fullscreen is the only thing that hides such
  a dock: while a fullscreen window is up the dock has no strip and comes
  back with the tray, the toggle shortcut or an app asking for attention,
  exactly as a dock above a visible taskbar does. Accepted.
- Known: a band given back keeps the window on its edge side until the work
  area the platform reports has grown over it
  (`rules::appbar::work_area_excludes`), so a release does not move the
  window inwards by the band's thickness and back; the reverse race (a
  layout that reads the shrunk work area before the grant arrives) can move
  the window inwards for a moment until the grant places it again.
- The magnification headroom is not reserved: a magnified icon still grows
  over a maximised window's bottom rows, as it grows over the taskbar.
- Every `ABM_SETPOS` and `ABM_REMOVE` resizes the maximised windows on that
  monitor; the settle, the release-only-on-change bookkeeping in the app
  layer and the requery guard keep that to one per real change, and a
  shell that keeps returning an unusable band costs at most one of each a
  minute (the retry).
- The mock platform records registrations (`MockCall::SetDockAppBar`,
  `MockPlatform::app_bars`) and grants the band on its scripted work area's
  edge at once, after every request as the real platform reports, so demo
  runs exercise the whole app-layer flow without touching the desktop.
- Win32 behaviour that cannot be proven off Windows gets a manual test on
  Windows: the taskbar visible and hidden, each edge, a maximised window,
  a position change, auto-hide toggled, two monitors, an Explorer restart,
  quitting and killing Stem.
