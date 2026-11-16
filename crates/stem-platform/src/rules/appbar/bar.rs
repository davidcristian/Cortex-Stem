//! One dock window's bar: its registration with the shell, the steps that
//! change it, and what it reports.

use stem_core::Rect;

use super::pacing::{RequeryGuard, Retry};
use crate::{AppBarGrant, AppBarRequest, AppBarSink, NativeWindowHandle};

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

/// Why a bar could not take its band.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Failure {
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
pub(super) enum Report {
    /// Skip it.
    IfChanged,
    /// Send it anyway.
    Always,
}

/// A registration with the shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Registered {
    /// The request it was registered and last positioned for.
    pub(super) request: AppBarRequest,
    /// The band last passed to `ABM_SETPOS`.
    pub(super) asked: Option<Rect>,
    /// The band the shell granted last, while it was usable.
    pub(super) granted: Option<AppBarGrant>,
}

/// One dock window's bar.
pub(super) struct Bar {
    pub(super) sink: AppBarSink,
    /// What the window wants now (its latest request).
    pub(super) wanted: Option<AppBarRequest>,
    /// What the shell holds for the window, if it is registered.
    pub(super) registered: Option<Registered>,
    /// What was last reported to the sink.
    pub(super) reported: Option<Option<AppBarGrant>>,
    pub(super) requery: RequeryGuard,
    pub(super) retry: Retry,
}

impl Bar {
    pub(super) fn new(sink: AppBarSink) -> Self {
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
    pub(super) fn report(&mut self, handle: isize, grant: Option<AppBarGrant>, when: Report) {
        if when == Report::IfChanged && self.reported == Some(grant) {
            return;
        }
        self.reported = Some(grant);
        (self.sink)(NativeWindowHandle(handle), grant);
    }

    /// An attempt to take the band for `request` failed at `now_ms`: the
    /// next is scheduled ([`Retry`]) and `None` reported.
    pub(super) fn failed(
        &mut self,
        handle: isize,
        request: AppBarRequest,
        failure: Failure,
        now_ms: u64,
    ) {
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
