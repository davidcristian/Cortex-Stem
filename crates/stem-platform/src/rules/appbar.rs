//! Where a dock window's AppBar asks for screen space, and where the window
//! goes once the shell answered.
//!
//! With `system.reserveScreenSpace` on (and auto-hide off, the app layer's
//! rule) every visible dock window registers with `SHAppBarMessage` as an
//! AppBar, so the shell takes a band at the window's screen edge out of its
//! monitor's work area and maximised windows stop short of the dock. The
//! `windows::appbar` thread sends the messages; everything it decides is
//! here, pure, so it is unit-tested on every host. Rectangles are physical
//! pixels of the virtual desktop, half-open like [`Rect`].
//!
//! The geometry, in the order of Microsoft's AppBar sample:
//! - [`query_rect`]: what is proposed with `ABM_QUERYPOS`: a band on the
//!   edge of the monitor's **bounds**, as long as the bounds along it and
//!   [`band_thickness`] across. Never on the work area's edge: the work
//!   area shrinks once the band is reserved, so a band proposed there would
//!   move inwards with every answer (a feedback loop).
//! - [`set_rect`]: what goes to `ABM_SETPOS`: the proposal as the shell
//!   adjusted it (moved inwards past the stock taskbar and other bars on
//!   that edge, trimmed at its ends by bars on the neighbouring edges), with
//!   the thickness put back on the side away from the edge, as the sample
//!   does between the two messages.
//! - [`window_rect`]: the dock window on the band the shell granted: its
//!   edge side on the band's edge side, as long as the band, and as thick
//!   as the window is (the panel plus the magnification headroom), so it
//!   reaches past the band into the work area, transparent and
//!   click-through there as before.
//! - [`reserved_thickness`]: how thick the band is: from the resting
//!   panel's far side to the window's edge side, the panel's thickness plus
//!   its margin to the edge, never the magnification headroom; unknown for a
//!   panel outside the window across the edge (a report measured before a
//!   resize of the window but read after it).
//! - [`work_area_excludes`]: whether a work area still leaves a band out,
//!   so a band just given back is still placed on until the work area the
//!   platform reports has grown over it again.
//!
//! The bookkeeping: [`steps`] (what a new request takes: remove, register,
//! position), [`Settle`] (a request waits until its window's reports have
//! settled, so a burst costs one `ABM_SETPOS`), [`RequeryGuard`] (a bound
//! on the repositions the shell's notifications cause, which owes a query
//! when its pause ends), [`Retry`] (the wait before a band that could not
//! be taken is asked for again), and [`Bars`], the state machine the
//! `windows::appbar` thread drives: every decision about what to send for
//! which window and what to report, over an [`AppBarShell`] that the thread
//! implements with `SHAppBarMessage` and the tests here script.

use std::collections::HashMap;
use std::sync::Arc;

use stem_core::Rect;
use stem_core::settings::DockPosition;

use crate::{AppBarGrant, AppBarRequest, AppBarSink, NativeWindowHandle};

/// Whether a dock on `edge` is vertical (`left`, `right`).
fn is_vertical(edge: DockPosition) -> bool {
    matches!(edge, DockPosition::Left | DockPosition::Right)
}

/// The size of `rect` across `edge`: its height on `bottom` and `top`, its
/// width on `left` and `right`.
pub fn across(rect: Rect, edge: DockPosition) -> i32 {
    if is_vertical(edge) {
        rect.width
    } else {
        rect.height
    }
}

/// The thickness a band for `request` gets: the request's, at least 1 px
/// and at most half the monitor across the edge, so no request (a panel
/// rect read for the wrong edge, say) can take the whole screen.
pub fn band_thickness(request: &AppBarRequest) -> i32 {
    let max = (across(request.monitor, request.edge) / 2).max(1);
    request.thickness.clamp(1, max)
}

/// `rect` cut to `thickness` across `edge`, keeping its side on `edge`
/// where it is (its bottom on `bottom`, its top on `top`, its left side on
/// `left`, its right side on `right`) and its length along the edge.
fn on_edge_side(rect: Rect, edge: DockPosition, thickness: i32) -> Rect {
    let thickness = thickness.max(1);
    let length_x = rect.width.max(1);
    let length_y = rect.height.max(1);
    match edge {
        DockPosition::Bottom => Rect::new(rect.x, rect.bottom() - thickness, length_x, thickness),
        DockPosition::Top => Rect::new(rect.x, rect.y, length_x, thickness),
        DockPosition::Left => Rect::new(rect.x, rect.y, thickness, length_y),
        DockPosition::Right => Rect::new(rect.right() - thickness, rect.y, thickness, length_y),
    }
}

/// The band to propose with `ABM_QUERYPOS` for `request`: on its edge of
/// the monitor's bounds, as long as the bounds along it and
/// [`band_thickness`] across.
pub fn query_rect(request: &AppBarRequest) -> Rect {
    on_edge_side(request.monitor, request.edge, band_thickness(request))
}

/// The band to pass to `ABM_SETPOS` once the shell adjusted the proposal to
/// `adjusted` (`ABM_QUERYPOS` moves its edge side inwards past the bars
/// already on that edge and may trim its ends): the edge side where the
/// shell put it and [`band_thickness`] across again.
pub fn set_rect(adjusted: Rect, request: &AppBarRequest) -> Rect {
    on_edge_side(adjusted, request.edge, band_thickness(request))
}

/// Whether `granted`, the shell's answer to `ABM_SETPOS`, is a band a dock
/// window can be placed on: not empty, and inside the requested monitor.
/// Anything else (a shell that did not answer, a rect on another monitor)
/// is not used.
pub fn is_usable(granted: Rect, request: &AppBarRequest) -> bool {
    let monitor = request.monitor;
    granted.width > 0
        && granted.height > 0
        && granted.x >= monitor.x
        && granted.y >= monitor.y
        && granted.right() <= monitor.right()
        && granted.bottom() <= monitor.bottom()
}

/// The dock window's rect on the band `granted` on `edge`: its edge side on
/// the band's edge side, as long as the band along the edge, and
/// `window_thickness` across (the window's own thickness, which reaches
/// past the band into the work area).
pub fn window_rect(granted: Rect, edge: DockPosition, window_thickness: i32) -> Rect {
    on_edge_side(granted, edge, window_thickness)
}

/// Whether `work_area` still leaves out the band `band` on `edge`: its side
/// on that edge stops at the band's far side or further in. True from the
/// moment the shell reserved the band until the moment it gave it back and
/// the work area grew over it again, so a band that was just released can
/// still be placed on while the work area (as last reported) has not caught
/// up, and the window does not move inwards by the band's thickness and
/// back.
pub fn work_area_excludes(work_area: Rect, band: Rect, edge: DockPosition) -> bool {
    match edge {
        DockPosition::Bottom => work_area.bottom() <= band.y,
        DockPosition::Top => work_area.y >= band.bottom(),
        DockPosition::Left => work_area.x >= band.right(),
        DockPosition::Right => work_area.right() <= band.x,
    }
}

/// How far (CSS px) a panel may reach past its window's sides across the
/// edge and still count as inside it ([`reserved_thickness`]): the page
/// rounds the panel outwards to whole CSS px, and the app layer rounds the
/// window's viewport to the nearest CSS px.
pub const PANEL_SLACK_CSS: i32 = 1;

/// The thickness to reserve for a window of `window` size (width, height)
/// whose resting panel is `panel` (in the same units, relative to the
/// window): from the panel's far side to the window's side on `edge`, which
/// is the panel's thickness plus its margin to the edge. Clamped to the
/// window; `Some(0)` for an empty panel.
///
/// `None` when the panel does not lie inside the window across `edge`
/// (within [`PANEL_SLACK_CSS`]): a rect the page measured in a viewport of
/// another size, which the app layer reads against the window's size when
/// it handles the report, after a resize the page had not seen yet (the
/// window shrinking after a drop or a smaller icon size). Its
/// distance to the edge side says nothing about the band: on `bottom` and
/// `right` it would be 0 or less, which gives the band back at once, only
/// for the page's next report to take it again. The caller keeps the
/// thickness it had. Pure.
pub fn reserved_thickness(window: (i32, i32), panel: Rect, edge: DockPosition) -> Option<i32> {
    if panel.width <= 0 || panel.height <= 0 {
        return Some(0);
    }
    let (width, height) = window;
    let (start, end, limit) = if is_vertical(edge) {
        (panel.x, panel.right(), width)
    } else {
        (panel.y, panel.bottom(), height)
    };
    if start < -PANEL_SLACK_CSS || end > limit.saturating_add(PANEL_SLACK_CSS) {
        return None;
    }
    let distance = match edge {
        DockPosition::Bottom | DockPosition::Right => limit.saturating_sub(start),
        DockPosition::Top | DockPosition::Left => end,
    };
    Some(distance.clamp(0, limit.max(0)))
}

/// One step of bringing a registration in line with a request ([`steps`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// `ABM_REMOVE`: the shell gives the band back.
    Remove,
    /// `ABM_NEW`: register the window as an AppBar.
    New,
    /// `ABM_QUERYPOS` then `ABM_SETPOS` for the request.
    Position,
}

/// What it takes to go from the registration the shell holds for a window
/// (`registered`: the request it was registered and positioned for, `None`
/// when there is none) to `wanted` (`None`: no band):
/// - nothing when they are the same;
/// - a release: [`Step::Remove`];
/// - a first request: [`Step::New`], [`Step::Position`];
/// - another thickness on the same edge of the same monitor: a new
///   [`Step::Position`] only;
/// - another edge or another monitor: [`Step::Remove`] first, so the shell
///   gives the old band back before the new one is taken, then
///   [`Step::New`], [`Step::Position`].
pub fn steps(registered: Option<AppBarRequest>, wanted: Option<AppBarRequest>) -> &'static [Step] {
    match (registered, wanted) {
        (None, None) => &[],
        (None, Some(_)) => &[Step::New, Step::Position],
        (Some(_), None) => &[Step::Remove],
        (Some(current), Some(next)) if current == next => &[],
        (Some(current), Some(next))
            if current.edge == next.edge && current.monitor == next.monitor =>
        {
            &[Step::Position]
        }
        (Some(_), Some(_)) => &[Step::Remove, Step::New, Step::Position],
    }
}

