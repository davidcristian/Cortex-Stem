//! The screen space a dock window reserves as an AppBar, and the band the
//! shell grants.

use std::sync::Arc;

use stem_core::Rect;
use stem_core::settings::DockPosition;

use crate::NativeWindowHandle;

/// The screen space a dock window reserves as an AppBar: a
/// band at its screen edge that the shell takes out of the monitor's work
/// area, so maximised windows stop short of the dock. Physical pixels of
/// the virtual desktop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppBarRequest {
    /// The full bounds of the window's monitor
    /// ([`MonitorInfo::bounds`](stem_core::MonitorInfo::bounds)),
    /// never its work area: the work area shrinks once the band is
    /// reserved, so a band asked for on its edge would move inwards with
    /// every answer. The shell itself moves the band past the stock taskbar
    /// and other bars on that edge ([`rules::appbar`](crate::rules::appbar)).
    pub monitor: Rect,
    /// The edge the band lies on.
    pub edge: DockPosition,
    /// Across the edge: the panel's resting thickness plus its margin to
    /// the edge, not the magnification headroom. The platform keeps it
    /// between 1 px and half the monitor
    /// ([`rules::appbar::band_thickness`](crate::rules::appbar::band_thickness)).
    pub thickness: i32,
}

/// The shell's answer to an [`AppBarRequest`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppBarGrant {
    /// The request this answers.
    pub request: AppBarRequest,
    /// The band the shell reserved, physical px: on the request's edge of
    /// its monitor, moved inwards past the bars already on that edge (the
    /// stock taskbar when it is visible there) and possibly shortened by
    /// bars on the neighbouring edges. The dock window goes with its edge
    /// side on this band's edge side
    /// ([`rules::appbar::window_rect`](crate::rules::appbar::window_rect)).
    pub rect: Rect,
}

/// Where a platform reports what became of a dock window's AppBar:
/// - `Some` with the band the shell grants after **every** request the
///   platform applied, even when it is the band reported before (the
///   caller may have dropped it while a newer request was on its way), and
///   whenever the shell moves the band (a bar or the taskbar changing on
///   that edge, an Explorer restart), then only when it differs from the
///   last one reported;
/// - `None` once the window holds no band for a request it still makes (a
///   registration that failed, or a band the shell answered badly and that
///   was given up). The platform keeps asking for that request, after a
///   growing delay (`rules::appbar::Retry`), and reports `Some` once it
///   holds the band; the caller need not ask again. Nothing is reported
///   after a release the caller asked for, nor at
///   [`Platform::release_app_bars`](crate::Platform::release_app_bars).
///
/// Called on a thread of the platform's own, or (the mock) on the caller's:
/// it must be cheap and must not block (post to the UI thread), and must
/// not call back into [`Platform::set_dock_app_bar`](crate::Platform::set_dock_app_bar)
/// synchronously.
pub type AppBarSink = Arc<dyn Fn(NativeWindowHandle, Option<AppBarGrant>) + Send + Sync + 'static>;
