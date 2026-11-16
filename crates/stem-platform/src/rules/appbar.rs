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
//!
//! [`Rect`]: stem_core::Rect

mod bar;
mod bars;
mod geometry;
mod pacing;

pub use bar::{Step, steps};
pub use bars::{AppBarShell, Bars, Notification};
pub use geometry::{
    PANEL_SLACK_CSS, across, band_thickness, is_usable, query_rect, reserved_thickness, set_rect,
    window_rect, work_area_excludes,
};
pub use pacing::{Requery, RequeryGuard, Retry, Settle};

#[cfg(test)]
mod tests;