/// Requests waiting for their window to settle. A dock window's reports
/// come in bursts (an icon-size slider, a drag that grows the window, a
/// DPI change, a panel rect still laid out for the old edge for a frame):
/// each request for a window replaces the one waiting and starts the wait
/// again, and only a request that has waited [`Settle::DELAY_MS`] without a
/// newer one is applied, so a burst costs one `ABM_SETPOS` (each of which
/// resizes every maximised window on the monitor). Releases do not wait;
/// the caller applies them at once and [`Settle::cancel`]s what waits.
///
/// Keyed by the caller's window key; time is an opaque millisecond counter
/// supplied by the caller, as for `rules::backoff`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settle<K> {
    /// `(key, request, due)`, at most one per key.
    pending: Vec<(K, AppBarRequest, u64)>,
}

impl<K> Default for Settle<K> {
    fn default() -> Self {
        Self {
            pending: Vec::new(),
        }
    }
}

impl<K: Copy + PartialEq> Settle<K> {
    /// How long a request waits for a newer one.
    pub const DELAY_MS: u64 = 250;

    /// `request` for `key` at `now_ms`: it replaces the one waiting for
    /// `key` and is due [`Settle::DELAY_MS`] later. The same request again
    /// keeps the time it is due.
    pub fn push(&mut self, key: K, request: AppBarRequest, now_ms: u64) {
        let due = now_ms.saturating_add(Self::DELAY_MS);
        match self.pending.iter_mut().find(|(k, _, _)| *k == key) {
            Some(entry) if entry.1 == request => {}
            Some(entry) => *entry = (key, request, due),
            None => self.pending.push((key, request, due)),
        }
    }

    /// Drop what waits for `key`; whether anything did.
    pub fn cancel(&mut self, key: K) -> bool {
        let before = self.pending.len();
        self.pending.retain(|(k, _, _)| *k != key);
        self.pending.len() != before
    }

    /// The requests due at `now_ms`, oldest due first; they stop waiting.
    pub fn take_due(&mut self, now_ms: u64) -> Vec<(K, AppBarRequest)> {
        let mut due: Vec<(K, AppBarRequest, u64)> = Vec::new();
        self.pending.retain(|&(key, request, at)| {
            if at <= now_ms {
                due.push((key, request, at));
                false
            } else {
                true
            }
        });
        due.sort_by_key(|&(_, _, at)| at);
        due.into_iter()
            .map(|(key, request, _)| (key, request))
            .collect()
    }

    /// When the next request is due, if any waits.
    pub fn next_due(&self) -> Option<u64> {
        self.pending.iter().map(|&(_, _, at)| at).min()
    }

    /// Whether a request waits for `key`.
    pub fn is_waiting(&self, key: K) -> bool {
        self.pending.iter().any(|(k, _, _)| *k == key)
    }
}

/// What [`RequeryGuard::allow`] says about one reposition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Requery {
    /// Go ahead.
    Go,
    /// No: too many in a row; a pause starts now (log it at warn).
    Pause,
    /// No: a pause is running.
    Paused,
}

/// A bound on the repositions (`ABM_SETPOS`) the shell's notifications
/// (`ABN_POSCHANGED`, `ABN_STATECHANGE`) cause for one bar. A notification
/// whose `ABM_QUERYPOS` answer is the band the bar already asked for is not
/// a reposition and does not count, which ends the ordinary exchange
/// between bars at once; this guard is for two bars that would keep moving
/// each other (another app's bar on the same edge that answers every
/// notification with a different band): at most [`RequeryGuard::BURST`]
/// repositions within [`RequeryGuard::WINDOW_MS`], then none for
/// [`RequeryGuard::PAUSE_MS`]. Requests of the app layer are not bounded by
/// it. Time is an opaque millisecond counter supplied by the caller.
///
/// A reposition refused while paused is not forgotten: the bar then owes
/// one query when the pause ends ([`RequeryGuard::owed_at`]), so a band the
/// shell moved during the pause is followed once it is over, even if no
/// notification comes after it. The guard is a circuit breaker that closes
/// again by itself.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RequeryGuard {
    /// When the current window of counting started.
    window_start: u64,
    /// Repositions let through in the current window.
    count: u32,
    /// While paused: the time from which repositions may go again.
    paused_until: Option<u64>,
    /// Whether a reposition was refused during the pause and the band has
    /// not been seen back where the bar asked for it since.
    owed: bool,
}

impl RequeryGuard {
    pub const BURST: u32 = 8;
    pub const WINDOW_MS: u64 = 2_000;
    pub const PAUSE_MS: u64 = 10_000;

    /// Whether a reposition at `now_ms` may go ahead; counts the ones it
    /// lets through. A refusal ([`Requery::Pause`], [`Requery::Paused`])
    /// leaves a query owed for the end of the pause.
    pub fn allow(&mut self, now_ms: u64) -> Requery {
        if let Some(until) = self.paused_until {
            if now_ms < until {
                self.owed = true;
                return Requery::Paused;
            }
            self.paused_until = None;
            self.count = 0;
            self.owed = false;
        }
        if self.count == 0 || now_ms.saturating_sub(self.window_start) >= Self::WINDOW_MS {
            self.window_start = now_ms;
            self.count = 0;
        }
        if self.count >= Self::BURST {
            self.paused_until = Some(now_ms.saturating_add(Self::PAUSE_MS));
            self.owed = true;
            return Requery::Pause;
        }
        self.count += 1;
        Requery::Go
    }

    /// When the bar owes a query: the end of the pause, if a reposition was
    /// refused during it and the band has not caught up since; `None` when
    /// nothing is owed.
    pub fn owed_at(&self) -> Option<u64> {
        if self.owed { self.paused_until } else { None }
    }

    /// Whether the query owed is due at `now_ms`; it stops being owed (the
    /// caller queries now, and [`RequeryGuard::allow`] then ends the pause).
    pub fn take_owed(&mut self, now_ms: u64) -> bool {
        match self.owed_at() {
            Some(at) if at <= now_ms => {
                self.owed = false;
                true
            }
            _ => false,
        }
    }

    /// The band is where the bar asked for it (or the bar holds none):
    /// nothing is owed. The pause, if one runs, goes on.
    pub fn caught_up(&mut self) {
        self.owed = false;
    }
}

/// The wait before a bar that wants a band but could not take one tries
/// again: the shell refused `ABM_NEW`, was not running, or answered
/// `ABM_SETPOS` with a rect that is not a band on the request's monitor
/// ([`is_usable`]; a display change racing the request, say). Without it a
/// band given up would stay given up until something else changed the
/// request. [`Retry::DELAYS_MS`] grows with each failure in a row and stays
/// at its last value; a band taken, a new request or a release starts it
/// over ([`Retry::reset`]). Time is an opaque millisecond counter supplied
/// by the caller.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Retry {
    /// Attempts that failed in a row.
    failures: u32,
    /// When the next attempt is due, if one is.
    due: Option<u64>,
}

impl Retry {
    /// The wait after the first, second, third and every later failure in
    /// a row.
    pub const DELAYS_MS: [u64; 4] = [1_000, 5_000, 30_000, 60_000];

    /// An attempt failed at `now_ms`: the next is due one delay later. The
    /// delay.
    pub fn failed(&mut self, now_ms: u64) -> u64 {
        let index = usize::try_from(self.failures)
            .unwrap_or(usize::MAX)
            .min(Self::DELAYS_MS.len() - 1);
        let delay = Self::DELAYS_MS[index];
        self.failures = self.failures.saturating_add(1);
        self.due = Some(now_ms.saturating_add(delay));
        delay
    }

    /// Attempts that failed in a row.
    pub fn failures(&self) -> u32 {
        self.failures
    }

    /// When the next attempt is due, if one is.
    pub fn due(&self) -> Option<u64> {
        self.due
    }

    /// Whether an attempt is due at `now_ms`; it stops being due.
    pub fn take_due(&mut self, now_ms: u64) -> bool {
        match self.due {
            Some(at) if at <= now_ms => {
                self.due = None;
                true
            }
            _ => false,
        }
    }

    /// Forget the failures and the attempt due.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

// ---------------------------------------------------------------------------
// The bars' bookkeeping
// ---------------------------------------------------------------------------

/// The shell as [`Bars`] sees it: one method per `SHAppBarMessage` it
/// sends, plus whether the shell runs at all. `windows::appbar` implements
/// it with the real messages on its thread; the tests here script it.
/// Handles are the dock windows' raw `HWND` values.
pub trait AppBarShell {
    /// Whether the shell runs (its tray window exists). Sends nothing.
    fn present(&mut self) -> bool;
    /// `ABM_NEW` for `handle`: whether the shell took the registration.
    fn register(&mut self, handle: isize) -> bool;
    /// `ABM_REMOVE` for `handle`.
    fn remove(&mut self, handle: isize);
    /// `ABM_QUERYPOS` with `rect` on `edge`: the rect the shell answers.
    fn query(&mut self, handle: isize, edge: DockPosition, rect: Rect) -> Rect;
    /// `ABM_SETPOS` with `rect` on `edge`: the rect the shell answers.
    fn set_pos(&mut self, handle: isize, edge: DockPosition, rect: Rect) -> Rect;
    /// `ABM_ACTIVATE`.
    fn activate(&mut self, handle: isize, active: bool);
    /// `ABM_WINDOWPOSCHANGED`.
    fn window_pos_changed(&mut self, handle: isize);
}

/// What the shell tells a bar through its callback message (`wParam`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notification {
    /// `ABN_POSCHANGED`: something on the bar's edge changed (the taskbar,
    /// another bar).
    PosChanged,
    /// `ABN_STATECHANGE`: the taskbar's auto-hide or always-on-top state
    /// changed.
    StateChange,
    /// `ABN_FULLSCREENAPP`: a fullscreen app opened (`true`) or closed.
    FullScreenApp(bool),
    /// `ABN_WINDOWARRANGE`: windows are being tiled or cascaded (`true`),
    /// or are done.
    WindowArrange(bool),
    /// Any other code.
    Other(u32),
}

