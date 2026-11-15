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
//!   its margin to the edge, never the magnification headroom.
//! - [`work_area_excludes`]: whether a work area still leaves a band out,
//!   so a band just given back is still placed on until the work area the
//!   platform reports has grown over it again.
//!
//! The bookkeeping: [`steps`] (what a new request takes: remove, register,
//! position), [`Settle`] (a request waits until its window's reports have
//! settled, so a burst costs one `ABM_SETPOS`) and [`RequeryGuard`] (a bound
//! on the repositions the shell's notifications cause).

use stem_core::Rect;
use stem_core::settings::DockPosition;

use crate::AppBarRequest;

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

/// The thickness to reserve for a window of `window` size (width, height)
/// whose resting panel is `panel` (in the same units, relative to the
/// window): from the panel's far side to the window's side on `edge`, which
/// is the panel's thickness plus its margin to the edge. Clamped to the
/// window; 0 for an empty panel.
pub fn reserved_thickness(window: (i32, i32), panel: Rect, edge: DockPosition) -> i32 {
    if panel.width <= 0 || panel.height <= 0 {
        return 0;
    }
    let (width, height) = window;
    let (distance, limit) = match edge {
        DockPosition::Bottom => (height - panel.y, height),
        DockPosition::Top => (panel.bottom(), height),
        DockPosition::Left => (panel.right(), width),
        DockPosition::Right => (width - panel.x, width),
    };
    distance.clamp(0, limit.max(0))
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
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RequeryGuard {
    /// When the current window of counting started.
    window_start: u64,
    /// Repositions let through in the current window.
    count: u32,
    /// While paused: the time from which repositions may go again.
    paused_until: Option<u64>,
}

impl RequeryGuard {
    pub const BURST: u32 = 8;
    pub const WINDOW_MS: u64 = 2_000;
    pub const PAUSE_MS: u64 = 10_000;

    /// Whether a reposition at `now_ms` may go ahead; counts the ones it
    /// lets through.
    pub fn allow(&mut self, now_ms: u64) -> Requery {
        if let Some(until) = self.paused_until {
            if now_ms < until {
                return Requery::Paused;
            }
            self.paused_until = None;
            self.count = 0;
        }
        if self.count == 0 || now_ms.saturating_sub(self.window_start) >= Self::WINDOW_MS {
            self.window_start = now_ms;
            self.count = 0;
        }
        if self.count >= Self::BURST {
            self.paused_until = Some(now_ms.saturating_add(Self::PAUSE_MS));
            return Requery::Pause;
        }
        self.count += 1;
        Requery::Go
    }
}

#[cfg(test)]
mod tests {
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
            72
        );
        // On the top edge, 8 px below the window's top.
        let panel = Rect::new(640, 8, 640, 64);
        assert_eq!(
            reserved_thickness((1920, 135), panel, DockPosition::Top),
            72
        );
        // Vertical: a 64×640 panel 8 px from the window's left side.
        let panel = Rect::new(8, 200, 64, 640);
        assert_eq!(
            reserved_thickness((135, 1032), panel, DockPosition::Left),
            72
        );
        // And 8 px from its right side.
        let panel = Rect::new(63, 200, 64, 640);
        assert_eq!(
            reserved_thickness((135, 1032), panel, DockPosition::Right),
            72
        );
        // The window grown for a drag: the panel's place follows, the band
        // does not.
        let panel = Rect::new(640, 935, 640, 64);
        assert_eq!(
            reserved_thickness((1920, 1007), panel, DockPosition::Bottom),
            72
        );
    }

    #[test]
    fn the_reserved_thickness_is_clamped_to_the_window_and_zero_without_a_panel() {
        assert_eq!(
            reserved_thickness((1920, 135), Rect::new(0, 0, 0, 0), DockPosition::Bottom),
            0
        );
        assert_eq!(
            reserved_thickness((1920, 135), Rect::new(10, 10, 0, 64), DockPosition::Top),
            0
        );
        // A panel rect still laid out for the bottom read on the left edge:
        // as long as the panel is (640), but never more than the window.
        assert_eq!(
            reserved_thickness((135, 1032), Rect::new(640, 63, 640, 64), DockPosition::Left),
            135
        );
        // A panel outside the window on the far side.
        assert_eq!(
            reserved_thickness(
                (1920, 135),
                Rect::new(0, 200, 640, 64),
                DockPosition::Bottom
            ),
            0
        );
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
}
