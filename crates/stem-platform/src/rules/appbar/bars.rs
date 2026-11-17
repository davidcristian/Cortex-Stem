//! Every dock window's bar, kept in line with the window's latest request
//! by the messages sent through an [`AppBarShell`].

mod apply;

use std::collections::HashMap;
use std::sync::Arc;

use stem_core::Rect;
use stem_core::settings::DockPosition;

use super::bar::Bar;
use super::pacing::{RequeryGuard, Settle};
use crate::{AppBarRequest, AppBarSink};

/// The shell as [`Bars`] sees it: one method per `SHAppBarMessage` it
/// sends, plus whether the shell runs at all. `windows::appbar` implements
/// it with the real messages on its thread; the tests in
/// `rules/appbar/tests/bars.rs` script it.
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
///
/// [`steps`]: super::steps
/// [`query_rect`]: super::query_rect
/// [`set_rect`]: super::set_rect
/// [`is_usable`]: super::is_usable
/// [`Retry`]: super::Retry
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
}