/// Why a bar could not take its band.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Failure {
    /// No tray window: Explorer is not running or is restarting.
    NoShell,
    /// `ABM_NEW` refused, twice.
    Refused,
    /// `ABM_SETPOS` answered a rect that is not a band on the monitor.
    Unusable(Rect),
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoShell => f.write_str(
                "there is no taskbar to register with: Explorer is not running or is restarting",
            ),
            Self::Refused => f.write_str("the shell refused ABM_NEW"),
            Self::Unusable(rect) => write!(
                f,
                "the shell granted {rect:?}, which is not a band on that monitor"
            ),
        }
    }
}

/// How [`Bar::report`] treats a report equal to the last one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Report {
    /// Skip it.
    IfChanged,
    /// Send it anyway.
    Always,
}

/// A registration with the shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Registered {
    /// The request it was registered and last positioned for.
    request: AppBarRequest,
    /// The band last passed to `ABM_SETPOS`.
    asked: Option<Rect>,
    /// The band the shell granted last, while it was usable.
    granted: Option<AppBarGrant>,
}

/// One dock window's bar.
struct Bar {
    sink: AppBarSink,
    /// What the window wants now (its latest request).
    wanted: Option<AppBarRequest>,
    /// What the shell holds for the window, if it is registered.
    registered: Option<Registered>,
    /// What was last reported to the sink.
    reported: Option<Option<AppBarGrant>>,
    requery: RequeryGuard,
    retry: Retry,
}

impl Bar {
    fn new(sink: AppBarSink) -> Self {
        Self {
            sink,
            wanted: None,
            registered: None,
            reported: None,
            requery: RequeryGuard::default(),
            retry: Retry::default(),
        }
    }

    /// Report `grant` to the sink; with [`Report::IfChanged`] only when it
    /// differs from what was reported last.
    fn report(&mut self, handle: isize, grant: Option<AppBarGrant>, when: Report) {
        if when == Report::IfChanged && self.reported == Some(grant) {
            return;
        }
        self.reported = Some(grant);
        (self.sink)(NativeWindowHandle(handle), grant);
    }

    /// An attempt to take the band for `request` failed at `now_ms`: the
    /// next is scheduled ([`Retry`]) and `None` reported.
    fn failed(&mut self, handle: isize, request: AppBarRequest, failure: Failure, now_ms: u64) {
        let delay = self.retry.failed(now_ms);
        if self.retry.failures() == 1 {
            log::warn!(
                "AppBar {handle:#x}: could not take a band on the {:?} edge ({failure}); no \
                 screen space is reserved for this dock; trying again in {} s",
                request.edge,
                delay / 1000
            );
        } else {
            log::debug!(
                "AppBar {handle:#x}: still no band on the {:?} edge ({failure}, {} failures in a \
                 row); trying again in {} s",
                request.edge,
                self.retry.failures(),
                delay / 1000
            );
        }
        self.report(handle, None, Report::IfChanged);
    }
}

/// The dock windows' AppBars: what each window wants, what the shell holds
/// for it, and what was reported, kept in line by the messages it sends
/// through an [`AppBarShell`]. The `windows::appbar` thread owns one and
/// feeds it every command in order; nothing here knows of threads or
/// Win32, so all of it is unit-tested on every host. Time is an opaque
/// millisecond counter supplied by the caller; [`Bars::next_due`] says
/// when [`Bars::tick`] has work.
///
/// - [`Bars::set`]: the latest request of a window. A band waits in the
///   [`Settle`]; a release (`None`) is `ABM_REMOVE` at once and not
///   reported.
/// - Applying a request takes [`steps`]: `ABM_NEW`, then `ABM_QUERYPOS` on
///   the monitor's bounds ([`query_rect`]) and `ABM_SETPOS` with the
///   shell's adjustment cut back to the thickness ([`set_rect`]). The band
///   granted is reported after **every** request applied, even one that
///   changed nothing (the caller may have dropped the band it had while a
///   newer request waited), and only when it differs otherwise.
/// - A request that could not take its band (the shell gone, `ABM_NEW`
///   refused, a band that is not [`is_usable`], given back at once) reports
///   `None` and is tried again after a [`Retry`] delay, for as long as the
///   window wants it.
/// - `ABN_POSCHANGED` / `ABN_STATECHANGE` query again and set only when the
///   band moved, within the [`RequeryGuard`]; a band moved during its pause
///   is queried again when the pause ends.
/// - `TaskbarCreated` registers the bar again at once; a destroyed window's
///   bar is removed and forgotten; [`Bars::release_all`] gives every band
///   back (the quit) and reports nothing.
#[derive(Default)]
pub struct Bars {
    bars: HashMap<isize, Bar>,
    settle: Settle<isize>,
}

impl std::fmt::Debug for Bars {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Bars")
            .field("bars", &self.bars.len())
            .field("settle", &self.settle)
            .finish()
    }
}

impl Bars {
    /// The latest request of the window `handle` (`None`: no band), to be
    /// reported through `sink`. A band waits for the settle; a release is
    /// sent at once.
    pub fn set(
        &mut self,
        shell: &mut impl AppBarShell,
        handle: isize,
        request: Option<AppBarRequest>,
        sink: AppBarSink,
        now_ms: u64,
    ) {
        let bar = self
            .bars
            .entry(handle)
            .or_insert_with(|| Bar::new(Arc::clone(&sink)));
        bar.sink = sink;
        if bar.wanted != request {
            bar.retry.reset();
        }
        bar.wanted = request;
        match request {
            Some(request) => self.settle.push(handle, request, now_ms),
            None => {
                self.settle.cancel(handle);
                // A release the caller asked for is not reported.
                bar.reported = Some(None);
                if bar.registered.take().is_some() {
                    shell.remove(handle);
                    log::info!("AppBar {handle:#x}: band given back");
                }
            }
        }
    }

    /// A notification from the shell for the window `handle`.
    pub fn notify(
        &mut self,
        shell: &mut impl AppBarShell,
        handle: isize,
        notification: Notification,
        now_ms: u64,
    ) {
        match notification {
            Notification::PosChanged => self.requery(shell, handle, now_ms, "ABN_POSCHANGED"),
            Notification::StateChange => self.requery(shell, handle, now_ms, "ABN_STATECHANGE"),
            Notification::FullScreenApp(open) => log::debug!(
                "AppBar {handle:#x}: a fullscreen app {} on this bar's monitor",
                if open { "opened" } else { "closed" }
            ),
            Notification::WindowArrange(start) => log::trace!(
                "AppBar {handle:#x}: windows are being {} (tile or cascade)",
                if start { "arranged" } else { "done arranging" }
            ),
            Notification::Other(code) => {
                log::trace!("AppBar {handle:#x}: notification {code} ignored");
            }
        }
    }

    /// `WM_ACTIVATE` on the window: `ABM_ACTIVATE` while it is registered.
    pub fn activate(&mut self, shell: &mut impl AppBarShell, handle: isize, active: bool) {
        if self.is_registered(handle) {
            shell.activate(handle, active);
        }
    }

    /// `WM_WINDOWPOSCHANGED` on the window: `ABM_WINDOWPOSCHANGED` while it
    /// is registered.
    pub fn window_pos_changed(&mut self, shell: &mut impl AppBarShell, handle: isize) {
        if self.is_registered(handle) {
            shell.window_pos_changed(handle);
        }
    }

    /// `TaskbarCreated` reached the window: the shell started again and its
    /// registrations are gone, so the bar registers again at once for what
    /// it wants.
    pub fn shell_restarted(&mut self, shell: &mut impl AppBarShell, handle: isize, now_ms: u64) {
        let Some(bar) = self.bars.get_mut(&handle) else {
            return;
        };
        let Some(wanted) = bar.wanted else {
            return;
        };
        log::info!("AppBar {handle:#x}: the shell started again; registering the bar again");
        bar.registered = None;
        bar.retry.reset();
        bar.requery = RequeryGuard::default();
        self.settle.cancel(handle);
        self.apply(shell, handle, wanted, now_ms);
    }

    /// The window is being destroyed: its band is given back and the bar
    /// forgotten. Nothing is reported.
    pub fn destroyed(&mut self, shell: &mut impl AppBarShell, handle: isize) {
        self.settle.cancel(handle);
        if let Some(bar) = self.bars.remove(&handle)
            && bar.registered.is_some()
        {
            log::debug!("AppBar {handle:#x}: the window is gone; giving its band back");
            shell.remove(handle);
        }
    }

    /// Give every band back (the quit). Nothing is reported, and nothing is
    /// tried again.
    pub fn release_all(&mut self, shell: &mut impl AppBarShell) {
        for (&handle, bar) in &mut self.bars {
            self.settle.cancel(handle);
            bar.wanted = None;
            bar.retry.reset();
            bar.requery.caught_up();
            bar.reported = Some(None);
            if bar.registered.take().is_some() {
                shell.remove(handle);
                log::info!("AppBar {handle:#x}: band given back");
            }
        }
    }

    /// When [`Bars::tick`] has work next: a settled request, an attempt to
    /// try again, or a query owed at the end of a pause.
    pub fn next_due(&self) -> Option<u64> {
        let bars = self
            .bars
            .values()
            .flat_map(|bar| [bar.retry.due(), bar.requery.owed_at()])
            .flatten();
        self.settle.next_due().into_iter().chain(bars).min()
    }

