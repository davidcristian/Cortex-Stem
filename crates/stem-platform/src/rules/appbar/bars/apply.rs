//! How [`Bars`] applies a request and follows the shell's notifications:
//! the messages it sends for one bar and what it reports.

use stem_core::Rect;

use super::{AppBarShell, Bars};
use crate::rules::appbar::bar::{Bar, Failure, Registered, Report, Step, steps};
use crate::rules::appbar::geometry::{across, is_usable, query_rect, set_rect};
use crate::rules::appbar::pacing::{Requery, RequeryGuard};
use crate::{AppBarGrant, AppBarRequest};

impl Bars {
    /// Bring the bar of `handle` in line with `request` ([`steps`]; a bar
    /// registered whose band was never granted is positioned again), then
    /// report the band it holds, even when it is the one reported last. A
    /// failure reports `None` and schedules another attempt.
    pub(super) fn apply(
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
    pub(super) fn requery(
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
