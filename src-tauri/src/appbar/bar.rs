//! One dock window's bar: the request sent to the platform for it and the
//! band the platform granted.

use stem_core::settings::DockPosition;
use stem_core::{MonitorInfo, Rect};
use stem_platform::rules::appbar::work_area_excludes;
use stem_platform::{AppBarGrant, AppBarRequest, NativeWindowHandle};

use super::rules::grant_fits;

/// One dock window's bar, as this module asked for it. Its bookkeeping is
/// plain data ([`Bar::want`], [`Bar::take_grant`], [`Bar::band`]), so it is
/// unit-tested without an `AppHandle`.
#[derive(Debug, Clone, Default)]
pub(super) struct Bar {
    pub(super) handle: Option<NativeWindowHandle>,
    /// The request last handed to the platform; `None`: no band.
    pub(super) sent: Option<AppBarRequest>,
    /// The band the platform reported for `sent` (or for an earlier request
    /// on the same edge of the same monitor); `None` until it does.
    pub(super) grant: Option<AppBarGrant>,
    /// The band given back last, placed on until the work area has grown
    /// over it ([`granted_band`]); dropped with the next request.
    ///
    /// [`granted_band`]: super::granted_band
    pub(super) released: Option<AppBarGrant>,
}

/// What a report of the platform changed for a bar ([`Bar::take_grant`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GrantChange {
    /// Nothing: the band it already had, or still none.
    Same,
    /// A new band for the request in effect: lay the window out on it.
    Granted(AppBarGrant),
    /// The band it had is gone.
    Gone,
}

impl Bar {
    /// Take `current`, the window's handle now, as the bar's window. A bar
    /// left with another handle belongs to a window that is gone (a dock
    /// window destroyed and re-created under the same label): it starts
    /// over for the new window, and when it had asked for a band the old
    /// handle comes back so that band can be given back. A handle that
    /// cannot be read (`None`) keeps the one the bar has.
    pub(super) fn rebind(
        &mut self,
        current: Option<NativeWindowHandle>,
    ) -> Option<NativeWindowHandle> {
        if current.is_none() || self.handle == current {
            return None;
        }
        let old = std::mem::take(self);
        self.handle = current;
        old.handle.filter(|_| old.sent.is_some())
    }

    /// Take `wanted` as the request in effect; `true` when it differs from
    /// the one sent last and must go to the platform. A release keeps the
    /// band it had as the band given back (placed on until the work area
    /// has grown over it); a request drops the band given back, and the band
    /// it had unless that answers the same edge of the same monitor (the
    /// thickness may still be on its way).
    pub(super) fn want(&mut self, wanted: Option<AppBarRequest>) -> bool {
        if self.sent == wanted {
            return false;
        }
        self.sent = wanted;
        match wanted {
            None => {
                if let Some(grant) = self.grant.take() {
                    self.released = Some(grant);
                }
            }
            Some(request) => {
                self.released = None;
                if self
                    .grant
                    .is_some_and(|grant| !grant_fits(&grant, &request))
                {
                    self.grant = None;
                }
            }
        }
        true
    }

    /// The platform reported `grant` for this window. Kept only when it
    /// answers the request in effect (a band for an earlier edge or
    /// monitor, or one reported after a release, counts as none). `sent`
    /// stays as it is on a `None`: the platform keeps asking for that
    /// request and reports the band once it holds it (`AppBarSink`).
    pub(super) fn take_grant(&mut self, grant: Option<AppBarGrant>) -> GrantChange {
        let grant = grant.filter(|grant| self.sent.is_some_and(|sent| grant_fits(grant, &sent)));
        if self.grant == grant {
            return GrantChange::Same;
        }
        self.grant = grant;
        match grant {
            Some(grant) => GrantChange::Granted(grant),
            None => GrantChange::Gone,
        }
    }

    /// The band the window goes on for `edge` of `monitor` ([`granted_band`]):
    /// the band granted for exactly that place while it is asked for, or the
    /// band given back last for that place while `monitor`'s work area
    /// still leaves it out. A band given back that no longer applies (another
    /// place, or a work area grown over it) is forgotten. `label` is for the
    /// log.
    ///
    /// [`granted_band`]: super::granted_band
    pub(super) fn band(
        &mut self,
        label: &str,
        edge: DockPosition,
        monitor: &MonitorInfo,
    ) -> Option<Rect> {
        let fits = |grant: &AppBarGrant| {
            grant.request.edge == edge && grant.request.monitor == monitor.bounds
        };
        if let (Some(sent), Some(grant)) = (self.sent, self.grant) {
            return (fits(&grant) && grant_fits(&grant, &sent)).then_some(grant.rect);
        }
        let released = self.released?;
        if !fits(&released) {
            self.released = None;
            return None;
        }
        if work_area_excludes(monitor.work_area, released.rect, edge) {
            return Some(released.rect);
        }
        log::debug!(
            "dock window {label:?}: the band given back is free again; placing on the work area"
        );
        self.released = None;
        None
    }
}