    /// Do what is due at `now_ms`: apply the requests that have settled,
    /// try again for the bars whose attempt is due, and query the bars that
    /// owe a query.
    pub fn tick(&mut self, shell: &mut impl AppBarShell, now_ms: u64) {
        for (handle, request) in self.settle.take_due(now_ms) {
            self.apply(shell, handle, request, now_ms);
        }
        let retries: Vec<isize> = self
            .bars
            .iter_mut()
            .filter_map(|(&handle, bar)| bar.retry.take_due(now_ms).then_some(handle))
            .collect();
        for handle in retries {
            let wanted = self.bars.get(&handle).and_then(|bar| bar.wanted);
            // A newer request in the settle is the next attempt.
            if let Some(wanted) = wanted
                && !self.settle.is_waiting(handle)
            {
                log::debug!("AppBar {handle:#x}: trying again to take a band");
                self.apply(shell, handle, wanted, now_ms);
            }
        }
        let owed: Vec<isize> = self
            .bars
            .iter_mut()
            .filter_map(|(&handle, bar)| bar.requery.take_owed(now_ms).then_some(handle))
            .collect();
        for handle in owed {
            self.requery(shell, handle, now_ms, "the end of the pause");
        }
    }

    fn is_registered(&self, handle: isize) -> bool {
        self.bars
            .get(&handle)
            .is_some_and(|bar| bar.registered.is_some())
    }

    /// Bring the bar of `handle` in line with `request` ([`steps`]; a bar
    /// registered whose band was never granted is positioned again), then
    /// report the band it holds, even when it is the one reported last. A
    /// failure reports `None` and schedules another attempt.
    fn apply(
        &mut self,
        shell: &mut impl AppBarShell,
        handle: isize,
        request: AppBarRequest,
        now_ms: u64,
    ) {
        let Some(bar) = self.bars.get_mut(&handle) else {
            return;
        };
        if bar.wanted != Some(request) {
            return;
        }
        let mut todo = steps(bar.registered.map(|r| r.request), Some(request));
        if todo.is_empty() && bar.registered.is_some_and(|r| r.granted.is_none()) {
            todo = &[Step::Position];
        }
        for &step in todo {
            match step {
                Step::Remove => {
                    shell.remove(handle);
                    bar.registered = None;
                    log::info!(
                        "AppBar {handle:#x}: band given back before registering on another edge \
                         or monitor"
                    );
                }
                Step::New => {
                    if let Err(failure) = register(shell, handle) {
                        bar.failed(handle, request, failure, now_ms);
                        return;
                    }
                    bar.registered = Some(Registered {
                        request,
                        asked: None,
                        granted: None,
                    });
                }
                Step::Position => {
                    // The band granted for an earlier request is not this
                    // request's until the shell answers again.
                    if let Some(registered) = bar.registered.as_mut() {
                        registered.request = request;
                        registered.granted = None;
                    }
                    if !shell.present() {
                        bar.failed(handle, request, Failure::NoShell, now_ms);
                        return;
                    }
                    let asked = ask(shell, handle, &request);
                    if let Err(failure) = take(shell, handle, bar, request, asked) {
                        bar.failed(handle, request, failure, now_ms);
                        return;
                    }
                }
            }
        }
        bar.retry.reset();
        let granted = bar.registered.and_then(|r| r.granted);
        bar.report(handle, granted, Report::Always);
    }

    /// Query the band of the bar `handle` again (`cause`: what asked) and
    /// set it when it moved, within the bar's [`RequeryGuard`].
    fn requery(
        &mut self,
        shell: &mut impl AppBarShell,
        handle: isize,
        now_ms: u64,
        cause: &'static str,
    ) {
        let Some(bar) = self.bars.get_mut(&handle) else {
            log::trace!("AppBar {handle:#x}: {cause} for no bar");
            return;
        };
        let Some(registered) = bar.registered else {
            bar.requery.caught_up();
            return;
        };
        if !shell.present() {
            log::debug!(
                "AppBar {handle:#x}: {cause} while the shell is gone; TaskbarCreated registers \
                 the bar again"
            );
            bar.requery.caught_up();
            return;
        }
        let request = registered.request;
        let asked = ask(shell, handle, &request);
        if Some(asked) == registered.asked {
            bar.requery.caught_up();
            log::trace!("AppBar {handle:#x}: {cause}: the band is unchanged");
            return;
        }
        match bar.requery.allow(now_ms) {
            Requery::Go => {
                log::debug!(
                    "AppBar {handle:#x}: {cause}: the shell moved the band; asking for {asked:?}"
                );
                match take(shell, handle, bar, request, asked) {
                    Ok(grant) => {
                        bar.retry.reset();
                        bar.report(handle, Some(grant), Report::IfChanged);
                    }
                    Err(failure) => bar.failed(handle, request, failure, now_ms),
                }
            }
            Requery::Pause => log::warn!(
                "AppBar {handle:#x}: the shell moved the band {} times in a row; its \
                 notifications are ignored for {} s (another bar keeps moving it?), then the \
                 band is asked for again",
                RequeryGuard::BURST,
                RequeryGuard::PAUSE_MS / 1000
            ),
            Requery::Paused => {
                log::trace!("AppBar {handle:#x}: {cause} ignored (paused)");
            }
        }
    }
}

/// `ABM_NEW` for the window. A refusal means an error or a window that is
/// registered already (a registration the bookkeeping no longer knows of):
/// the stale one is removed and the registration tried once more.
fn register(shell: &mut impl AppBarShell, handle: isize) -> Result<(), Failure> {
    if !shell.present() {
        return Err(Failure::NoShell);
    }
    if shell.register(handle) {
        return Ok(());
    }
    log::debug!("AppBar {handle:#x}: ABM_NEW refused; removing a stale registration and retrying");
    shell.remove(handle);
    if shell.register(handle) {
        Ok(())
    } else {
        Err(Failure::Refused)
    }
}

/// `ABM_QUERYPOS` with the band on the request's edge of the monitor's
/// bounds; the shell's adjustment cut back to the thickness.
fn ask(shell: &mut impl AppBarShell, handle: isize, request: &AppBarRequest) -> Rect {
    let adjusted = shell.query(handle, request.edge, query_rect(request));
    set_rect(adjusted, request)
}

/// `ABM_SETPOS` with `asked`; the band the shell answered, or the
/// registration given up (`ABM_REMOVE`) when the answer is not a usable
/// band.
fn take(
    shell: &mut impl AppBarShell,
    handle: isize,
    bar: &mut Bar,
    request: AppBarRequest,
    asked: Rect,
) -> Result<AppBarGrant, Failure> {
    let granted = shell.set_pos(handle, request.edge, asked);
    if let Some(registered) = bar.registered.as_mut() {
        registered.asked = Some(asked);
    }
    if !is_usable(granted, &request) {
        log::debug!("AppBar {handle:#x}: unusable band {granted:?} for {request:?}; removing");
        shell.remove(handle);
        bar.registered = None;
        return Err(Failure::Unusable(granted));
    }
    let grant = AppBarGrant {
        request,
        rect: granted,
    };
    if let Some(registered) = bar.registered.as_mut() {
        registered.granted = Some(grant);
    }
    log::info!(
        "AppBar {handle:#x}: {} px reserved on the {:?} edge of {:?}: the shell granted \
         {granted:?}",
        across(granted, request.edge),
        request.edge,
        request.monitor
    );
    Ok(grant)
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use parking_lot::Mutex;
    use pretty_assertions::assert_eq;

    use super::*;

    /// A 1920×1080 monitor at the origin.
    const MONITOR: Rect = Rect::new(0, 0, 1920, 1080);
    /// A 2560×1440 monitor left of the primary (a negative origin).
    const LEFT_MONITOR: Rect = Rect::new(-2560, 0, 2560, 1440);

    fn request(monitor: Rect, edge: DockPosition, thickness: i32) -> AppBarRequest {
        AppBarRequest {
            monitor,
            edge,
            thickness,
        }
    }

    #[test]
    fn the_query_rect_lies_on_the_monitor_bounds_edge_on_all_four_edges() {
        assert_eq!(
            query_rect(&request(MONITOR, DockPosition::Bottom, 72)),
            Rect::new(0, 1008, 1920, 72)
        );
        assert_eq!(
            query_rect(&request(MONITOR, DockPosition::Top, 72)),
            Rect::new(0, 0, 1920, 72)
        );
        assert_eq!(
            query_rect(&request(MONITOR, DockPosition::Left, 72)),
            Rect::new(0, 0, 72, 1080)
        );
        assert_eq!(
            query_rect(&request(MONITOR, DockPosition::Right, 72)),
            Rect::new(1848, 0, 72, 1080)
        );
        // A negative origin: the right edge is the boundary with the primary.
        assert_eq!(
            query_rect(&request(LEFT_MONITOR, DockPosition::Right, 108)),
            Rect::new(-108, 0, 108, 1440)
        );
        assert_eq!(
            query_rect(&request(LEFT_MONITOR, DockPosition::Left, 108)),
            Rect::new(-2560, 0, 108, 1440)
        );
    }

    #[test]
    fn the_band_is_at_least_one_pixel_and_at_most_half_the_monitor() {
        assert_eq!(
            band_thickness(&request(MONITOR, DockPosition::Bottom, 0)),
            1
        );
        assert_eq!(
            band_thickness(&request(MONITOR, DockPosition::Bottom, -5)),
            1
        );
        assert_eq!(
            band_thickness(&request(MONITOR, DockPosition::Bottom, 900)),
            540
        );
        assert_eq!(
            band_thickness(&request(MONITOR, DockPosition::Left, 1500)),
            960
        );
        assert_eq!(
            band_thickness(&request(MONITOR, DockPosition::Left, 72)),
            72
        );
        assert_eq!(
            query_rect(&request(MONITOR, DockPosition::Top, 5000)),
            Rect::new(0, 0, 1920, 540)
        );
        // A degenerate monitor still gives a 1 px band.
        assert_eq!(
            query_rect(&request(Rect::new(0, 0, 0, 0), DockPosition::Bottom, 72)),
            Rect::new(0, -1, 1, 1)
        );
    }

    #[test]
    fn the_set_rect_keeps_the_side_the_shell_moved_and_puts_the_thickness_back() {
        // A visible 48 px taskbar at the bottom: the shell moves the
        // proposal's bottom up to the taskbar's top and leaves its top where
        // it was, which would make the band 24 px thick.
        let bottom = request(MONITOR, DockPosition::Bottom, 72);
        assert_eq!(
            set_rect(Rect::new(0, 1008, 1920, 24), &bottom),
            Rect::new(0, 960, 1920, 72)
        );
        // Another bar 30 px thick at the top.
        let top = request(MONITOR, DockPosition::Top, 72);
        assert_eq!(
            set_rect(Rect::new(0, 30, 1920, 42), &top),
            Rect::new(0, 30, 1920, 72)
        );
        // On the left, trimmed at its lower end by the bottom taskbar.
        let left = request(MONITOR, DockPosition::Left, 72);
        assert_eq!(
            set_rect(Rect::new(0, 0, 72, 1032), &left),
            Rect::new(0, 0, 72, 1032)
        );
        // On the right, past a 20 px bar on the same edge and trimmed.
        let right = request(MONITOR, DockPosition::Right, 72);
        assert_eq!(
            set_rect(Rect::new(1848, 0, 52, 1032), &right),
            Rect::new(1828, 0, 72, 1032)
        );
        // Unchanged by a shell with nothing on that edge.
        for edge in [
            DockPosition::Bottom,
            DockPosition::Top,
            DockPosition::Left,
            DockPosition::Right,
        ] {
            let r = request(MONITOR, edge, 72);
            assert_eq!(set_rect(query_rect(&r), &r), query_rect(&r), "{edge:?}");
        }
    }

    #[test]
    fn the_window_sits_with_its_edge_side_on_the_band_on_all_four_edges() {
        // The band above a 48 px taskbar; the window is 135 px thick.
        assert_eq!(
            window_rect(Rect::new(0, 960, 1920, 72), DockPosition::Bottom, 135),
            Rect::new(0, 897, 1920, 135)
        );
        assert_eq!(
            window_rect(Rect::new(0, 30, 1920, 72), DockPosition::Top, 135),
            Rect::new(0, 30, 1920, 135)
        );
        assert_eq!(
            window_rect(Rect::new(0, 0, 72, 1032), DockPosition::Left, 135),
            Rect::new(0, 0, 135, 1032)
        );
        assert_eq!(
            window_rect(Rect::new(1828, 0, 72, 1032), DockPosition::Right, 135),
            Rect::new(1765, 0, 135, 1032)
        );
        // A negative origin, on the boundary with the primary.
        assert_eq!(
            window_rect(Rect::new(-108, 0, 108, 1392), DockPosition::Right, 203),
            Rect::new(-203, 0, 203, 1392)
        );
        // Never empty.
        assert_eq!(
            window_rect(Rect::new(0, 960, 1920, 72), DockPosition::Bottom, 0),
            Rect::new(0, 1031, 1920, 1)
        );
    }

    #[test]
    fn the_reserved_thickness_runs_from_the_panels_far_side_to_the_windows_edge_side() {
        // A horizontal window 1920×135 with a 640×64 panel 8 px above its
        // bottom: 64 + 8.
        let panel = Rect::new(640, 63, 640, 64);
        assert_eq!(
            reserved_thickness((1920, 135), panel, DockPosition::Bottom),
            Some(72)
        );
        // On the top edge, 8 px below the window's top.
        let panel = Rect::new(640, 8, 640, 64);
        assert_eq!(
            reserved_thickness((1920, 135), panel, DockPosition::Top),
            Some(72)
        );
        // Vertical: a 64×640 panel 8 px from the window's left side.
        let panel = Rect::new(8, 200, 64, 640);
        assert_eq!(
            reserved_thickness((135, 1032), panel, DockPosition::Left),
            Some(72)
        );
        // And 8 px from its right side.
        let panel = Rect::new(63, 200, 64, 640);
        assert_eq!(
            reserved_thickness((135, 1032), panel, DockPosition::Right),
            Some(72)
        );
        // The window grown for a drag: the panel's place follows, the band
        // does not.
        let panel = Rect::new(640, 935, 640, 64);
        assert_eq!(
            reserved_thickness((1920, 1007), panel, DockPosition::Bottom),
            Some(72)
        );
        // A panel longer than its window along the edge still has its
        // thickness across it.
        let panel = Rect::new(-40, 63, 2000, 64);
        assert_eq!(
            reserved_thickness((1920, 135), panel, DockPosition::Bottom),
            Some(72)
        );
    }

    #[test]
    fn the_reserved_thickness_is_clamped_to_the_window_and_zero_without_a_panel() {
        assert_eq!(
            reserved_thickness((1920, 135), Rect::new(0, 0, 0, 0), DockPosition::Bottom),
            Some(0)
        );
        assert_eq!(
            reserved_thickness((1920, 135), Rect::new(10, 10, 0, 64), DockPosition::Top),
            Some(0)
        );
        // Rounded outwards past the window's sides by a px: inside, and
        // never more than the window.
        assert_eq!(
            reserved_thickness(
                (1920, 135),
                Rect::new(640, 72, 640, 64),
                DockPosition::Bottom
            ),
            Some(63)
        );
        assert_eq!(
            reserved_thickness((1920, 135), Rect::new(640, -1, 640, 137), DockPosition::Top),
            Some(135)
        );
        assert_eq!(
            reserved_thickness(
                (135, 1032),
                Rect::new(-1, 200, 137, 640),
                DockPosition::Right
            ),
            Some(135)
        );
    }

    #[test]
    fn a_panel_outside_its_window_across_the_edge_has_no_thickness() {
        // The icon size going from 96 to 48 on `bottom`: the page measures
        // the smaller panel (64 css high) in the 226 css window, 8 above its
        // bottom, but the report is read after the window shrank to 149.
        // 149 - 154 would be 0: a band given back for nothing.
        assert_eq!(
            reserved_thickness(
                (1920, 149),
                Rect::new(640, 154, 640, 64),
                DockPosition::Bottom
            ),
            None
        );
        // The same after a drop on `bottom`: measured in the 1007 css high
        // window the drag grew, read in the 135 css one.
        assert_eq!(
            reserved_thickness(
                (1920, 135),
                Rect::new(640, 935, 640, 64),
                DockPosition::Bottom
            ),
            None
        );
        // And on `right`: measured in the grown window, the column
        // lies past the side of the 150 css wide one.
        assert_eq!(
            reserved_thickness(
                (150, 928),
                Rect::new(1572, 144, 64, 640),
                DockPosition::Right
            ),
            None
        );
        // Once the page measured in the window's own viewport again.
        assert_eq!(
            reserved_thickness((150, 928), Rect::new(78, 144, 64, 640), DockPosition::Right),
            Some(72)
        );
        // A panel rect still laid out for the bottom read on the left edge.
        assert_eq!(
            reserved_thickness((135, 1032), Rect::new(640, 63, 640, 64), DockPosition::Left),
            None
        );
        // Above the window, and past its top side.
        assert_eq!(
            reserved_thickness((1920, 135), Rect::new(0, -70, 640, 64), DockPosition::Top),
            None
        );
        assert_eq!(
            reserved_thickness((1920, 135), Rect::new(0, -2, 640, 64), DockPosition::Bottom),
            None
        );
        // Every edge: a panel more than a px past the far side.
        for (window, panel, edge) in [
            ((1920, 135), Rect::new(0, 72, 640, 65), DockPosition::Bottom),
            ((1920, 135), Rect::new(0, 72, 640, 65), DockPosition::Top),
            ((135, 1032), Rect::new(72, 0, 65, 640), DockPosition::Left),
            ((135, 1032), Rect::new(72, 0, 65, 640), DockPosition::Right),
        ] {
            assert_eq!(reserved_thickness(window, panel, edge), None, "{edge:?}");
        }
    }

    #[test]
    fn a_band_is_usable_only_when_it_is_not_empty_and_inside_its_monitor() {
        let r = request(MONITOR, DockPosition::Bottom, 72);
        assert!(is_usable(Rect::new(0, 960, 1920, 72), &r));
        assert!(is_usable(MONITOR, &r));
        assert!(!is_usable(Rect::new(0, 960, 1920, 0), &r));
        assert!(!is_usable(Rect::new(0, 960, 0, 72), &r));
        assert!(
            !is_usable(Rect::new(0, 1050, 1920, 72), &r),
            "past the bottom"
        );
        assert!(
            !is_usable(Rect::new(-10, 960, 1920, 72), &r),
            "past the left"
        );
        let left = request(LEFT_MONITOR, DockPosition::Right, 108);
        assert!(is_usable(Rect::new(-108, 0, 108, 1440), &left));
        assert!(
            !is_usable(Rect::new(1812, 0, 108, 1080), &left),
            "the primary"
        );
    }

    #[test]
    fn a_work_area_excludes_a_band_until_it_grows_over_it_on_all_four_edges() {
        // Bottom: the band above a 48 px taskbar.
        let band = Rect::new(0, 960, 1920, 72);
        assert!(work_area_excludes(
            Rect::new(0, 0, 1920, 960),
            band,
            DockPosition::Bottom
        ));
        assert!(!work_area_excludes(
            Rect::new(0, 0, 1920, 1032),
            band,
            DockPosition::Bottom
        ));
        // Top.
        let band = Rect::new(0, 0, 1920, 72);
        assert!(work_area_excludes(
            Rect::new(0, 72, 1920, 960),
            band,
            DockPosition::Top
        ));
        assert!(!work_area_excludes(
            Rect::new(0, 0, 1920, 1032),
            band,
            DockPosition::Top
        ));
        // Left.
        let band = Rect::new(0, 0, 72, 1032);
        assert!(work_area_excludes(
            Rect::new(72, 0, 1848, 1032),
            band,
            DockPosition::Left
        ));
        assert!(!work_area_excludes(
            Rect::new(0, 0, 1920, 1032),
            band,
            DockPosition::Left
        ));
        // Right, on a monitor with a negative origin.
        let band = Rect::new(-108, 0, 108, 1440);
        assert!(work_area_excludes(
            Rect::new(-2560, 0, 2452, 1392),
            band,
            DockPosition::Right
        ));
        assert!(!work_area_excludes(
            Rect::new(-2560, 0, 2560, 1392),
            band,
            DockPosition::Right
        ));
    }

    #[test]
    fn steps_register_reposition_or_move_a_bar() {
        let bottom = request(MONITOR, DockPosition::Bottom, 72);
        let thicker = request(MONITOR, DockPosition::Bottom, 96);
        let left = request(MONITOR, DockPosition::Left, 72);
        let other_monitor = request(LEFT_MONITOR, DockPosition::Bottom, 72);
        assert_eq!(steps(None, None), &[] as &[Step]);
        assert_eq!(steps(None, Some(bottom)), &[Step::New, Step::Position]);
        assert_eq!(steps(Some(bottom), None), &[Step::Remove]);
        assert_eq!(steps(Some(bottom), Some(bottom)), &[] as &[Step]);
        assert_eq!(steps(Some(bottom), Some(thicker)), &[Step::Position]);
        assert_eq!(
            steps(Some(bottom), Some(left)),
            &[Step::Remove, Step::New, Step::Position]
        );
        assert_eq!(
            steps(Some(bottom), Some(other_monitor)),
            &[Step::Remove, Step::New, Step::Position]
        );
    }

    #[test]
    fn settle_applies_the_latest_request_once_it_has_waited() {
        let a = request(MONITOR, DockPosition::Bottom, 72);
        let b = request(MONITOR, DockPosition::Bottom, 80);
        let mut settle: Settle<u32> = Settle::default();
        assert_eq!(settle.next_due(), None);
        settle.push(1, a, 1_000);
        assert_eq!(settle.next_due(), Some(1_250));
        assert_eq!(settle.take_due(1_249), vec![]);
        // A newer request replaces it and waits again.
        settle.push(1, b, 1_200);
        assert_eq!(settle.next_due(), Some(1_450));
        assert_eq!(settle.take_due(1_300), vec![]);
        // The same request again keeps its time.
        settle.push(1, b, 1_400);
        assert_eq!(settle.next_due(), Some(1_450));
        assert_eq!(settle.take_due(1_450), vec![(1, b)]);
        assert_eq!(settle.next_due(), None);
        assert_eq!(settle.take_due(5_000), vec![]);
    }

    #[test]
    fn settle_keeps_one_request_per_window_and_cancels_on_a_release() {
        let a = request(MONITOR, DockPosition::Bottom, 72);
        let b = request(LEFT_MONITOR, DockPosition::Right, 108);
        let mut settle: Settle<u32> = Settle::default();
        settle.push(2, b, 100);
        settle.push(1, a, 0);
        assert_eq!(settle.next_due(), Some(250));
        assert_eq!(settle.take_due(400), vec![(1, a), (2, b)]);
        settle.push(1, a, 1_000);
        settle.push(2, b, 1_000);
        assert!(settle.cancel(1));
        assert!(!settle.cancel(1));
        assert_eq!(settle.take_due(2_000), vec![(2, b)]);
    }

    #[test]
    fn the_requery_guard_lets_a_burst_through_then_pauses() {
        let mut guard = RequeryGuard::default();
        for i in 0..RequeryGuard::BURST {
            assert_eq!(guard.allow(100 + u64::from(i)), Requery::Go, "{i}");
        }
        assert_eq!(guard.allow(200), Requery::Pause);
        assert_eq!(guard.allow(300), Requery::Paused);
        assert_eq!(
            guard.allow(200 + RequeryGuard::PAUSE_MS - 1),
            Requery::Paused
        );
        // After the pause, counting starts again.
        assert_eq!(guard.allow(200 + RequeryGuard::PAUSE_MS), Requery::Go);
    }

    #[test]
    fn the_requery_guard_forgets_repositions_older_than_its_window() {
        let mut guard = RequeryGuard::default();
        let mut now = 0;
        // One reposition every 300 ms never pauses: at most 7 per window.
        for _ in 0..40 {
            assert_eq!(guard.allow(now), Requery::Go, "at {now} ms");
            now += 300;
        }
    }

    #[test]
    fn the_requery_guard_owes_a_query_for_a_reposition_it_refused() {
        let mut guard = RequeryGuard::default();
        assert_eq!(guard.owed_at(), None);
        for i in 0..RequeryGuard::BURST {
            assert_eq!(guard.allow(100 + u64::from(i)), Requery::Go, "{i}");
        }
        assert_eq!(guard.owed_at(), None, "nothing refused yet");
        assert_eq!(guard.allow(200), Requery::Pause);
        let end = 200 + RequeryGuard::PAUSE_MS;
        assert_eq!(guard.owed_at(), Some(end));
        // The band seen back in place: nothing owed, the pause goes on.
        guard.caught_up();
        assert_eq!(guard.owed_at(), None);
        assert_eq!(guard.allow(300), Requery::Paused);
        assert_eq!(guard.owed_at(), Some(end));
        assert!(!guard.take_owed(end - 1));
        assert!(guard.take_owed(end));
        assert_eq!(guard.owed_at(), None);
        assert!(!guard.take_owed(end + 1));
        // The query owed goes through: the pause is over.
        assert_eq!(guard.allow(end), Requery::Go);
        assert_eq!(guard.owed_at(), None);
    }

    #[test]
    fn retry_waits_longer_after_each_failure_in_a_row_up_to_a_cap() {
        let mut retry = Retry::default();
        assert_eq!(retry.due(), None);
        assert_eq!(retry.failed(0), 1_000);
        assert_eq!(retry.due(), Some(1_000));
        assert!(!retry.take_due(999));
        assert!(retry.take_due(1_000));
        assert_eq!(retry.due(), None);
        assert!(!retry.take_due(5_000));
        assert_eq!(retry.failed(1_000), 5_000);
        assert_eq!(retry.failed(6_000), 30_000);
        assert_eq!(retry.failed(36_000), 60_000);
        assert_eq!(retry.failed(96_000), 60_000);
        assert_eq!(retry.due(), Some(156_000));
        assert_eq!(retry.failures(), 5);
        retry.reset();
        assert_eq!(retry, Retry::default());
        assert_eq!(retry.failed(0), 1_000);
    }

    #[test]
    fn settle_says_which_windows_have_a_request_waiting() {
        let mut settle: Settle<u32> = Settle::default();
        assert!(!settle.is_waiting(1));
        settle.push(1, request(MONITOR, DockPosition::Bottom, 72), 0);
        assert!(settle.is_waiting(1));
        assert!(!settle.is_waiting(2));
        settle.take_due(250);
        assert!(!settle.is_waiting(1));
    }

    // -- Bars ----------------------------------------------------------------

    const H: isize = 0xA;
    const H2: isize = 0xB;
    const H3: isize = 0xC;
    /// The bottom band [`query_rect`] proposes for 72 px on [`MONITOR`]; also
    /// what the shell grants with the taskbar hidden.
    const BOTTOM_ASK: Rect = Rect::new(0, 1008, 1920, 72);
    /// The bottom band above the fake shell's 48 px taskbar.
    const ABOVE_TASKBAR: Rect = Rect::new(0, 960, 1920, 72);

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Sent {
        New(isize),
        Remove(isize),
        Query(isize, Rect),
        SetPos(isize, Rect),
        Activate(isize, bool),
        PosChanged(isize),
    }

    /// A scripted shell on [`MONITOR`]: a taskbar `taskbar` px thick on the
    /// bottom edge (0: hidden) that `ABM_QUERYPOS` moves a bottom band past,
    /// as Explorer does; `ABM_NEW` refused `refusals` more times;
    /// `ABM_SETPOS` answers from `set_answers` first, then grants what it is
    /// asked for. Records every message.
    #[derive(Debug)]
    struct FakeShell {
        present: bool,
        refusals: u32,
        taskbar: i32,
        set_answers: VecDeque<Rect>,
        sent: Vec<Sent>,
    }

    impl FakeShell {
        fn new() -> Self {
            Self {
                present: true,
                refusals: 0,
                taskbar: 48,
                set_answers: VecDeque::new(),
                sent: Vec::new(),
            }
        }

        /// The messages sent since the last call.
        fn take(&mut self) -> Vec<Sent> {
            std::mem::take(&mut self.sent)
        }
    }

    impl AppBarShell for FakeShell {
        fn present(&mut self) -> bool {
            self.present
        }

        fn register(&mut self, handle: isize) -> bool {
            self.sent.push(Sent::New(handle));
            if self.refusals > 0 {
                self.refusals -= 1;
                return false;
            }
            true
        }

        fn remove(&mut self, handle: isize) {
            self.sent.push(Sent::Remove(handle));
        }

        fn query(&mut self, handle: isize, edge: DockPosition, rect: Rect) -> Rect {
            self.sent.push(Sent::Query(handle, rect));
            let taskbar_top = MONITOR.bottom() - self.taskbar;
            if edge == DockPosition::Bottom && self.taskbar > 0 && rect.bottom() > taskbar_top {
                let top = rect.y.min(taskbar_top);
                return Rect::new(rect.x, top, rect.width, taskbar_top - top);
            }
            rect
        }

        fn set_pos(&mut self, handle: isize, _edge: DockPosition, rect: Rect) -> Rect {
            self.sent.push(Sent::SetPos(handle, rect));
            self.set_answers.pop_front().unwrap_or(rect)
        }

        fn activate(&mut self, handle: isize, active: bool) {
            self.sent.push(Sent::Activate(handle, active));
        }

        fn window_pos_changed(&mut self, handle: isize) {
            self.sent.push(Sent::PosChanged(handle));
        }
    }

    type Reports = Arc<Mutex<Vec<(NativeWindowHandle, Option<AppBarGrant>)>>>;

    fn recorder() -> (AppBarSink, Reports) {
        let reports: Reports = Arc::default();
        let seen = Arc::clone(&reports);
        let sink: AppBarSink = Arc::new(move |handle, grant| seen.lock().push((handle, grant)));
        (sink, reports)
    }

    /// The reports since the last call.
    fn drain(reports: &Reports) -> Vec<(NativeWindowHandle, Option<AppBarGrant>)> {
        std::mem::take(&mut *reports.lock())
    }

    fn bottom() -> AppBarRequest {
        request(MONITOR, DockPosition::Bottom, 72)
    }

    fn reported(handle: isize, request: AppBarRequest, rect: Rect) -> Vec<ReportOf> {
        vec![(
            NativeWindowHandle(handle),
            Some(AppBarGrant { request, rect }),
        )]
    }

    fn reported_none(handle: isize) -> Vec<ReportOf> {
        vec![(NativeWindowHandle(handle), None)]
    }

    type ReportOf = (NativeWindowHandle, Option<AppBarGrant>);

    /// [`H`]'s bar on the bottom edge above the fake shell's taskbar,
    /// registered at 250 ms, with nothing sent or reported left over.
    fn registered() -> (FakeShell, Bars, AppBarSink, Reports) {
        let mut shell = FakeShell::new();
        let (sink, reports) = recorder();
        let mut bars = Bars::default();
        bars.set(&mut shell, H, Some(bottom()), Arc::clone(&sink), 0);
        bars.tick(&mut shell, 250);
        assert_eq!(shell.take().len(), 3);
        assert_eq!(drain(&reports), reported(H, bottom(), ABOVE_TASKBAR));
        (shell, bars, sink, reports)
    }

    #[test]
    fn a_request_settles_then_registers_on_the_bounds_and_reports_the_band() {
        let mut shell = FakeShell::new();
        let (sink, reports) = recorder();
        let mut bars = Bars::default();
        bars.set(&mut shell, H, Some(bottom()), sink, 0);
        assert_eq!(shell.take(), vec![]);
        assert_eq!(bars.next_due(), Some(250));
        bars.tick(&mut shell, 249);
        assert_eq!(shell.take(), vec![]);
        bars.tick(&mut shell, 250);
        assert_eq!(
            shell.take(),
            vec![
                Sent::New(H),
                Sent::Query(H, BOTTOM_ASK),
                Sent::SetPos(H, ABOVE_TASKBAR)
            ]
        );
        assert_eq!(drain(&reports), reported(H, bottom(), ABOVE_TASKBAR));
        assert_eq!(bars.next_due(), None);
    }

    #[test]
    fn a_request_that_changes_nothing_reports_the_band_again() {
        let (mut shell, mut bars, sink, reports) = registered();
        // The window asks for the top edge and comes back before the settle
        // ends; its caller dropped the band meanwhile and must learn it again.
        let top = request(MONITOR, DockPosition::Top, 72);
        bars.set(&mut shell, H, Some(top), Arc::clone(&sink), 1_000);
        bars.set(&mut shell, H, Some(bottom()), sink, 1_100);
        bars.tick(&mut shell, 1_350);
        assert_eq!(shell.take(), vec![]);
        assert_eq!(drain(&reports), reported(H, bottom(), ABOVE_TASKBAR));
    }

    #[test]
    fn a_release_is_sent_at_once_cancels_what_waits_and_reports_nothing() {
        let (mut shell, mut bars, sink, reports) = registered();
        let thicker = request(MONITOR, DockPosition::Bottom, 96);
        bars.set(&mut shell, H, Some(thicker), Arc::clone(&sink), 1_000);
        bars.set(&mut shell, H, None, Arc::clone(&sink), 1_010);
        assert_eq!(shell.take(), vec![Sent::Remove(H)]);
        assert_eq!(bars.next_due(), None);
        bars.tick(&mut shell, 5_000);
        assert_eq!(shell.take(), vec![]);
        assert_eq!(drain(&reports), vec![]);
        // Released again: nothing to give back; the shell's notifications
        // move nothing.
        bars.set(&mut shell, H, None, sink, 6_000);
        bars.notify(&mut shell, H, Notification::PosChanged, 6_100);
        assert_eq!(shell.take(), vec![]);
        assert_eq!(drain(&reports), vec![]);
    }

    #[test]
    fn another_edge_gives_the_old_band_back_first_and_another_thickness_only_moves_it() {
        let (mut shell, mut bars, sink, reports) = registered();
        let top = request(MONITOR, DockPosition::Top, 72);
        let top_band = Rect::new(0, 0, 1920, 72);
        bars.set(&mut shell, H, Some(top), Arc::clone(&sink), 1_000);
        bars.tick(&mut shell, 1_250);
        assert_eq!(
            shell.take(),
            vec![
                Sent::Remove(H),
                Sent::New(H),
                Sent::Query(H, top_band),
                Sent::SetPos(H, top_band)
            ]
        );
        assert_eq!(drain(&reports), reported(H, top, top_band));
        let thicker = request(MONITOR, DockPosition::Top, 96);
        let thicker_band = Rect::new(0, 0, 1920, 96);
        bars.set(&mut shell, H, Some(thicker), sink, 2_000);
        bars.tick(&mut shell, 2_250);
        assert_eq!(
            shell.take(),
            vec![Sent::Query(H, thicker_band), Sent::SetPos(H, thicker_band)]
        );
        assert_eq!(drain(&reports), reported(H, thicker, thicker_band));
    }

    #[test]
    fn a_band_the_shell_answers_badly_is_given_up_and_asked_for_again_with_a_backoff() {
        let mut shell = FakeShell::new();
        // Three answers on the next monitor (a display change racing the
        // request), then good ones.
        let elsewhere = Rect::new(1920, 960, 1920, 72);
        shell.set_answers = VecDeque::from([elsewhere; 3]);
        let (sink, reports) = recorder();
        let mut bars = Bars::default();
        bars.set(&mut shell, H, Some(bottom()), sink, 0);
        bars.tick(&mut shell, 250);
        let given_up = vec![
            Sent::New(H),
            Sent::Query(H, BOTTOM_ASK),
            Sent::SetPos(H, ABOVE_TASKBAR),
            Sent::Remove(H),
        ];
        assert_eq!(shell.take(), given_up);
        assert_eq!(drain(&reports), reported_none(H));
        assert_eq!(bars.next_due(), Some(1_250));
        bars.tick(&mut shell, 1_249);
        assert_eq!(shell.take(), vec![]);
        bars.tick(&mut shell, 1_250);
        assert_eq!(shell.take(), given_up);
        // Still no band: nothing new to report.
        assert_eq!(drain(&reports), vec![]);
        assert_eq!(bars.next_due(), Some(6_250));
        bars.tick(&mut shell, 6_250);
        assert_eq!(shell.take(), given_up);
        assert_eq!(bars.next_due(), Some(36_250));
        bars.tick(&mut shell, 36_250);
        assert_eq!(
            shell.take(),
            vec![
                Sent::New(H),
                Sent::Query(H, BOTTOM_ASK),
                Sent::SetPos(H, ABOVE_TASKBAR)
            ]
        );
        assert_eq!(drain(&reports), reported(H, bottom(), ABOVE_TASKBAR));
        assert_eq!(bars.next_due(), None);
    }

    #[test]
    fn a_band_lost_to_a_bad_answer_after_a_notification_is_asked_for_again() {
        let (mut shell, mut bars, _sink, reports) = registered();
        // The taskbar hides itself and the shell answers the new band badly.
        shell.taskbar = 0;
        shell.set_answers = VecDeque::from([Rect::new(0, 0, 0, 0)]);
        bars.notify(&mut shell, H, Notification::PosChanged, 1_000);
        assert_eq!(
            shell.take(),
            vec![
                Sent::Query(H, BOTTOM_ASK),
                Sent::SetPos(H, BOTTOM_ASK),
                Sent::Remove(H)
            ]
        );
        assert_eq!(drain(&reports), reported_none(H));
        assert_eq!(bars.next_due(), Some(2_000));
        // Without a band the shell's notifications move nothing.
        bars.notify(&mut shell, H, Notification::PosChanged, 1_500);
        assert_eq!(shell.take(), vec![]);
        bars.tick(&mut shell, 2_000);
        assert_eq!(
            shell.take(),
            vec![
                Sent::New(H),
                Sent::Query(H, BOTTOM_ASK),
                Sent::SetPos(H, BOTTOM_ASK)
            ]
        );
        assert_eq!(drain(&reports), reported(H, bottom(), BOTTOM_ASK));
        assert_eq!(bars.next_due(), None);
    }

    #[test]
    fn a_refused_registration_is_tried_once_more_at_once_then_after_a_delay() {
        let mut shell = FakeShell::new();
        let (sink, reports) = recorder();
        let mut bars = Bars::default();
        // Refused once (a stale registration): removed and registered.
        shell.refusals = 1;
        bars.set(&mut shell, H2, Some(bottom()), Arc::clone(&sink), 0);
        bars.tick(&mut shell, 250);
        assert_eq!(
            shell.take(),
            vec![
                Sent::New(H2),
                Sent::Remove(H2),
                Sent::New(H2),
                Sent::Query(H2, BOTTOM_ASK),
                Sent::SetPos(H2, ABOVE_TASKBAR)
            ]
        );
        assert_eq!(drain(&reports), reported(H2, bottom(), ABOVE_TASKBAR));
        // Refused twice: given up, and tried again later.
        shell.refusals = 2;
        bars.set(&mut shell, H, Some(bottom()), sink, 1_000);
        bars.tick(&mut shell, 1_250);
        assert_eq!(
            shell.take(),
            vec![Sent::New(H), Sent::Remove(H), Sent::New(H)]
        );
        assert_eq!(drain(&reports), reported_none(H));
        assert_eq!(bars.next_due(), Some(2_250));
        bars.tick(&mut shell, 2_250);
        assert_eq!(
            shell.take(),
            vec![
                Sent::New(H),
                Sent::Query(H, BOTTOM_ASK),
                Sent::SetPos(H, ABOVE_TASKBAR)
            ]
        );
        assert_eq!(drain(&reports), reported(H, bottom(), ABOVE_TASKBAR));
    }

    #[test]
    fn without_the_shell_nothing_is_sent_and_taskbar_created_registers_at_once() {
        let mut shell = FakeShell::new();
        shell.present = false;
        let (sink, reports) = recorder();
        let mut bars = Bars::default();
        bars.set(&mut shell, H, Some(bottom()), Arc::clone(&sink), 0);
        bars.tick(&mut shell, 250);
        assert_eq!(shell.take(), vec![]);
        assert_eq!(drain(&reports), reported_none(H));
        assert_eq!(bars.next_due(), Some(1_250));
        bars.tick(&mut shell, 1_250);
        assert_eq!(shell.take(), vec![]);
        assert_eq!(drain(&reports), vec![]);
        assert_eq!(bars.next_due(), Some(6_250));
        // Explorer is back: `TaskbarCreated` registers at once.
        shell.present = true;
        bars.shell_restarted(&mut shell, H, 2_000);
        assert_eq!(
            shell.take(),
            vec![
                Sent::New(H),
                Sent::Query(H, BOTTOM_ASK),
                Sent::SetPos(H, ABOVE_TASKBAR)
            ]
        );
        assert_eq!(drain(&reports), reported(H, bottom(), ABOVE_TASKBAR));
        assert_eq!(bars.next_due(), None);
        // For a bar that wants nothing it sends nothing.
        bars.set(&mut shell, H, None, sink, 3_000);
        assert_eq!(shell.take(), vec![Sent::Remove(H)]);
        bars.shell_restarted(&mut shell, H, 3_100);
        assert_eq!(shell.take(), vec![]);
    }

    #[test]
    fn a_bar_the_shell_left_unpositioned_is_positioned_again() {
        let (mut shell, mut bars, sink, reports) = registered();
        // Explorer is gone when the icon size changes.
        shell.present = false;
        let thicker = request(MONITOR, DockPosition::Bottom, 96);
        bars.set(&mut shell, H, Some(thicker), sink, 1_000);
        bars.tick(&mut shell, 1_250);
        assert_eq!(shell.take(), vec![]);
        assert_eq!(drain(&reports), reported_none(H));
        // Still registered: the next attempt positions only.
        shell.present = true;
        bars.tick(&mut shell, 2_250);
        assert_eq!(
            shell.take(),
            vec![
                Sent::Query(H, Rect::new(0, 984, 1920, 96)),
                Sent::SetPos(H, Rect::new(0, 936, 1920, 96))
            ]
        );
        assert_eq!(
            drain(&reports),
            reported(H, thicker, Rect::new(0, 936, 1920, 96))
        );
        assert_eq!(bars.next_due(), None);
    }

    #[test]
    fn a_notification_sets_the_band_again_only_when_it_moved() {
        let (mut shell, mut bars, _sink, reports) = registered();
        bars.notify(&mut shell, H, Notification::PosChanged, 1_000);
        assert_eq!(shell.take(), vec![Sent::Query(H, BOTTOM_ASK)]);
        assert_eq!(drain(&reports), vec![]);
        // The taskbar hides itself: the band moves down to the screen's edge.
        shell.taskbar = 0;
        bars.notify(&mut shell, H, Notification::StateChange, 2_000);
        assert_eq!(
            shell.take(),
            vec![Sent::Query(H, BOTTOM_ASK), Sent::SetPos(H, BOTTOM_ASK)]
        );
        assert_eq!(drain(&reports), reported(H, bottom(), BOTTOM_ASK));
        // Fullscreen, tiling and unknown codes send nothing.
        for notification in [
            Notification::FullScreenApp(true),
            Notification::FullScreenApp(false),
            Notification::WindowArrange(true),
            Notification::Other(99),
        ] {
            bars.notify(&mut shell, H, notification, 3_000);
        }
        assert_eq!(shell.take(), vec![]);
        assert_eq!(bars.next_due(), None);
    }

    /// Drives [`RequeryGuard::BURST`] repositions of `H`'s band (the fake
    /// taskbar shown and hidden in turn), then one more that the guard
    /// refuses, with the taskbar hidden; returns when the pause ends.
    fn burst_into_a_pause(shell: &mut FakeShell, bars: &mut Bars, reports: &Reports) -> u64 {
        let mut now = 1_000;
        for i in 0..RequeryGuard::BURST {
            shell.taskbar = if i % 2 == 0 { 0 } else { 48 };
            bars.notify(shell, H, Notification::PosChanged, now);
            assert_eq!(shell.take().len(), 2, "query and set, {i}");
            now += 10;
        }
        drain(reports);
        // The last band asked for is above the taskbar; the next is refused.
        shell.taskbar = 0;
        bars.notify(shell, H, Notification::PosChanged, now);
        assert_eq!(shell.take(), vec![Sent::Query(H, BOTTOM_ASK)]);
        now + RequeryGuard::PAUSE_MS
    }

    #[test]
    fn after_the_requery_pause_the_bar_follows_a_band_moved_during_it() {
        let (mut shell, mut bars, _sink, reports) = registered();
        let end = burst_into_a_pause(&mut shell, &mut bars, &reports);
        assert_eq!(bars.next_due(), Some(end));
        // During the pause the band is queried, never set.
        bars.notify(&mut shell, H, Notification::PosChanged, end - 5_000);
        assert_eq!(shell.take(), vec![Sent::Query(H, BOTTOM_ASK)]);
        bars.tick(&mut shell, end - 1);
        assert_eq!(shell.take(), vec![]);
        // At its end the bar follows the band, without a notification.
        bars.tick(&mut shell, end);
        assert_eq!(
            shell.take(),
            vec![Sent::Query(H, BOTTOM_ASK), Sent::SetPos(H, BOTTOM_ASK)]
        );
        assert_eq!(drain(&reports), reported(H, bottom(), BOTTOM_ASK));
        assert_eq!(bars.next_due(), None);
    }

    #[test]
    fn a_band_back_in_place_by_the_end_of_the_pause_is_not_set_again() {
        let (mut shell, mut bars, _sink, reports) = registered();
        let end = burst_into_a_pause(&mut shell, &mut bars, &reports);
        // The taskbar is back where the bar last asked for its band.
        shell.taskbar = 48;
        bars.tick(&mut shell, end);
        assert_eq!(shell.take(), vec![Sent::Query(H, BOTTOM_ASK)]);
        assert_eq!(drain(&reports), vec![]);
        assert_eq!(bars.next_due(), None);
        // Seen back in place by a notification during the pause: nothing is
        // owed at all.
        let (mut shell, mut bars, _sink, reports) = registered();
        let end = burst_into_a_pause(&mut shell, &mut bars, &reports);
        shell.taskbar = 48;
        bars.notify(&mut shell, H, Notification::PosChanged, end - 5_000);
        assert_eq!(shell.take(), vec![Sent::Query(H, BOTTOM_ASK)]);
        assert_eq!(bars.next_due(), None);
    }

    #[test]
    fn activation_and_moves_reach_the_shell_only_for_registered_bars() {
        let mut shell = FakeShell::new();
        let mut bars = Bars::default();
        bars.activate(&mut shell, H, true);
        bars.window_pos_changed(&mut shell, H);
        assert_eq!(shell.take(), vec![]);
        let (mut shell, mut bars, _sink, _reports) = registered();
        bars.activate(&mut shell, H, true);
        bars.activate(&mut shell, H, false);
        bars.window_pos_changed(&mut shell, H);
        bars.activate(&mut shell, H2, true);
        assert_eq!(
            shell.take(),
            vec![
                Sent::Activate(H, true),
                Sent::Activate(H, false),
                Sent::PosChanged(H)
            ]
        );
    }

    #[test]
    fn a_destroyed_window_or_the_quit_gives_the_bands_back_and_reports_nothing() {
        let (mut shell, mut bars, sink, reports) = registered();
        let left = request(MONITOR, DockPosition::Left, 72);
        bars.set(&mut shell, H2, Some(left), Arc::clone(&sink), 1_000);
        bars.tick(&mut shell, 1_250);
        assert_eq!(shell.take().len(), 3);
        drain(&reports);
        // A third window's request still waits.
        bars.set(&mut shell, H3, Some(bottom()), sink, 2_000);
        bars.destroyed(&mut shell, H);
        assert_eq!(shell.take(), vec![Sent::Remove(H)]);
        bars.notify(&mut shell, H, Notification::PosChanged, 2_010);
        bars.shell_restarted(&mut shell, H, 2_020);
        assert_eq!(shell.take(), vec![]);
        bars.release_all(&mut shell);
        assert_eq!(shell.take(), vec![Sent::Remove(H2)]);
        assert_eq!(bars.next_due(), None);
        bars.tick(&mut shell, 10_000);
        assert_eq!(shell.take(), vec![]);
        assert_eq!(drain(&reports), vec![]);
    }
}
