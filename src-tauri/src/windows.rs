//! Window lifecycle: the dock window(s) and the settings window.
//!
//! One dock window per monitor: the label `dock` for the primary
//! monitor's window and `dock-<id>` for every other monitor's
//! while `settings.monitors` is `All` ([`dock_label`]: the monitor id with
//! every character outside `[A-Za-z0-9_-]` dropped; the page gets the id in
//! its URL, `dock.html?monitor=<id>`, while the primary's page has no query
//! string). [`plan_docks`] (pure) says which windows should exist for a mode
//! over a monitor list; [`reconcile_docks`] creates the missing ones, closes
//! the ones whose monitor is gone (never the primary's) and lays the rest
//! out again on their own monitor, at startup, on every `dock://monitors`
//! (hot-plug, resolution or work-area change, a new primary) and when the
//! `monitors` setting changes. One `info` line per window created or
//! closed. With `monitors = primary` (the default) exactly one window
//! exists and nothing of this is visible.
//!
//! Each window is transparent, undecorated, always on top, hidden from
//! Alt+Tab and the stock taskbar, and does not take focus when clicked:
//! `focusable(false)` makes tao itself keep `WS_EX_NOACTIVATE` across its
//! style rewrites, and `Platform::prepare_dock_window` (called on the main
//! thread, right after creation) adds the rest. A dock window is
//! never closed by the user: close requests are refused (this is a tray
//! app; see `lib.rs`); only a monitor that goes away, or the setting, closes
//! a secondary one (`destroy`, which skips the refusal).
//!
//! Sizing (`docs/ipc/sizing-protocol.md`, v1.16): a dock window sits on one
//! **edge** of its monitor's work area (`settings.position`: bottom, top,
//! left or right), flush with that edge and spanning the work area's **full
//! length along it** (its width on bottom and top, its height on left and
//! right), so the UI can centre the panel and animate items in and out
//! without any window resize. The frontend reports its content size in CSS
//! pixels through `set_dock_content_size`; this module takes the value
//! across the edge as the window's **thickness** (the height on a
//! horizontal dock, the width on a vertical one), scales it by the window's
//! scale factor and clamps it to the work area ([`plan_dock_rect`] over
//! [`edge_rect`], pure and unit-tested); the other value is informational
//! (logged; a lower bound for the length only on hosts without a monitor
//! list). What each window's UI reported last is remembered per label
//! ([`DockGeometry`]) so the layout can be redone when the service reports a
//! monitor change, the scale changes, or the edge changes
//! ([`on_settings_changed`]: every dock window moves to the new edge in
//! place, keeping its thickness until its UI reports again,
//! [`DockContent::on_edge`]). Every placement takes its rect from one pure
//! function, [`relayout_rect`] (over [`plan_dock_rect`] and [`edge_rect`]),
//! and goes through one function, [`place`], whose window calls never pass
//! through a rect outside both the old and the new place
//! ([`placement_steps`]), so a move between edges cannot cross onto a
//! neighbouring monitor with another scale on the way. Clicks outside the
//! panel fall through to the desktop via the hit region
//! ([`apply_hit_region`]), which `behavior.clickThrough` switches off
//! and on without a restart ([`on_settings_changed`]): the UI's latest
//! report is remembered while the setting is off and replayed when it is
//! turned on again.
//!
//! DPI changes: when a dock window's scale factor changes (Windows
//! Settings → Display → Scale, or the window landing on a monitor with
//! another scale) tao resizes the window to keep its logical size, which is
//! not our layout: [`on_scale_factor_changed`] plans that window again from
//! its remembered content size and converts its remembered hit region and
//! panel rect again, at the next turn of the event loop (inside the event
//! callback tao has not applied the new size yet). The UI learns the ratio
//! from the browser on its own, re-requests its icons and reports its
//! geometry again; no event is sent to it.
//!
//! Auto-hide and the toggle: each UI reports when auto-hide slides
//! its panel out and back (`set_dock_auto_hidden`, kept per label in
//! [`DockAutoHidden`]), so that [`toggle_dock`] (tray click, Ctrl+Alt+D)
//! brings a slid-out panel back with one press (the `dock://show` event)
//! instead of hiding the window first. One press acts on the whole set: the
//! primary's state decides, and every dock follows, including one
//! created while the set is hidden (hot-plug, *All monitors*): it starts
//! hidden like the others ([`show_new_dock_window`]) and the next press
//! shows it with them.
//!
//! The dock panel: each UI reports where its panel rests
//! (`set_dock_panel_rect`, CSS px of its viewport); [`apply_dock_panel`]
//! remembers the rect and hands it to the service in physical pixels of the
//! virtual desktop ([`panel_on_screen`]: scaled like the hit region and
//! offset by the window's own top-left corner, whichever edge the window is
//! on), with the window's label and monitor, which is what the engine
//! compares window frames against, per dock. The conversion is redone
//! whenever this module moves the window ([`place`]: a monitor or
//! work-area change moves it without the UI's viewport changing), the
//! service gets no rect while the window is hidden by the tray or the
//! toggle shortcut ([`toggle_dock`]), nor after an edge change until the UI
//! reports a rect laid out for the new edge ([`forget_edge_layout`]), and
//! neither monitor nor rect once the window is closed.
//!
//! Report provenance (`docs/ipc/report-provenance.md`, v1.18): the
//! page measures, then reports, and a report is handled here later, maybe
//! after this module moved or resized the window. Each of the three
//! geometry reports names the edge the page laid itself out for, and a
//! report naming another edge than the one the window was placed on last
//! ([`DockWindowState::stale_for`]) was measured before the page saw a
//! position change: it is dropped (one `debug` line), and the page's next
//! reports, laid out for the new edge, are applied as usual. One exception:
//! a content size for the edge in effect, while the window is not on it
//! (its move there failed), is taken and moves the window there
//! ([`DockWindowState::take_content`]), so a failed move does not leave the
//! dock dropping every report until an unrelated placement. The panel rect
//! also carries the viewport it is relative to ([`PanelReport`]): every
//! conversion re-anchors it to the window's viewport of the moment
//! ([`reanchor_panel`], the page's own layout: anchored to the edge side,
//! centred along it), so the engine's panel, the band and the backdrop read
//! it where the panel is now, after a resize the page has not reported yet
//! (its content size, sent in the same tick and applied first; a drag's
//! growth; a thickness change on `top` or `left`, which leaves the rect
//! unchanged, so the page does not report it again). The band's thickness
//! is read against the report's own viewport.
//!
//! Reserve screen space (`appbar`): while a dock window holds a band
//! the shell reserved for it as an AppBar, [`layout`] puts the window's
//! edge side on the band's edge side
//! (`stem_platform::rules::appbar::window_rect`) instead of on the work
//! area's edge (which is the band's far side once it is reserved); the
//! window keeps its thickness and length rule otherwise. Every placement
//! ([`place`]), show, hide and panel report asks `appbar::sync` whether the
//! band must change ([`app_bar_inputs`]); a new band lays the window out
//! again ([`relayout_later`]); a window that closes gives its band back
//! first (`appbar::forget`).
//!
//! Keyboard entry (`docs/ipc/keyboard-entry.md`, v1.19): the focus
//! shortcut (`behavior.focusShortcut`, `shortcut`) runs
//! [`on_focus_shortcut`], which remembers the window in front ([`DockFocus`],
//! [`return_target`]: the engine's active window, or the settings window
//! when it is in front), picks the dock window on that window's monitor
//! ([`focus_target`]), brings it on screen (shown, or `dock://show`), makes
//! it the foreground window off the main thread
//! (`Platform::focus_dock_window`) and sends its page `dock://focus`.
//! Escape in the page (`release_dock_focus`, [`release_dock_focus`]) brings
//! the remembered window back ([`give_focus_back`]: the `Activate` action for
//! an app's window, `set_focus` for the settings window); a second press
//! while that dock still has the keyboard sends its page
//! `dock://release-focus`, and the page does what Escape does. A release
//! from another dock window leaves the session alone while its own dock is
//! still in front ([`DockFocus::release`]). A click on a dock
//! ([`forget_dock_focus`]) forgets the remembered window; the dock losing
//! the activation ([`on_dock_blurred`], Tauri's `Focused(false)` while
//! another window is in front) ends the session.
//!
//! The settings window (`settings`) is an ordinary decorated window,
//! created on demand and destroyed when closed. Its native title bar follows
//! the *Theme* setting (`settings.theme`): set when the window is
//! created and again whenever the setting changes ([`on_settings_changed`]).

use std::collections::HashMap;
use std::fmt::Write as _;

use parking_lot::Mutex;
use stem_core::settings::{DockPosition, MonitorMode, ThemeMode};
use stem_core::{DockState, MonitorId, MonitorInfo, Rect, Settings, WindowAction, WindowId};
use stem_platform::rules::appbar::{across, reserved_thickness, window_rect};
use stem_platform::{DockWindowOptions, HitRegion, NativeWindowHandle, Platform, PlatformError};
use stem_service::DockService;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, WindowEvent,
};

pub const DOCK_LABEL: &str = "dock";
pub const SETTINGS_LABEL: &str = "settings";

/// Initial dock window size (logical px) at creation: its height is the
/// thickness the window takes on whichever edge it is first placed
/// ([`thickness_of`] with no edge yet) and keeps until the UI reports its
/// content; the length is replaced by the work area's as soon as the window
/// is placed.
const INITIAL_SIZE: (f64, f64) = (640.0, 96.0);
/// What the UI may report along the edge (the length, informational), in
/// CSS px; anything else is clamped ([`clamp_content`]).
const LENGTH_RANGE: (f64, f64) = (64.0, 8192.0);
/// What the UI may ask for across the edge (the thickness), in CSS px.
// The maximum is generous on purpose: during a drag-to-reorder the UI asks
// for a very thick window so the lifted icon can go anywhere on screen;
// the work area clamps it further.
const THICKNESS_RANGE: (f64, f64) = (32.0, 4096.0);

const SETTINGS_SIZE: (f64, f64) = (760.0, 600.0);
const SETTINGS_MIN_SIZE: (f64, f64) = (560.0, 420.0);

/// What one dock window's UI reported last (CSS px) and where this module
/// put the window, kept per label in [`DockGeometry`].
#[derive(Debug, Default, Clone)]
pub struct DockWindowState {
    /// The monitor the window was placed on last; `None` while the platform
    /// has reported no monitors yet (the window then sits where Tauri's own
    /// view of the primary monitor put it, and the engine gets no monitor
    /// for it).
    monitor: Option<MonitorId>,
    /// The latest `set_dock_content_size`, clamped, with the edge the page
    /// measured it for (the edge the window was on then, or the
    /// report was dropped); `None` before the UI's first report.
    content: Option<DockContent>,
    /// The edge the window was placed on last, taken as soon as the window
    /// has its size for that edge ([`place`]); `None` before its first
    /// placement. Says which side of the window is its thickness when no
    /// content was reported yet ([`thickness_of`]).
    edge: Option<DockPosition>,
    /// `(panel, envelope)` of the latest `set_dock_hit_region`; applied
    /// while `behavior.clickThrough` is on and replayed when it is turned
    /// back on.
    hit_region_css: Option<(Rect, Rect)>,
    /// The resting panel of the latest `set_dock_panel_rect` with
    /// the edge and the viewport the page measured it in; converted
    /// again, re-anchored to the window's viewport of the moment
    /// ([`PanelReport::now`]), whenever the window moves or is shown.
    panel: Option<PanelReport>,
    /// From that panel's far side to the viewport's edge side, CSS px, read
    /// against the viewport the page measured it in: the
    /// thickness of the band *Reserve screen space* asks for, scaled when
    /// the request is made (`appbar::request_for`). The distance to the edge
    /// side is what re-anchoring keeps, so it holds in the window's viewport
    /// of any moment: a report handled after a resize (a drop, a smaller
    /// icon size) gives the band its right thickness at once, and a window
    /// that grows for a drag never asks for a thicker band. Kept as it was
    /// only when the rect does not lie inside its own viewport across the
    /// edge (`reserved_thickness` is `None`), which a page laid out as
    /// `docs/ipc/sizing-protocol.md` says never sends. Dropped with the
    /// panel rect.
    reserve_css: Option<i32>,
    /// The window's scale factor as last seen (at creation, then on every
    /// `ScaleFactorChanged`), so the log line of a change can say what it
    /// changed from.
    scale: Option<f64>,
}

impl DockWindowState {
    /// The edge the window was placed on last when a report the page
    /// measured for `edge` does not belong to it: the page laid
    /// itself out for another edge, before it saw a position change, and
    /// the report is dropped. `None` when the report is current, and for any
    /// edge before the window's first placement (a window is placed when it
    /// is created, before its page loads).
    fn stale_for(&self, edge: DockPosition) -> Option<DockPosition> {
        self.edge.filter(|placed| *placed != edge)
    }

    /// Take a `set_dock_content_size` measured for `content.edge`, unless it
    /// is stale ([`Self::stale_for`]; `Err` with the window's edge) and
    /// `in_effect`, the edge the windows are laid out for ([`dock_edge`]),
    /// is not its edge either. A report for the edge in effect is never
    /// stale: the page measured it after it saw the position change. The
    /// window is not on that edge yet only when its move there failed
    /// ([`on_position_setting`] logs it; a getter or a resize failed) or has
    /// not run yet: `Ok(Some(placed))`, and the [`layout`] that follows the
    /// report is the move, retried. Without that, every report for the edge
    /// in effect would be dropped until an unrelated placement (a monitor or
    /// DPI change) moved the window. `Ok(None)` for a report on the window's
    /// edge.
    fn take_content(
        &mut self,
        content: DockContent,
        in_effect: DockPosition,
    ) -> Result<Option<DockPosition>, DockPosition> {
        let stale = self.stale_for(content.edge);
        if let Some(placed) = stale {
            if content.edge != in_effect {
                return Err(placed);
            }
        }
        self.content = Some(content);
        Ok(stale)
    }

    /// Take a `set_dock_hit_region` measured for `edge`, unless it is stale
    /// ([`Self::stale_for`]; `Err` with the window's edge).
    fn take_hit_region(
        &mut self,
        panel: Rect,
        envelope: Rect,
        edge: DockPosition,
    ) -> Result<(), DockPosition> {
        if let Some(placed) = self.stale_for(edge) {
            return Err(placed);
        }
        self.hit_region_css = Some((panel, envelope));
        Ok(())
    }

    /// Take a `set_dock_panel_rect`, unless it is stale
    /// ([`Self::stale_for`]; `Err` with the window's edge), with the band's
    /// thickness read against the report's own viewport: `Ok(Some(css))`,
    /// or `Ok(None)` when the rect does not lie inside that viewport across
    /// the edge, which keeps the thickness as it was.
    fn take_panel(&mut self, report: PanelReport) -> Result<Option<i32>, DockPosition> {
        if let Some(placed) = self.stale_for(report.edge) {
            return Err(placed);
        }
        self.panel = Some(report);
        let thickness = reserved_thickness(report.viewport.whole(), report.rect, report.edge);
        if thickness.is_some() {
            self.reserve_css = thickness;
        }
        Ok(thickness)
    }
}

/// A dock window's viewport in CSS px: the space the page measures in.
/// What `set_dock_panel_rect` carries as `viewport` (the page's
/// `window.innerWidth` × `window.innerHeight`, whole CSS px), and what the
/// window has now ([`CssViewport::of`] its client area, fractional at a
/// scale other than 1). The app layer's own type: no `stem-core` type
/// crosses IPC for it.
#[derive(Debug, Clone, Copy, PartialEq, serde::Deserialize)]
pub struct CssViewport {
    pub width: f64,
    pub height: f64,
}

impl CssViewport {
    /// A client area of `physical` px (width, height) at `scale` (1 when
    /// unusable). Pure.
    pub fn of(physical: (u32, u32), scale: f64) -> Self {
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        Self {
            width: f64::from(physical.0) / scale,
            height: f64::from(physical.1) / scale,
        }
    }

    /// Argument check for `set_dock_panel_rect`'s `viewport`: both sides
    /// finite and positive (`docs/ipc/report-provenance.md`). Pure.
    pub fn validate(self) -> Result<(), String> {
        let usable = |v: f64| v.is_finite() && v > 0.0;
        if usable(self.width) && usable(self.height) {
            Ok(())
        } else {
            Err(format!(
                "the viewport must have a positive, finite width and height: {self:?}"
            ))
        }
    }

    /// Rounded to whole CSS px, the space of the page's rects.
    fn whole(self) -> (i32, i32) {
        let whole = |v: f64| {
            if v.is_finite() {
                v.round().clamp(0.0, f64::from(i32::MAX)) as i32
            } else {
                0
            }
        };
        (whole(self.width), whole(self.height))
    }
}

/// One `set_dock_panel_rect` with its provenance, kept per
/// window ([`DockWindowState`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelReport {
    /// The panel at rest, whole CSS px relative to `viewport`.
    pub rect: Rect,
    /// The edge the page had laid itself out for.
    pub edge: DockPosition,
    /// The page's viewport the rect is relative to.
    pub viewport: CssViewport,
}

impl PanelReport {
    /// The rect in a window whose viewport is `now` ([`reanchor_panel`]):
    /// where the panel rests now. As reported when the window cannot say
    /// (`None`). Pure.
    pub fn now(self, now: Option<CssViewport>) -> Rect {
        match now {
            Some(now) => reanchor_panel(self.rect, self.edge, self.viewport, now),
            None => self.rect,
        }
    }
}

/// How far (CSS px) the page's viewport and the window's may differ on an
/// axis and still count as the same ([`reanchor_panel`]): the page reports
/// whole CSS px, and the window's client area of whole physical px is
/// fractional in CSS px at a scale other than 1, so the two differ by less
/// than 1 when nothing changed. The window's thickness and length change by
/// whole CSS px at 1×, and by more than one at any larger scale.
pub const VIEWPORT_SLACK_CSS: f64 = 1.0;

/// The panel `rect` (CSS px) that the page measured in the viewport `from`,
/// laid out for `edge`, moved to where the page's layout
/// (`docs/ipc/sizing-protocol.md`: the panel anchored to the window's edge
/// side and centred along it) puts it in the viewport `to`: across the edge
/// it keeps its distance to the viewport's edge side (on `bottom` it moves
/// by the change in height, on `right` by the change in width, on `top` and
/// `left` it stays), along the edge its offset from the viewport's centre
/// (it moves by half the change in length, rounded to the nearest CSS px).
/// Its size never changes. A difference under [`VIEWPORT_SLACK_CSS`] on an
/// axis moves nothing on it. Pure.
pub fn reanchor_panel(rect: Rect, edge: DockPosition, from: CssViewport, to: CssViewport) -> Rect {
    let change = |before: f64, after: f64| {
        let d = after - before;
        if d.is_finite() && d.abs() >= VIEWPORT_SLACK_CSS {
            d
        } else {
            0.0
        }
    };
    let dw = change(from.width, to.width);
    let dh = change(from.height, to.height);
    let (dx, dy) = match edge {
        DockPosition::Bottom => (dw / 2.0, dh),
        DockPosition::Top => (dw / 2.0, 0.0),
        DockPosition::Left => (0.0, dh / 2.0),
        DockPosition::Right => (dw, dh / 2.0),
    };
    let shift = |v: i32, d: f64| {
        (f64::from(v) + d)
            .round()
            .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
    };
    Rect::new(
        shift(rect.x, dx),
        shift(rect.y, dy),
        rect.width,
        rect.height,
    )
}

/// The dock windows that exist, by label, with what each one's UI reported,
/// plus the settings this module applies, as currently in effect.
/// Kept in Tauri's managed state so a monitor change can redo the layouts
/// and a settings change can apply the hit regions after the fact.
#[derive(Debug, Default)]
pub struct DockGeometry {
    docks: Mutex<HashMap<String, DockWindowState>>,
    /// `behavior.clickThrough` as currently in effect, so a settings event
    /// only acts on a change.
    click_through: Mutex<Option<bool>>,
    /// `settings.monitors` as the windows were last reconciled for, so a
    /// settings event only rebuilds the set on a change.
    mode: Mutex<Option<MonitorMode>>,
    /// `settings.position` as the windows are laid out for, so a
    /// settings event only moves them on a change; [`dock_edge`] reads it
    /// for every placement.
    position: Mutex<Option<DockPosition>>,
}

impl DockGeometry {
    /// A copy of the state of the dock window `label`, if it exists.
    fn dock(&self, label: &str) -> Option<DockWindowState> {
        self.docks.lock().get(label).cloned()
    }

    /// Change the state of the dock window `label`, if it exists.
    fn update<R>(&self, label: &str, f: impl FnOnce(&mut DockWindowState) -> R) -> Option<R> {
        self.docks.lock().get_mut(label).map(f)
    }

    /// The labels of the dock windows that exist, the primary's first, the
    /// rest sorted.
    fn labels(&self) -> Vec<String> {
        let mut labels: Vec<String> = self.docks.lock().keys().cloned().collect();
        labels.sort_by(|a, b| (a != DOCK_LABEL).cmp(&(b != DOCK_LABEL)).then(a.cmp(b)));
        labels
    }
}

/// `settings.theme` as the settings window's title bar last took it (or
/// will take it, while the window is closed), so a settings event only acts
/// on a change. Kept in Tauri's managed state.
#[derive(Debug, Default)]
pub struct SettingsWindowTheme(Mutex<Option<ThemeMode>>);

/// Whether each dock window's panel is slid out by auto-hide, as
/// its UI last reported through `set_dock_auto_hidden`, keyed by window
/// label. `false` for a window that never reported, and again when the
/// window is created. Kept in Tauri's managed state so [`toggle_dock`] can
/// tell "bring the panel back" from "hide the window".
#[derive(Debug, Default)]
pub struct DockAutoHidden(Mutex<HashMap<String, bool>>);

// ---------------------------------------------------------------------------
// dock windows: which ones (pure)
// ---------------------------------------------------------------------------

/// One dock window that should exist: its label and the monitor it sits on
/// (`None` only for the primary's window while no monitor is known).
#[derive(Debug, Clone, PartialEq)]
pub struct DockPlan {
    pub label: String,
    pub monitor: Option<MonitorInfo>,
}

/// The label of the dock window on monitor `id` other than the primary's:
/// `dock-` and the id with every character outside `[A-Za-z0-9_-]` dropped
/// (`\\.\DISPLAY3` → `dock-DISPLAY3`; Tauri labels allow `a-zA-Z0-9-/:_`).
/// Pure.
pub fn dock_label(id: &MonitorId) -> String {
    let mut label = String::with_capacity(DOCK_LABEL.len() + 1 + id.0.len());
    label.push_str(DOCK_LABEL);
    label.push('-');
    label.extend(
        id.0.chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-'),
    );
    label
}

/// `value` percent-encoded for a URL query (every byte outside the
/// unreserved set `A-Za-z0-9-_.~`), so `URLSearchParams.get('monitor')`
/// gives the page the monitor id back exactly (`\\.\DISPLAY3` →
/// `%5C%5C.%5CDISPLAY3`). Pure.
pub fn query_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(byte));
        } else {
            // Writing to a String cannot fail.
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

/// The page a dock window loads: the primary's has no query string, the
/// others carry their monitor id (`docs/ipc/windows.md`). Pure.
pub fn dock_page(label: &str, monitor: Option<&MonitorId>) -> String {
    match monitor {
        Some(id) if label != DOCK_LABEL => format!("dock.html?monitor={}", query_value(&id.0)),
        _ => "dock.html".to_owned(),
    }
}

/// Which dock windows should exist for `mode` over `monitors`: the
/// primary's window (`dock`) on [`primary_monitor`] always, even with no
/// monitor known; with `All`, one more per other monitor, labelled by
/// [`dock_label`]. A monitor whose label collides with an earlier one's is
/// left out (the caller logs it). Pure.
pub fn plan_docks(mode: MonitorMode, monitors: &[MonitorInfo]) -> Vec<DockPlan> {
    let primary = primary_monitor(monitors);
    let mut plan = vec![DockPlan {
        label: DOCK_LABEL.to_owned(),
        monitor: primary.clone(),
    }];
    if mode == MonitorMode::Primary {
        return plan;
    }
    let primary_id = primary.map(|m| m.id);
    for monitor in monitors {
        if primary_id.as_ref() == Some(&monitor.id) {
            continue;
        }
        let label = dock_label(&monitor.id);
        if plan.iter().any(|p| p.label == label) {
            continue;
        }
        plan.push(DockPlan {
            label,
            monitor: Some(monitor.clone()),
        });
    }
    plan
}

fn primary_monitor(monitors: &[MonitorInfo]) -> Option<MonitorInfo> {
    monitors
        .iter()
        .find(|m| m.is_primary)
        .or_else(|| monitors.first())
        .cloned()
}

/// `monitor` for a log line.
fn describe_monitor(monitor: Option<&MonitorInfo>) -> String {
    match monitor {
        Some(m) => format!(
            "{} ({}x{} at {},{}, scale {}{})",
            m.id.0,
            m.bounds.width,
            m.bounds.height,
            m.bounds.x,
            m.bounds.y,
            m.scale_factor,
            if m.is_primary { ", primary" } else { "" }
        ),
        None => "no known monitor (Tauri's view of the primary)".to_owned(),
    }
}

// ---------------------------------------------------------------------------
// dock windows: creation, closing, reconciliation
// ---------------------------------------------------------------------------

/// Create the dock window(s) for the current settings and monitor list, at
/// startup (inside `setup`, on the main thread). With no monitor reported
/// yet only the primary's window exists; the first `dock://monitors`
/// creates the others when the mode asks for them.
pub fn create_dock_windows(app: &AppHandle, service: &DockService) -> tauri::Result<()> {
    let settings = service.settings();
    let click_through = settings.behavior.click_through;
    if let Some(geometry) = app.try_state::<DockGeometry>() {
        *geometry.click_through.lock() = Some(click_through);
        *geometry.mode.lock() = Some(settings.monitors);
        *geometry.position.lock() = Some(settings.position);
    }
    log::info!(
        "click-through outside the panel: {}",
        describe_click_through(click_through)
    );
    let monitors = service.monitors();
    log::info!(
        "dock windows: {:?} on the {:?} edge over {} known monitor(s)",
        settings.monitors,
        settings.position,
        monitors.len()
    );
    reconcile_docks(app, service, settings.monitors, &monitors)
}

/// Make the set of dock windows match [`plan_docks`] for `mode` over
/// `monitors`: close the windows whose monitor is gone (never the
/// primary's), create the missing ones, and lay every remaining one out on
/// its own monitor (which the panel conversion then reports to the engine
/// with the monitor id). A secondary window that cannot be created is
/// logged and skipped; the primary's failure is returned. Main thread.
fn reconcile_docks(
    app: &AppHandle,
    service: &DockService,
    mode: MonitorMode,
    monitors: &[MonitorInfo],
) -> tauri::Result<()> {
    let plan = plan_docks(mode, monitors);
    if mode == MonitorMode::All {
        for monitor in monitors {
            if !plan.iter().any(|p| p.monitor.as_ref() == Some(monitor)) {
                log::warn!(
                    "monitor {} gets no dock: its label {:?} is already taken",
                    monitor.id.0,
                    dock_label(&monitor.id)
                );
            }
        }
    }

    let existing = app
        .try_state::<DockGeometry>()
        .map(|geometry| geometry.labels())
        .unwrap_or_default();
    for label in existing {
        if label != DOCK_LABEL && !plan.iter().any(|p| p.label == label) {
            close_dock_window(app, service, &label);
        }
    }

    for entry in plan {
        match app.get_webview_window(&entry.label) {
            Some(window) => {
                let monitor_id = entry.monitor.as_ref().map(|m| m.id.clone());
                let previous = app.try_state::<DockGeometry>().and_then(|geometry| {
                    geometry.update(&entry.label, |dock| {
                        std::mem::replace(&mut dock.monitor, monitor_id.clone())
                    })
                });
                if previous.as_ref() != Some(&monitor_id) {
                    log::info!(
                        "dock window {:?} now on {}",
                        entry.label,
                        describe_monitor(entry.monitor.as_ref())
                    );
                }
                if let Err(e) = relayout_dock(app, &window, monitors) {
                    log::warn!(
                        "re-positioning dock window {:?} after a monitor change failed: {e}",
                        entry.label
                    );
                }
            }
            None => {
                let result = create_dock_window(app, service, &entry.label, entry.monitor.as_ref());
                match result {
                    Ok(()) => {}
                    Err(e) if entry.label == DOCK_LABEL => return Err(e),
                    Err(e) => log::warn!(
                        "dock window {:?} for monitor {} could not be created: {e}",
                        entry.label,
                        entry.monitor.as_ref().map_or("?", |m| m.id.0.as_str())
                    ),
                }
            }
        }
    }
    Ok(())
}

/// Create one dock window on `monitor` and show it, unless the set is
/// hidden ([`show_new_dock_window`]). Main thread (the platform subclasses
/// the window right after creation, which is only allowed from the thread
/// that created it).
fn create_dock_window(
    app: &AppHandle,
    service: &DockService,
    label: &str,
    monitor: Option<&MonitorInfo>,
) -> tauri::Result<()> {
    let page = dock_page(label, monitor.map(|m| &m.id));
    let window = WebviewWindowBuilder::new(app, label, WebviewUrl::App(page.into()))
        .title("Stem")
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .shadow(false)
        .focused(false)
        // tao derives WS_EX_NOACTIVATE from this flag, so the bit survives
        // its own style rewrites (`show`, `hide`, `set_always_on_top`, …).
        .focusable(false)
        .visible(false)
        .inner_size(INITIAL_SIZE.0, INITIAL_SIZE.1)
        .build()?;

    // The dock is never closed by the user (Quit lives in the tray), a DPI
    // change lays it out again, and losing the focus may end the
    // focus shortcut's session.
    let handle = app.clone();
    let owned = label.to_owned();
    window.on_window_event(move |event| match event {
        WindowEvent::CloseRequested { api, .. } => {
            log::debug!("dock window {owned:?}: close request ignored");
            api.prevent_close();
        }
        WindowEvent::ScaleFactorChanged {
            scale_factor,
            new_inner_size,
            ..
        } => on_scale_factor_changed(&handle, &owned, *scale_factor, *new_inner_size),
        // The dock lost the keyboard focus; maybe the activation.
        WindowEvent::Focused(false) => on_dock_blurred(&handle, &owned),
        _ => {}
    });

    apply_native_styles(service, &window);
    if let Some(geometry) = app.try_state::<DockGeometry>() {
        geometry.docks.lock().insert(
            label.to_owned(),
            DockWindowState {
                monitor: monitor.map(|m| m.id.clone()),
                scale: window.scale_factor().ok(),
                ..DockWindowState::default()
            },
        );
    }
    // A fresh page starts with its panel shown.
    if let Some(flags) = app.try_state::<DockAutoHidden>() {
        flags.0.lock().insert(label.to_owned(), false);
    }
    // Not placed yet: the initial height becomes the thickness on any edge.
    layout(&window, monitor, dock_edge(app), None)?;
    let primary_visible = if label == DOCK_LABEL {
        None
    } else {
        primary_visibility(app)
    };
    if show_new_dock_window(label, primary_visible) {
        window.show()?;
        log::info!(
            "dock window created {label:?} on {}",
            describe_monitor(monitor)
        );
    } else {
        // Hidden, it reports no panel to the engine (`push_dock_panel`);
        // the next Show of the set (`toggle_dock`, which acts on every dock
        // window) shows it and hands its panel over.
        log::info!(
            "dock window created {label:?} on {}, hidden like the rest of the set",
            describe_monitor(monitor)
        );
    }
    Ok(())
}

/// Whether a dock window that was just created is shown at once.
/// The primary's window (`dock`) always is. Any other follows the set,
/// whose state is the primary's (as for [`toggle_dock`]): it is shown only
/// when `primary_visible` is `Some(true)`; a hidden primary means the toggle
/// hid the set, and a window shown then would be the one dock on screen,
/// turning the next press into a Show. `None` (the primary's state could
/// not be read) leaves it hidden too. Pure.
pub fn show_new_dock_window(label: &str, primary_visible: Option<bool>) -> bool {
    label == DOCK_LABEL || primary_visible == Some(true)
}

/// Whether the primary's dock window is visible; `None` when there is none,
/// or (after a warning) when its state cannot be read. Main thread.
fn primary_visibility(app: &AppHandle) -> Option<bool> {
    let primary = app.get_webview_window(DOCK_LABEL)?;
    match primary.is_visible() {
        Ok(visible) => Some(visible),
        Err(e) => {
            log::warn!("could not read whether the primary dock window is visible: {e}");
            None
        }
    }
}

/// Close the secondary dock window `label` (its monitor went away, or the
/// mode went back to the primary only): destroy it (a plain close would be
/// refused like a user's), forget its state, and tell the engine the dock
/// is gone. Main thread.
fn close_dock_window(app: &AppHandle, service: &DockService, label: &str) {
    let monitor = app
        .try_state::<DockGeometry>()
        .and_then(|geometry| geometry.docks.lock().remove(label))
        .and_then(|dock| dock.monitor);
    if let Some(flags) = app.try_state::<DockAutoHidden>() {
        flags.0.lock().remove(label);
    }
    forget_session_of(app, label, "it closed");
    crate::appbar::forget(app, label);
    match app.get_webview_window(label) {
        Some(window) => {
            if let Err(e) = window.destroy() {
                log::warn!("dock window {label:?} could not be closed: {e}");
            }
        }
        None => log::debug!("dock window {label:?} was already gone"),
    }
    service.set_dock_panel(label, None, None);
    log::info!(
        "dock window closed {label:?} (monitor {})",
        monitor.as_ref().map_or("unknown", |m| m.0.as_str())
    );
}

fn describe_click_through(enabled: bool) -> &'static str {
    if enabled {
        "on"
    } else {
        "off (the whole dock window takes clicks)"
    }
}

/// The OS handle of a Tauri window, for `Platform` calls. `None` where the
/// platform has no native handles (non-Windows hosts) or the lookup fails.
pub fn native_handle(window: &WebviewWindow) -> Option<NativeWindowHandle> {
    #[cfg(windows)]
    {
        match window.hwnd() {
            Ok(hwnd) => Some(NativeWindowHandle(hwnd.0 as isize)),
            Err(e) => {
                log::warn!("no HWND for {:?}: {e}", window.label());
                None
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = window;
        None
    }
}

fn apply_native_styles(service: &DockService, window: &WebviewWindow) {
    #[cfg(windows)]
    {
        match window.hwnd() {
            Ok(hwnd) => {
                let handle = NativeWindowHandle(hwnd.0 as isize);
                if let Err(e) = service
                    .platform()
                    .prepare_dock_window(handle, DockWindowOptions::default())
                {
                    log::warn!("prepare_dock_window failed: {e}");
                }
            }
            Err(e) => log::warn!("no HWND for dock window: {e}"),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (
            service,
            window,
            NativeWindowHandle(0),
            DockWindowOptions::default(),
        );
    }
}

/// The monitor the dock window `label` was placed on, as the state says
/// and the list `monitors` knows it; `None` before the first monitor list,
/// or while the monitor is gone from the list (a hot-plug the reconcile
/// has not caught up with yet).
fn monitor_of(app: &AppHandle, label: &str, monitors: &[MonitorInfo]) -> Option<MonitorInfo> {
    let id = app
        .try_state::<DockGeometry>()
        .and_then(|geometry| geometry.dock(label))
        .and_then(|dock| dock.monitor)?;
    monitors.iter().find(|m| m.id == id).cloned()
}

/// The monitor id the dock window `label` was placed on, for the engine.
fn monitor_id_of(app: &AppHandle, label: &str) -> Option<MonitorId> {
    app.try_state::<DockGeometry>()
        .and_then(|geometry| geometry.dock(label))
        .and_then(|dock| dock.monitor)
}

/// The dock windows that exist, the primary's first.
pub(crate) fn dock_windows(app: &AppHandle) -> Vec<WebviewWindow> {
    let labels = app
        .try_state::<DockGeometry>()
        .map(|geometry| geometry.labels())
        .unwrap_or_default();
    labels
        .iter()
        .filter_map(|label| app.get_webview_window(label))
        .collect()
}

// ---------------------------------------------------------------------------
// dock windows: the UI's reports
// ---------------------------------------------------------------------------

/// One `debug` line for a report the page measured for `edge` while the
/// window is on `placed`: dropped, the page reports for the new
/// edge next.
fn log_stale(label: &str, report: &str, edge: DockPosition, placed: DockPosition) {
    log::debug!(
        "dock window {label:?}: {report} measured for the {edge:?} edge dropped; the window is on the {placed:?} edge"
    );
}

/// Take the reported content size (CSS px), measured for `edge`: the value
/// across that edge becomes the window's thickness, and the window is laid
/// on the edge in effect of its own monitor's work area, as long as the
/// work area along it ([`layout`]). A report measured for another edge than
/// the window's is dropped ([`DockWindowState::take_content`]),
/// unless it names the edge in effect: then the window's move to that edge
/// failed or is still to come, and this layout is the move (logged). The
/// page's first report for a new edge always includes a content size (it
/// sends one after every position change), before its hit region and panel
/// rect, so those are taken once this layout placed the window. Called from
/// the `set_dock_content_size` command, on the main thread.
pub fn apply_content_size(
    app: &AppHandle,
    window: &WebviewWindow,
    width: f64,
    height: f64,
    edge: DockPosition,
) -> tauri::Result<()> {
    let (width, height) = clamp_content(edge, width, height);
    let content = DockContent {
        width,
        height,
        edge,
    };
    let in_effect = dock_edge(app);
    let taken = app.try_state::<DockGeometry>().and_then(|geometry| {
        geometry.update(window.label(), |dock| dock.take_content(content, in_effect))
    });
    match taken {
        Some(Ok(None)) => {}
        Some(Ok(Some(placed))) => log::info!(
            "dock window {:?}: a content size for the {edge:?} edge while the window is still on the {placed:?} edge; moving it there",
            window.label()
        ),
        Some(Err(placed)) => {
            log_stale(window.label(), "a content size", edge, placed);
            return Ok(());
        }
        None => log::warn!(
            "no state for dock window {:?}; monitor changes will not re-layout it",
            window.label()
        ),
    }
    let monitors = match app.try_state::<crate::AppService>() {
        Some(service) => service.0.monitors(),
        None => Vec::new(),
    };
    let monitor = monitor_of(app, window.label(), &monitors);
    layout(window, monitor.as_ref(), in_effect, Some(content))
}

/// Argument check for `set_dock_hit_region` (`docs/ipc/hit-region.md`):
/// non-negative sizes, and the envelope contains the panel. Pure.
pub fn validate_hit_region(panel: Rect, envelope: Rect) -> Result<(), String> {
    for (name, rect) in [("panel", panel), ("envelope", envelope)] {
        if rect.width < 0 || rect.height < 0 {
            return Err(format!("{name} has a negative size: {rect:?}"));
        }
    }
    let contains = envelope.x <= panel.x
        && envelope.y <= panel.y
        && envelope.right() >= panel.right()
        && envelope.bottom() >= panel.bottom();
    if !contains {
        return Err(format!(
            "envelope {envelope:?} does not contain panel {panel:?}"
        ));
    }
    Ok(())
}

/// Remember the UI's hit region (CSS px, window-local), measured for
/// `edge`, and, while `behavior.clickThrough` is on, hand it to the
/// platform in physical pixels. While the setting is off the window stays
/// fully clickable and the report waits for the setting to be turned on
/// ([`on_settings_changed`]). A report measured for another edge than the
/// window's is dropped ([`DockWindowState::take_hit_region`]): the
/// platform keeps the region it has until the page's report for the new
/// edge. A platform without the feature (mock, non-Windows hosts) leaves
/// the window fully clickable; that is logged once at debug and is not an
/// error for the UI. Runs on the main thread.
pub fn apply_hit_region(
    app: &AppHandle,
    window: &WebviewWindow,
    panel: Rect,
    envelope: Rect,
    edge: DockPosition,
) -> Result<(), String> {
    let service = app
        .try_state::<crate::AppService>()
        .ok_or_else(|| "service not available".to_owned())?;
    if let Some(geometry) = app.try_state::<DockGeometry>() {
        let taken = geometry.update(window.label(), |dock| {
            dock.take_hit_region(panel, envelope, edge)
        });
        if let Some(Err(placed)) = taken {
            log_stale(window.label(), "a hit region", edge, placed);
            return Ok(());
        }
    }
    if !service.0.settings().behavior.click_through {
        log::trace!(
            "hit region of {:?} remembered; click-through is off",
            window.label()
        );
        return Ok(());
    }
    push_hit_region(&service.0, window, panel, envelope)
}

/// The platform call behind [`apply_hit_region`]: CSS px → physical px by
/// the window's scale factor, then `set_dock_hit_region`. Main thread (the
/// scale factor is a window getter).
fn push_hit_region(
    service: &DockService,
    window: &WebviewWindow,
    panel: Rect,
    envelope: Rect,
) -> Result<(), String> {
    let Some(handle) = native_handle(window) else {
        log::debug!("hit region ignored: no native window handle on this host");
        return Ok(());
    };
    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    let region = HitRegion {
        panel: scale_rect(panel, scale),
        envelope: scale_rect(envelope, scale),
    };
    match service.platform().set_dock_hit_region(handle, Some(region)) {
        Ok(()) => {
            log::trace!("hit region of {:?}: {region:?}", window.label());
            Ok(())
        }
        Err(PlatformError::Unsupported(task)) => {
            static LOGGED: std::sync::Once = std::sync::Once::new();
            LOGGED.call_once(|| {
                log::debug!("hit regions are not supported by this platform (feature: {task})");
            });
            Ok(())
        }
        Err(e) => Err(e.to_string()),
    }
}

/// Argument check for `set_dock_panel_rect` (`docs/ipc/dock-panel.md`):
/// non-negative sizes. Pure.
pub fn validate_panel_rect(rect: Rect) -> Result<(), String> {
    if rect.width < 0 || rect.height < 0 {
        return Err(format!("the panel has a negative size: {rect:?}"));
    }
    Ok(())
}

/// Remember the UI's resting panel (CSS px of the viewport it carries,
/// measured for the edge it names) and hand it to the service in physical
/// screen pixels, with the window's label and monitor,
/// re-anchored to the window's viewport of the moment
/// ([`push_dock_panel`]); no rect goes instead while the window is hidden. A
/// report measured for another edge than the window's is dropped
/// ([`DockWindowState::take_panel`]). The panel's distance to its
/// viewport's edge side is the band *Reserve screen space* asks for
/// (`appbar::sync`), read against the report's own viewport, so a report
/// handled after the resize the page sent its content size for (the same
/// tick, applied first) says what it said when the page measured it. Runs
/// on the main thread.
pub fn apply_dock_panel(
    app: &AppHandle,
    window: &WebviewWindow,
    report: PanelReport,
) -> Result<(), String> {
    let service = app
        .try_state::<crate::AppService>()
        .ok_or_else(|| "service not available".to_owned())?;
    if let Some(geometry) = app.try_state::<DockGeometry>() {
        let taken = geometry.update(window.label(), |dock| {
            (dock.take_panel(report), dock.reserve_css)
        });
        match taken {
            Some((Err(placed), _)) => {
                log_stale(window.label(), "a panel rect", report.edge, placed);
                return Ok(());
            }
            Some((Ok(None), reserve)) => log::debug!(
                "dock window {:?}: the panel {:?} does not lie inside the {:?} css viewport it was measured in across the {:?} edge; the band keeps {reserve:?} css",
                window.label(),
                report.rect,
                report.viewport,
                report.edge
            ),
            Some((Ok(Some(_)), _)) | None => {}
        }
    }
    let pushed = push_dock_panel(app, &service.0, window, report, None);
    crate::appbar::sync(app, window);
    pushed
}

/// The window's viewport in CSS px now (its client size over its scale
/// factor, not rounded), the space the page lays itself out in; `None`
/// (logged at debug) when the window cannot say.
fn viewport_now(window: &WebviewWindow) -> Option<CssViewport> {
    let read = || -> tauri::Result<CssViewport> {
        let size = window.inner_size()?;
        let scale = window.scale_factor()?;
        Ok(CssViewport::of((size.width, size.height), scale))
    };
    match read() {
        Ok(viewport) => Some(viewport),
        Err(e) => {
            log::debug!(
                "dock window {:?}: its viewport is unknown ({e}); its panel rect is read as reported",
                window.label()
            );
            None
        }
    }
}

/// Convert the page's panel `report` for the engine and the backdrop: on a
/// hidden window no rect; else the rect re-anchored to the window's
/// viewport now ([`PanelReport::now`]), CSS px → physical screen px
/// from the window's top-left corner (`origin` when the caller just placed
/// it there, else read from the window) and its scale factor
/// ([`panel_on_screen`]), then `DockService::set_dock_panel` for the
/// window's label and monitor and `backdrop::on_panel`. Main thread (the
/// getters are window calls).
fn push_dock_panel(
    app: &AppHandle,
    service: &DockService,
    window: &WebviewWindow,
    report: PanelReport,
    origin: Option<(i32, i32)>,
) -> Result<(), String> {
    let label = window.label();
    let monitor = monitor_id_of(app, label);
    if !window.is_visible().map_err(|e| e.to_string())? {
        service.set_dock_panel(label, monitor, None);
        crate::backdrop::on_panel(app, window, None);
        return Ok(());
    }
    let origin = match origin {
        Some(origin) => origin,
        None => {
            let position = window.outer_position().map_err(|e| e.to_string())?;
            (position.x, position.y)
        }
    };
    let scale = window.scale_factor().map_err(|e| e.to_string())?;
    let viewport = viewport_now(window);
    let css = report.now(viewport);
    let panel = panel_on_screen(origin, css, scale);
    if css == report.rect {
        log::trace!(
            "dock panel of {label:?}: {css:?} css at {scale}x from {origin:?} -> {panel:?} on {monitor:?}"
        );
    } else {
        log::trace!(
            "dock panel of {label:?}: {:?} css measured in {:?} on the {:?} edge, {css:?} in {viewport:?}, at {scale}x from {origin:?} -> {panel:?} on {monitor:?}",
            report.rect,
            report.viewport,
            report.edge
        );
    }
    service.set_dock_panel(label, monitor, Some(panel));
    crate::backdrop::on_panel(
        app,
        window,
        Some(crate::backdrop::PanelConversion {
            origin,
            css,
            scale,
            viewport,
        }),
    );
    Ok(())
}

/// The window was moved or resized by this module (`place`), or shown
/// again: convert the UI's latest panel report for the window's new place
/// and viewport ([`push_dock_panel`]). `origin` is the window's top-left
/// corner in physical pixels when known (just placed), else it is read from
/// the window. Main thread; silent when the UI has reported nothing yet.
fn sync_dock_panel(window: &WebviewWindow, origin: Option<(i32, i32)>) {
    let app = window.app_handle();
    let (Some(service), Some(geometry)) = (
        app.try_state::<crate::AppService>(),
        app.try_state::<DockGeometry>(),
    ) else {
        return;
    };
    let Some(report) = geometry.dock(window.label()).and_then(|dock| dock.panel) else {
        return;
    };
    if let Err(e) = push_dock_panel(app, &service.0, window, report, origin) {
        log::debug!(
            "the dock panel of {:?} could not be converted after a move: {e}",
            window.label()
        );
    }
}

/// The resting panel in physical pixels of the virtual desktop: `css`
/// scaled like a hit region (`scale_rect`, rounded outwards) and offset by
/// the window's top-left corner `origin` (physical px). Pure.
pub fn panel_on_screen(origin: (i32, i32), css: Rect, scale: f64) -> Rect {
    let scaled = scale_rect(css, scale);
    Rect::new(
        origin.0.saturating_add(scaled.x),
        origin.1.saturating_add(scaled.y),
        scaled.width,
        scaled.height,
    )
}

// ---------------------------------------------------------------------------
// settings and monitor changes
// ---------------------------------------------------------------------------

/// The settings changed (settings window, tray, a command): apply
/// `behavior.clickThrough`, `theme`, `position`
/// and `monitors` where they differ from what is in effect. Runs on
/// the service worker thread; the window work hops to the main thread,
/// where the window getters are direct calls. The edge is taken before the
/// monitor mode, so windows a mode change creates already go to the new
/// edge.
pub fn on_settings_changed(app: &AppHandle, settings: &Settings) {
    on_click_through_setting(app, settings.behavior.click_through);
    on_theme_setting(app, settings.theme);
    on_position_setting(app, settings.position);
    on_monitor_mode_setting(app, settings.monitors);
}

/// `settings.position` changed (with `monitors`, one of the two
/// changes `Settings::diff_requires_window_rebuild` names): move every dock
/// window to the new edge of its own monitor's work area
/// (`docs/ipc/sizing-protocol.md`, step 5). The windows are moved in place,
/// not created anew: no window attribute depends on the edge (only its rect
/// does), a new window would reload its page for nothing (a blank frame,
/// every icon fetched again, the hover and auto-hide state lost), and a
/// monitor change already re-places windows the same way
/// ([`relayout_dock`]). Each window keeps its thickness, now across the new
/// edge, until its page reports a content size for that edge
/// ([`DockContent::on_edge`]); what its page laid out for the old edge is
/// dropped first ([`forget_edge_layout`]), and the primary's backdrop and
/// every band go with it (`backdrop::on_panel`, `appbar::sync`) until the
/// page reports its panel on the new edge. The pages get `dock://settings`
/// before this runs (`lib.rs`, `forward_events`), so a page usually lays
/// itself out for the new edge before its window moves. Any thread; the
/// window work hops to the main thread.
fn on_position_setting(app: &AppHandle, edge: DockPosition) {
    let Some(geometry) = app.try_state::<DockGeometry>() else {
        return;
    };
    let previous = {
        let mut current = geometry.position.lock();
        if *current == Some(edge) {
            return;
        }
        current.replace(edge)
    };
    let handle = app.clone();
    let scheduled = app.run_on_main_thread(move || {
        let Some(service) = handle.try_state::<crate::AppService>() else {
            return;
        };
        let windows = dock_windows(&handle);
        log::info!(
            "dock windows: moving {} window(s) from the {} edge to the {edge:?} edge",
            windows.len(),
            previous.map_or_else(|| "unknown".to_owned(), |p| format!("{p:?}"))
        );
        let monitors = service.0.monitors();
        for window in windows {
            forget_edge_layout(&handle, &service.0, window.label());
            // The backdrop and the band on the old edge go now,
            // before the window moves: no material without its panel, and
            // the move converts no panel rect (there is none until the page
            // reports one), so nothing else would take the material away.
            // Both come back with the page's first panel report there.
            crate::backdrop::on_panel(&handle, &window, None);
            crate::appbar::sync(&handle, &window);
            if let Err(e) = relayout_dock(&handle, &window, &monitors) {
                log::warn!(
                    "dock window {:?} could not be moved to the {edge:?} edge: {e}",
                    window.label()
                );
            }
        }
    });
    if let Err(e) = scheduled {
        log::warn!("could not schedule the move to the {edge:?} edge: {e}");
    }
}

/// Before the dock window `label` moves to another edge: forget the
/// panel rect and the hit region its page laid out for the old edge, and
/// tell the engine the dock has no panel until the page reports one for
/// the new edge (as while the window is hidden). Converted again at the
/// window's new origin, the old rect would lie where no panel is (a bottom
/// panel 700 px long running from a left window 135 px wide into the
/// desktop), and the engine would compute the dock's `overlapped` from it:
/// with *When a window overlaps* the dock could slide out and back
/// for nothing. The platform keeps the hit region it has (window-local,
/// where the page last drew its panel) until the page's next report; it is
/// not replayed meanwhile (a DPI change, `behavior.clickThrough` turned
/// on). The content size is kept: its thickness holds until the next
/// report (`docs/ipc/sizing-protocol.md`, step 5). A report the page
/// measured for the old edge that is handled after this is dropped
/// ([`DockWindowState::stale_for`]): the window is placed on the new edge
/// in the same turn of the main thread. Main thread.
fn forget_edge_layout(app: &AppHandle, service: &DockService, label: &str) {
    let Some(geometry) = app.try_state::<DockGeometry>() else {
        return;
    };
    let forgotten = geometry.update(label, |dock| {
        dock.hit_region_css = None;
        dock.reserve_css = None;
        dock.panel.take().map(|_| dock.monitor.clone())
    });
    if let Some(Some(monitor)) = forgotten {
        log::debug!(
            "dock window {label:?}: panel rect withdrawn until its page reports for the new edge"
        );
        service.set_dock_panel(label, monitor, None);
    }
}

fn on_click_through_setting(app: &AppHandle, enabled: bool) {
    let Some(geometry) = app.try_state::<DockGeometry>() else {
        return;
    };
    {
        let mut current = geometry.click_through.lock();
        if *current == Some(enabled) {
            return;
        }
        *current = Some(enabled);
    }
    let handle = app.clone();
    if let Err(e) = app.run_on_main_thread(move || apply_click_through(&handle, enabled)) {
        log::warn!("could not schedule the click-through change: {e}");
    }
}

/// Turn click-through on (replay each UI's latest hit region) or off (the
/// whole dock windows take clicks again), for every dock window. Main
/// thread.
fn apply_click_through(app: &AppHandle, enabled: bool) {
    let Some(service) = app.try_state::<crate::AppService>() else {
        return;
    };
    let windows = dock_windows(app);
    if windows.is_empty() {
        log::debug!("click-through changed before any dock window exists");
        return;
    }
    if enabled {
        let mut waiting = 0;
        for window in &windows {
            let region = app
                .try_state::<DockGeometry>()
                .and_then(|geometry| geometry.dock(window.label()))
                .and_then(|dock| dock.hit_region_css);
            match region {
                Some((panel, envelope)) => {
                    if let Err(e) = push_hit_region(&service.0, window, panel, envelope) {
                        log::warn!(
                            "click-through could not be turned on for {:?}: {e}",
                            window.label()
                        );
                    }
                }
                None => waiting += 1,
            }
        }
        if waiting > 0 {
            log::info!(
                "click-through outside the panel: on (waiting for the hit region of {waiting} dock window(s))"
            );
        } else {
            log::info!(
                "click-through outside the panel: {}",
                describe_click_through(true)
            );
        }
        return;
    }
    for window in &windows {
        let Some(handle) = native_handle(window) else {
            continue;
        };
        if let Err(e) = service.0.platform().set_dock_hit_region(handle, None)
            && !matches!(e, PlatformError::Unsupported(_))
        {
            log::warn!(
                "click-through could not be turned off for {:?}: {e}",
                window.label()
            );
        }
    }
    log::info!(
        "click-through outside the panel: {}",
        describe_click_through(false)
    );
}

/// `settings.theme` changed: the settings window's title bar follows it (the
/// page itself follows `dock://settings`). Any thread; never blocks.
fn on_theme_setting(app: &AppHandle, mode: ThemeMode) {
    let Some(state) = app.try_state::<SettingsWindowTheme>() else {
        return;
    };
    {
        let mut current = state.0.lock();
        if *current == Some(mode) {
            return;
        }
        *current = Some(mode);
    }
    let handle = app.clone();
    if let Err(e) = app.run_on_main_thread(move || apply_settings_window_theme(&handle, mode)) {
        log::warn!("could not schedule the settings window's theme change: {e}");
    }
}

/// Give the open settings window's title bar the theme for `mode`. A closed
/// window takes the setting when it is created. Main thread.
fn apply_settings_window_theme(app: &AppHandle, mode: ThemeMode) {
    let Some(window) = app.get_webview_window(SETTINGS_LABEL) else {
        log::debug!("theme setting {mode:?}: the settings window is not open");
        return;
    };
    match window.set_theme(native_theme(mode)) {
        Ok(()) => log::debug!("settings window title bar: {mode:?}"),
        Err(e) => log::warn!("the settings window's theme could not be set: {e}"),
    }
}

/// `settings.theme` → the native window theme: forced light or dark, or
/// `None` to follow Windows. Pure.
pub fn native_theme(mode: ThemeMode) -> Option<tauri::Theme> {
    match mode {
        ThemeMode::System => None,
        ThemeMode::Light => Some(tauri::Theme::Light),
        ThemeMode::Dark => Some(tauri::Theme::Dark),
    }
}

/// `settings.monitors` changed: create the secondary dock windows
/// (`All`) or close them (`Primary`); the primary's window is never touched.
/// Any thread; the window work hops to the main thread.
fn on_monitor_mode_setting(app: &AppHandle, mode: MonitorMode) {
    let Some(geometry) = app.try_state::<DockGeometry>() else {
        return;
    };
    {
        let mut current = geometry.mode.lock();
        if *current == Some(mode) {
            return;
        }
        *current = Some(mode);
    }
    let handle = app.clone();
    let scheduled = app.run_on_main_thread(move || {
        let Some(service) = handle.try_state::<crate::AppService>() else {
            return;
        };
        let monitors = service.0.monitors();
        log::info!(
            "dock windows: {mode:?} over {} known monitor(s)",
            monitors.len()
        );
        if let Err(e) = reconcile_docks(&handle, &service.0, mode, &monitors) {
            log::warn!("the dock windows could not be rebuilt for {mode:?}: {e}");
        }
    });
    if let Err(e) = scheduled {
        log::warn!("could not schedule the dock window rebuild: {e}");
    }
}

/// CSS px → physical px, rounded outwards (left/top down, right/bottom up)
/// so a region never loses an edge pixel to rounding. Pure.
pub fn scale_rect(rect: Rect, scale: f64) -> Rect {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    let left = (f64::from(rect.x) * scale).floor();
    let top = (f64::from(rect.y) * scale).floor();
    let right = (f64::from(rect.right()) * scale).ceil();
    let bottom = (f64::from(rect.bottom()) * scale).ceil();
    let clamp = |v: f64| v.clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32;
    Rect::new(
        clamp(left),
        clamp(top),
        clamp(right - left),
        clamp(bottom - top),
    )
}

/// The service reported a new monitor list (startup, hot-plug, resolution
/// or taskbar change, a new primary): reconcile the dock windows with it
/// and lay each out on its monitor. Runs on the service worker
/// thread; the window work hops to the main thread so the blocking getters
/// run there directly.
pub fn on_monitors_changed(app: &AppHandle, monitors: Vec<MonitorInfo>) {
    let handle = app.clone();
    let scheduled = app.run_on_main_thread(move || {
        let Some(service) = handle.try_state::<crate::AppService>() else {
            return;
        };
        if handle.get_webview_window(DOCK_LABEL).is_none() {
            log::debug!("monitors changed before the dock window exists");
            return;
        }
        let mode = service.0.settings().monitors;
        if let Err(e) = reconcile_docks(&handle, &service.0, mode, &monitors) {
            log::warn!("re-positioning the docks after a monitor change failed: {e}");
        }
    });
    if let Err(e) = scheduled {
        log::warn!("could not schedule the dock re-layout: {e}");
    }
}

/// A dock window's scale factor changed (`WindowEvent::ScaleFactorChanged`):
/// the display scale was changed, or the window landed on a monitor
/// with another DPI. Runs on the main thread, inside tao's handling of
/// `WM_DPICHANGED`, which resizes the window to keep its logical size once
/// this returns; the rest is therefore posted for the next turn of the event
/// loop ([`post_to_main_thread`]): the window's physical rect from the
/// remembered content size ([`relayout_dock`], which also converts the
/// remembered panel rect), then the remembered hit region at the new scale
/// ([`replay_hit_region`]). The UI watches the ratio in the browser on its
/// own, re-requests its icons and reports its geometry again; those reports
/// arrive after this and redo the same conversions with the same numbers.
fn on_scale_factor_changed(
    app: &AppHandle,
    label: &str,
    scale: f64,
    new_inner_size: PhysicalSize<u32>,
) {
    let previous = app
        .try_state::<DockGeometry>()
        .and_then(|geometry| geometry.update(label, |dock| dock.scale.replace(scale)))
        .flatten();
    log::info!(
        "dock window {label:?}: scale factor {} -> {scale} (inner size now {}x{} px); laying it out again",
        previous.map_or_else(|| "?".to_owned(), |s| s.to_string()),
        new_inner_size.width,
        new_inner_size.height
    );
    let handle = app.clone();
    let label = label.to_owned();
    post_to_main_thread(app, move || {
        let Some(window) = handle.get_webview_window(&label) else {
            log::debug!("dock window {label:?} is gone before its scale change was applied");
            return;
        };
        let monitors = handle
            .try_state::<crate::AppService>()
            .map_or_else(Vec::new, |service| service.0.monitors());
        if let Err(e) = relayout_dock(&handle, &window, &monitors) {
            log::warn!("laying out dock window {label:?} again after a scale change failed: {e}");
        }
        if let Err(e) = replay_hit_region(&handle, &window) {
            log::warn!(
                "the hit region of {label:?} could not be converted after a scale change: {e}"
            );
        }
    });
}

/// Run `f` on the main thread at the next turn of the event loop.
/// `AppHandle::run_on_main_thread` runs its task at once when it is already
/// on the main thread (tauri-runtime-wry's `send_user_message`), which is
/// too early from inside a window event callback: tao is still handling the
/// message that raised the event and sizes the window itself afterwards. A
/// hop through the async runtime turns the task into a posted message.
fn post_to_main_thread(app: &AppHandle, f: impl FnOnce() + Send + 'static) {
    let handle = app.clone();
    drop(tauri::async_runtime::spawn(async move {
        if let Err(e) = handle.run_on_main_thread(f) {
            log::warn!("could not schedule work on the main thread: {e}");
        }
    }));
}

/// The shell granted the dock window `label` a new band
/// (`appbar`): lay the window out again at the next turn of the main
/// thread's loop, which puts it on the band ([`layout`]). Any thread.
pub(crate) fn relayout_later(app: &AppHandle, label: &str) {
    let handle = app.clone();
    let label = label.to_owned();
    post_to_main_thread(app, move || {
        let Some(window) = handle.get_webview_window(&label) else {
            log::debug!("dock window {label:?} is gone before it could go onto its band");
            return;
        };
        let monitors = handle
            .try_state::<crate::AppService>()
            .map_or_else(Vec::new, |service| service.0.monitors());
        if let Err(e) = relayout_dock(&handle, &window, &monitors) {
            log::warn!("dock window {label:?} could not be placed on its band: {e}");
        }
    });
}

/// What decides the band the dock window asks for
/// (`appbar::request_for`): whether it is shown, the bounds of the monitor
/// and the edge it was placed on, the edge in effect, the panel's distance
/// to its edge side from the page's latest report, and its scale. Main
/// thread (the getters are window calls).
pub(crate) fn app_bar_inputs(
    app: &AppHandle,
    window: &WebviewWindow,
    enabled: bool,
) -> crate::appbar::BarInputs {
    let dock = app
        .try_state::<DockGeometry>()
        .and_then(|geometry| geometry.dock(window.label()))
        .unwrap_or_default();
    let monitor = dock.monitor.as_ref().and_then(|id| {
        app.try_state::<crate::AppService>().and_then(|service| {
            service
                .0
                .monitors()
                .into_iter()
                .find(|m| &m.id == id)
                .map(|m| m.bounds)
        })
    });
    crate::appbar::BarInputs {
        enabled,
        visible: window.is_visible().unwrap_or(false),
        monitor,
        placed: dock.edge,
        edge: dock_edge(app),
        reserve_css: dock.reserve_css,
        scale: window.scale_factor().unwrap_or(1.0),
    }
}

/// Size and place a dock window on the edge in effect ([`dock_edge`]) of its
/// own monitor's work area from what is remembered for it: [`layout`] with
/// its UI's latest content size, or none before the first report. The one
/// path behind a monitor or work-area change, a DPI change
/// and an edge change. Main thread.
fn relayout_dock(
    app: &AppHandle,
    window: &WebviewWindow,
    monitors: &[MonitorInfo],
) -> tauri::Result<()> {
    let content = app
        .try_state::<DockGeometry>()
        .and_then(|geometry| geometry.dock(window.label()))
        .and_then(|dock| dock.content);
    let monitor = monitor_of(app, window.label(), monitors);
    layout(window, monitor.as_ref(), dock_edge(app), content)
}

/// Hand a dock window's latest hit region to the platform again, converted
/// at the window's current scale factor (a DPI change), while
/// `behavior.clickThrough` is on. Nothing to do before the UI's first report
/// or while the setting is off. Main thread.
fn replay_hit_region(app: &AppHandle, window: &WebviewWindow) -> Result<(), String> {
    let (Some(service), Some(geometry)) = (
        app.try_state::<crate::AppService>(),
        app.try_state::<DockGeometry>(),
    ) else {
        return Ok(());
    };
    if !service.0.settings().behavior.click_through {
        return Ok(());
    }
    let region = geometry
        .dock(window.label())
        .and_then(|dock| dock.hit_region_css);
    match region {
        Some((panel, envelope)) => push_hit_region(&service.0, window, panel, envelope),
        None => Ok(()),
    }
}

/// The screen edge the dock windows are laid out on: `settings.position` as
/// this module last took it ([`create_dock_windows`], then
/// [`on_position_setting`]), else the service's, else the bottom.
fn dock_edge(app: &AppHandle) -> DockPosition {
    app.try_state::<DockGeometry>()
        .and_then(|geometry| *geometry.position.lock())
        .or_else(|| {
            app.try_state::<crate::AppService>()
                .map(|service| service.0.settings().position)
        })
        .unwrap_or(DockPosition::Bottom)
}

/// Size and place the window on `edge` of `monitor`'s work area (Tauri's
/// own view of the window's monitor while none is known): the rect
/// [`relayout_rect`] gives from `content`, the UI's latest content size, or
/// before its first report (`None`) from the window's current size and the
/// edge it was placed on last. While the shell has reserved a band for the
/// window on that edge of that monitor (`appbar::granted_band`),
/// the window keeps that rect's thickness but goes with its edge side on
/// the band's edge side, as long as the band
/// (`stem_platform::rules::appbar::window_rect`). Every path that sizes or
/// moves a dock window comes here (creation, the content size,
/// [`relayout_dock`]); [`place`] applies the rect.
fn layout(
    window: &WebviewWindow,
    monitor: Option<&MonitorInfo>,
    edge: DockPosition,
    content: Option<DockContent>,
) -> tauri::Result<()> {
    let work_area = match monitor {
        Some(m) => m.work_area,
        None => fallback_work_area(window)?,
    };
    let scale = window_scale(window, monitor);
    let size: PhysicalSize<u32> = window.outer_size()?;
    let size = (size.width, size.height);
    let placed = window
        .app_handle()
        .try_state::<DockGeometry>()
        .and_then(|geometry| geometry.dock(window.label()))
        .and_then(|dock| dock.edge);
    let rect = relayout_rect(
        work_area,
        monitor.is_some(),
        edge,
        content,
        scale,
        size,
        placed,
    );
    let band = monitor
        .and_then(|m| crate::appbar::granted_band(window.app_handle(), window.label(), edge, m));
    let rect = match band {
        Some(band) => {
            let on_band = window_rect(band, edge, across(rect, edge));
            log::debug!(
                "dock window {:?}: {rect:?} moved onto the band {band:?} the shell reserved -> {on_band:?}",
                window.label()
            );
            on_band
        }
        None => rect,
    };
    match content {
        Some(content) => {
            let (width, height) = content.on_edge(edge);
            log::debug!(
                "dock window {:?}: {width}x{height} css at {scale}x -> {rect:?} in {work_area:?} (the {edge:?} edge, full length)",
                window.label()
            );
        }
        None => log::debug!(
            "dock window {:?}: no content size yet, {} px across the edge kept at {scale}x -> {rect:?} in {work_area:?} (the {edge:?} edge, full length)",
            window.label(),
            thickness_of(size, placed)
        ),
    }
    place(window, edge, rect)
}

/// The window's scale factor, or its monitor's (1 without one) when the
/// window cannot give a usable one (logged).
fn window_scale(window: &WebviewWindow, monitor: Option<&MonitorInfo>) -> f64 {
    match window.scale_factor() {
        Ok(s) if s.is_finite() && s > 0.0 => s,
        Ok(s) => {
            log::warn!("window scale factor {s} is unusable; using the monitor's");
            monitor.map_or(1.0, |m| m.scale_factor)
        }
        Err(e) => {
            log::warn!("window scale factor unavailable ({e}); using the monitor's");
            monitor.map_or(1.0, |m| m.scale_factor)
        }
    }
}

/// Size and position a dock window at `rect` on `edge`, through the window
/// calls of [`placement_steps`] (never through a rect outside both its old
/// and its new place, so a move between edges does not pass over a
/// neighbouring monitor and change the window's DPI on the way); remember
/// the edge for its next placement as soon as the window has its new size
/// (the size alone says which side is the thickness, [`thickness_of`], so
/// a move that fails after the resize still leaves the right pair); then
/// redo the panel conversion for its new place and its new
/// viewport (the page's latest rect re-anchored to it, so a
/// thickness change or a drag's growth needs no new report from the page)
/// and let `appbar` follow (a new monitor or scale changes the
/// band). Every
/// placement of a dock window ends here.
fn place(window: &WebviewWindow, edge: DockPosition, rect: Rect) -> tauri::Result<()> {
    let current = current_rect(window);
    let target = (rect.width, rect.height);
    let mut size = current.map(|r| (r.width, r.height));
    let mut remembered = false;
    for step in placement_steps(current, rect) {
        if !remembered && size == Some(target) {
            remember_edge(window, edge);
            remembered = true;
        }
        match step {
            PlaceStep::Size { width, height } => {
                window.set_size(PhysicalSize::new(
                    width.unsigned_abs(),
                    height.unsigned_abs(),
                ))?;
                size = Some((width, height));
            }
            PlaceStep::Move { x, y } => window.set_position(PhysicalPosition::new(x, y))?,
        }
    }
    if !remembered {
        remember_edge(window, edge);
    }
    sync_dock_panel(window, Some((rect.x, rect.y)));
    crate::appbar::sync(window.app_handle(), window);
    Ok(())
}

/// Remember `edge` as the one the dock window was placed on ([`place`]).
fn remember_edge(window: &WebviewWindow, edge: DockPosition) {
    if let Some(geometry) = window.app_handle().try_state::<DockGeometry>() {
        geometry.update(window.label(), |dock| dock.edge = Some(edge));
    }
}

/// The window's rect on screen now, in physical pixels; `None` (logged at
/// debug) when the window cannot say, and [`placement_steps`] then resizes
/// before it moves.
fn current_rect(window: &WebviewWindow) -> Option<Rect> {
    let read = || -> tauri::Result<Rect> {
        let position = window.outer_position()?;
        let size = window.outer_size()?;
        Ok(Rect::new(
            position.x,
            position.y,
            i32::try_from(size.width).unwrap_or(i32::MAX),
            i32::try_from(size.height).unwrap_or(i32::MAX),
        ))
    };
    match read() {
        Ok(rect) => Some(rect),
        Err(e) => {
            log::debug!(
                "dock window {:?}: its rect could not be read before a placement ({e})",
                window.label()
            );
            None
        }
    }
}

/// Before the platform reported monitors, fall back to Tauri's own view: the
/// window's monitor, else the primary one (only queried when needed, so its
/// failure cannot mask a successful first answer).
fn fallback_work_area(window: &WebviewWindow) -> tauri::Result<Rect> {
    let monitor = match window.current_monitor()? {
        Some(current) => Some(current),
        None => window.primary_monitor()?,
    };
    Ok(match monitor {
        Some(m) => {
            let pos = m.work_area().position;
            let size = m.work_area().size;
            Rect::new(pos.x, pos.y, size.width as i32, size.height as i32)
        }
        None => Rect::new(0, 0, 1920, 1040),
    })
}

// ---------------------------------------------------------------------------
// dock windows: the placement rule (pure; `docs/ipc/sizing-protocol.md`)
// ---------------------------------------------------------------------------

/// Whether a dock on `edge` is vertical (`left`, `right`: its length runs
/// down the screen) rather than horizontal (`bottom`, `top`). Pure.
pub fn is_vertical(edge: DockPosition) -> bool {
    matches!(edge, DockPosition::Left | DockPosition::Right)
}

/// The dock window's rectangle on `edge` of `work_area`, in physical
/// pixels: flush with that edge, spanning the work area's full length along
/// it (its width on `bottom` and `top`, its height on `left` and `right`),
/// and `thickness` pixels across it, clamped to the work area and never
/// empty (a degenerate work area still gives a 1×1 window flush with its
/// edge). The placement rule every dock window follows, unit-tested below.
pub fn edge_rect(work_area: Rect, edge: DockPosition, thickness: i32) -> Rect {
    let width = work_area.width.max(1);
    let height = work_area.height.max(1);
    match edge {
        DockPosition::Bottom => {
            let thickness = thickness.clamp(1, height);
            Rect::new(
                work_area.x,
                work_area.bottom() - thickness,
                width,
                thickness,
            )
        }
        DockPosition::Top => {
            let thickness = thickness.clamp(1, height);
            Rect::new(work_area.x, work_area.y, width, thickness)
        }
        DockPosition::Left => {
            let thickness = thickness.clamp(1, width);
            Rect::new(work_area.x, work_area.y, thickness, height)
        }
        DockPosition::Right => {
            let thickness = thickness.clamp(1, width);
            Rect::new(
                work_area.right() - thickness,
                work_area.y,
                thickness,
                height,
            )
        }
    }
}

/// Physical window rectangle for a content size in CSS px on `edge`: the
/// thickness is `css_height` on a horizontal dock and `css_width` on a
/// vertical one, scaled by `scale` (rounded to the nearest pixel) and laid
/// on the edge by [`edge_rect`]; the other value does not count. Pure.
pub fn plan_dock_rect(
    work_area: Rect,
    scale: f64,
    css_width: f64,
    css_height: f64,
    edge: DockPosition,
) -> Rect {
    let thickness = if is_vertical(edge) {
        css_width
    } else {
        css_height
    };
    edge_rect(work_area, edge, physical_px(thickness, scale))
}

/// `work_area` made at least `length` physical pixels long along `edge`
/// (wider on `bottom` and `top`, taller on `left` and `right`), its origin
/// kept: where no monitor list is known, the content's length is the lower
/// bound, so the panel is never cut. Pure.
pub fn with_min_length(work_area: Rect, edge: DockPosition, length: i32) -> Rect {
    let mut grown = work_area;
    if is_vertical(edge) {
        grown.height = grown.height.max(length);
    } else {
        grown.width = grown.width.max(length);
    }
    grown
}

/// The thickness of a window of physical `size` (width, height) placed on
/// `placed` last: its width on a vertical edge, its height on a horizontal
/// one, and its height before its first placement (`None`: the window is
/// at [`INITIAL_SIZE`], whose height is the initial thickness on any edge).
/// What the window keeps when it is laid out before its UI reported a
/// content size. Pure.
pub fn thickness_of(size: (u32, u32), placed: Option<DockPosition>) -> i32 {
    let across = match placed {
        Some(edge) if is_vertical(edge) => size.0,
        _ => size.1,
    };
    i32::try_from(across).unwrap_or(i32::MAX)
}

/// The rect (physical px) a dock window is laid out at on `edge` of
/// `work_area`, every time it is sized or placed ([`layout`]):
/// - with `content`, the UI's latest content size (CSS px), taken across
///   `edge` by [`DockContent::on_edge`] (a size reported on an edge of the
///   other orientation keeps its thickness) and scaled by `scale`
///   ([`plan_dock_rect`]); where `monitor_known` is `false` (`work_area` is
///   Tauri's own view, the platform listed no monitors yet) the content's
///   length is the lower bound of the window's length, so the panel is
///   never cut (`docs/ipc/sizing-protocol.md`);
/// - before the UI's first report (`content` is `None`), at the window's
///   current thickness: the side of its physical `size` that
///   [`thickness_of`] picks for the edge it was `placed` on last (after a
///   DPI change, the size tao gave it to keep its logical size).
///
/// Pure.
pub fn relayout_rect(
    work_area: Rect,
    monitor_known: bool,
    edge: DockPosition,
    content: Option<DockContent>,
    scale: f64,
    size: (u32, u32),
    placed: Option<DockPosition>,
) -> Rect {
    let Some(content) = content else {
        return edge_rect(work_area, edge, thickness_of(size, placed));
    };
    let (width, height) = content.on_edge(edge);
    let work_area = if monitor_known {
        work_area
    } else {
        let length = if is_vertical(edge) { height } else { width };
        with_min_length(work_area, edge, physical_px(length, scale))
    };
    plan_dock_rect(work_area, scale, width, height, edge)
}

/// One window call of a placement ([`placement_steps`]), in physical px.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaceStep {
    /// A new size; the top-left corner stays (`set_size`).
    Size { width: i32, height: i32 },
    /// A new top-left corner; the size stays (`set_position`).
    Move { x: i32, y: i32 },
}

/// The window calls that take a dock window from `current` (its rect now)
/// to `target`, such that every rect it passes through lies inside
/// `current` or inside `target`: shrink to the smaller of the two sizes on
/// each axis where it stands (inside `current`), move (inside `target`),
/// grow to `target`; a call that would change nothing is left out, and a
/// window that keeps its top-left corner is resized in one call. The new
/// size at the old place, which a resize-then-move would pass through, can
/// stick out of both on a move between edges (a right dock turning into a
/// top one, 1920 px wide at the old `x`, lies mostly on the monitor to its
/// right), and Windows changes the DPI of a window whose larger part lands
/// on a monitor with another scale (`WM_DPICHANGED`, which tao answers by
/// resizing the window), then changes it back on the move. With
/// `current` unknown (`None`), resize then move. Pure.
pub fn placement_steps(current: Option<Rect>, target: Rect) -> Vec<PlaceStep> {
    let resize = PlaceStep::Size {
        width: target.width,
        height: target.height,
    };
    let reposition = PlaceStep::Move {
        x: target.x,
        y: target.y,
    };
    let Some(current) = current else {
        return vec![resize, reposition];
    };
    let same_size = (current.width, current.height) == (target.width, target.height);
    if (current.x, current.y) == (target.x, target.y) {
        return if same_size { Vec::new() } else { vec![resize] };
    }
    let interim = (
        current.width.min(target.width).max(1),
        current.height.min(target.height).max(1),
    );
    let mut steps = Vec::with_capacity(3);
    if interim != (current.width, current.height) {
        steps.push(PlaceStep::Size {
            width: interim.0,
            height: interim.1,
        });
    }
    steps.push(reposition);
    if interim != (target.width, target.height) {
        steps.push(resize);
    }
    steps
}

/// The content size a dock window's UI reported through
/// `set_dock_content_size` (CSS px, clamped by [`clamp_content`]), with the
/// edge in effect when it was reported, kept per window
/// ([`DockWindowState`]) to lay the window out again.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DockContent {
    pub width: f64,
    pub height: f64,
    pub edge: DockPosition,
}

impl DockContent {
    /// The content as `(width, height)` for a dock window on `edge`: as
    /// reported while `edge` has the orientation it was reported on,
    /// transposed otherwise, so the thickness stays the thickness (and the
    /// length the length) after an edge change until the UI reports for the
    /// new edge (`docs/ipc/sizing-protocol.md`, step 5). Pure.
    pub fn on_edge(self, edge: DockPosition) -> (f64, f64) {
        if is_vertical(edge) == is_vertical(self.edge) {
            (self.width, self.height)
        } else {
            (self.height, self.width)
        }
    }
}

/// A reported content size (CSS px) clamped for a dock on `edge`: the value
/// across the edge (the height on a horizontal dock, the width on a
/// vertical one) to [`THICKNESS_RANGE`], the one along it to
/// [`LENGTH_RANGE`]. Returns `(width, height)`. Pure.
pub fn clamp_content(edge: DockPosition, width: f64, height: f64) -> (f64, f64) {
    let length = |v: f64| v.clamp(LENGTH_RANGE.0, LENGTH_RANGE.1);
    let thickness = |v: f64| v.clamp(THICKNESS_RANGE.0, THICKNESS_RANGE.1);
    if is_vertical(edge) {
        (thickness(width), length(height))
    } else {
        (length(width), thickness(height))
    }
}

/// CSS px → physical px, rounded to the nearest pixel (never negative).
fn physical_px(css: f64, scale: f64) -> i32 {
    ((css * scale).round().max(0.0)).min(f64::from(i32::MAX)) as i32
}

// ---------------------------------------------------------------------------
// Settings window
// ---------------------------------------------------------------------------

/// Open the settings window, or show and focus it if it already exists.
/// Closing it destroys it (Tauri's default); the app keeps running. A new
/// window's title bar takes the current *Theme* setting. Main thread.
pub fn open_settings_window(app: &AppHandle) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(SETTINGS_LABEL) {
        window.show()?;
        window.set_focus()?;
        log::debug!("settings window focused");
        return Ok(());
    }
    let mode = app
        .try_state::<crate::AppService>()
        .map(|service| service.0.settings().theme);
    if let (Some(mode), Some(state)) = (mode, app.try_state::<SettingsWindowTheme>()) {
        *state.0.lock() = Some(mode);
    }
    WebviewWindowBuilder::new(app, SETTINGS_LABEL, WebviewUrl::App("settings.html".into()))
        .title("Stem Settings")
        .theme(mode.and_then(native_theme))
        .inner_size(SETTINGS_SIZE.0, SETTINGS_SIZE.1)
        .min_inner_size(SETTINGS_MIN_SIZE.0, SETTINGS_MIN_SIZE.1)
        .resizable(true)
        .decorations(true)
        .always_on_top(false)
        .center()
        .focused(true)
        .build()?;
    log::info!("settings window opened");
    Ok(())
}

// ---------------------------------------------------------------------------
// Visibility helpers (tray, global shortcut, single instance)
// ---------------------------------------------------------------------------

/// The dock UI of `window` reports that auto-hide slid its panel out
/// (`hidden`), or brought it back (`docs/ipc/toggle-while-hidden.md`).
/// Remembered per window label for [`toggle_dock`]; logged on a
/// change.
pub fn set_auto_hidden(
    app: &AppHandle,
    window: &WebviewWindow,
    hidden: bool,
) -> Result<(), String> {
    let flags = app
        .try_state::<DockAutoHidden>()
        .ok_or_else(|| "auto-hide state not available".to_owned())?;
    let previous = flags.0.lock().insert(window.label().to_owned(), hidden);
    if previous != Some(hidden) {
        log::debug!(
            "dock panel of {:?} {}",
            window.label(),
            if hidden { "auto-hidden" } else { "shown" }
        );
        crate::backdrop::on_auto_hidden(app, window, hidden);
    }
    Ok(())
}

/// What the UI of the dock window `label` last reported through
/// [`set_auto_hidden`]; `false` until it reports.
fn is_auto_hidden(app: &AppHandle, label: &str) -> bool {
    app.try_state::<DockAutoHidden>()
        .is_some_and(|flags| flags.0.lock().get(label).copied().unwrap_or(false))
}

/// What one press of the toggle does to the whole set of dock windows,
/// decided from the primary's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Toggle {
    /// The primary window is visible with its panel shown: hide every window.
    Hide,
    /// The primary window is hidden: show every window.
    Show,
    /// The primary window is visible with its panel slid out by auto-hide:
    /// ask every page for its panel (and show a window that is hidden).
    Reveal,
}

/// Tray left click and the global toggle shortcut: one press brings
/// the dock back whatever hid it, the next hides it. With one dock
/// per monitor the primary's window decides and every dock window
/// follows, so one press acts on the whole set:
/// - primary hidden: show every window; the engine gets their panels back.
/// - primary visible but its panel slid out by auto-hide
///   ([`DockAutoHidden`]): ask every page to bring its panel back
///   (`dock://show`; a page whose panel is up ignores it); a secondary
///   window that happens to be hidden is shown instead.
/// - primary visible and its panel shown: hide every window; the engine has
///   none of their panels on screen meanwhile.
///
/// The UI reports its phase after the fact, so a press that lands while the
/// panel is sliding out may still hide the windows; the next press shows
/// them (and the pages, visible again, bring their panels back).
pub fn toggle_dock(app: &AppHandle) {
    let Some(primary) = app.get_webview_window(DOCK_LABEL) else {
        log::warn!("toggle_dock: no dock window");
        return;
    };
    let action = match primary.is_visible() {
        Ok(true) if is_auto_hidden(app, DOCK_LABEL) => Toggle::Reveal,
        Ok(true) => Toggle::Hide,
        Ok(false) => Toggle::Show,
        Err(e) => {
            log::warn!("toggle_dock failed: {e}");
            return;
        }
    };
    log::debug!("toggle_dock: {action:?}");
    if action == Toggle::Hide {
        // The backdrop goes before the windows: no material without its panel.
        crate::backdrop::on_panel(app, &primary, None);
    }
    for window in dock_windows(app) {
        let label = window.label().to_owned();
        let result = match action {
            Toggle::Hide => window.hide().inspect(|()| {
                log::debug!("dock window {label:?} hidden");
                if let Some(service) = app.try_state::<crate::AppService>() {
                    service
                        .0
                        .set_dock_panel(&label, monitor_id_of(app, &label), None);
                }
                // A hidden dock reserves no screen space.
                crate::appbar::sync(app, &window);
            }),
            Toggle::Show => show_dock_window(&window),
            Toggle::Reveal if window.is_visible().unwrap_or(true) => {
                log::debug!("dock reveal requested for {label:?}");
                app.emit_to(&label, crate::events::SHOW, ())
            }
            Toggle::Reveal => show_dock_window(&window),
        };
        if let Err(e) = result {
            log::warn!("toggle_dock on {label:?} failed: {e}");
        }
    }
}

/// Show a hidden dock window, give the engine its panel back and ask the
/// shell for its band again.
fn show_dock_window(window: &WebviewWindow) -> tauri::Result<()> {
    window.show().inspect(|()| {
        log::debug!("dock window {:?} shown", window.label());
        sync_dock_panel(window, None);
        crate::appbar::sync(window.app_handle(), window);
    })
}

/// Show and focus every window we have (the dock windows, and the settings
/// window when it is open). Used when a second instance is started. A dock
/// window that is on screen with its panel slid out by auto-hide is asked
/// to bring the panel back instead (`dock://show`, as [`toggle_dock`] does).
pub fn reveal_all(app: &AppHandle) {
    for (label, window) in app.webview_windows() {
        if label.starts_with(DOCK_LABEL)
            && window.is_visible().unwrap_or(false)
            && is_auto_hidden(app, &label)
        {
            log::debug!("dock reveal requested by a second instance for {label:?}");
            if let Err(e) = app.emit_to(&label, crate::events::SHOW, ()) {
                log::warn!("reveal {label}: {e}");
            }
            continue;
        }
        if let Err(e) = window.show().and_then(|()| window.set_focus()) {
            log::warn!("reveal {label}: {e}");
        } else if label.starts_with(DOCK_LABEL) {
            sync_dock_panel(&window, None);
            crate::appbar::sync(app, &window);
        }
    }
}

// ---------------------------------------------------------------------------
// Keyboard entry: the focus shortcut
// ---------------------------------------------------------------------------

/// The focus shortcut's session (`docs/ipc/keyboard-entry.md`):
/// the dock window the latest press gave the keyboard focus to, and the
/// window that was in front then, which Escape or a second press
/// (`release_dock_focus`) brings back. `None` while no dock has the
/// keyboard from the shortcut: before the first press, and again once the
/// focus was given back ([`DockFocus::release`]), the dock lost the
/// activation ([`DockFocus::blurred`]), it closed or its activation failed
/// ([`DockFocus::end`]). A click on a dock forgets only the window
/// ([`DockFocus::supersede`]): the dock may still have the keyboard, and
/// then a second press still lets go ([`DockFocus::holder`]). Kept in
/// Tauri's managed state. The rules are its methods, which touch nothing
/// but the lock, so they are tested without an app; the functions below
/// that take an `AppHandle` only gather their inputs and log.
#[derive(Debug, Default)]
pub struct DockFocus(Mutex<Option<FocusSession>>);

/// One press of the focus shortcut, as [`DockFocus`] keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FocusSession {
    /// The label of the dock window that was given the focus.
    pub dock: String,
    /// The window that was in front at the press ([`return_target`]);
    /// `None` when there was none to go back to (the desktop or the Start
    /// menu was in front) or a click on a dock superseded it, and then
    /// Escape leaves the foreground alone.
    pub window: Option<ReturnTo>,
}

/// The window a press of the focus shortcut gives the keyboard focus back
/// to ([`return_target`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReturnTo {
    /// An app's window: the engine's active window at the press
    /// (`DockState.foreground`). Brought back with `Activate`
    /// ([`return_focus`]).
    App(WindowId),
    /// One of Stem's own windows other than a dock window (the settings
    /// window), by its label. The engine never takes Stem's own windows for
    /// the foreground, so its active window is then the app behind it.
    /// Brought back with Tauri's `set_focus`.
    Own(String),
}

/// What the dock window that gives the keyboard focus back
/// ([`DockFocus::release`]) is to bring back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Release {
    /// The session was this dock's: bring this window back.
    Window(ReturnTo),
    /// The session was this dock's, with nothing to go back to (nothing was
    /// in front at the press, or a click superseded the window): the
    /// foreground stays.
    Nothing,
    /// The session was another dock window's (this label), which is no
    /// longer the foreground window and lost the activation without its
    /// blur reaching us: stale, and dropped too; the foreground stays.
    Stale(String),
    /// The session is another dock window's (this label), which is still
    /// the foreground window: it still has the keyboard and its own release
    /// brings the window back, so the session stays; nothing happens here.
    Held(String),
    /// No session: the focus shortcut did not bring the keyboard here.
    NoSession,
}

impl DockFocus {
    /// The dock window a press of the focus shortcut lets go of, as Escape
    /// does: `front` (the dock window that is the foreground window, if one
    /// is) when the session is that dock's, which still has the keyboard
    /// from an earlier press. `None`: the press gives a dock the focus, also
    /// while a dock is in front without the session (after an Escape that
    /// had nothing to go back to).
    pub fn holder(&self, front: Option<&str>) -> Option<String> {
        let front = front?;
        self.0
            .lock()
            .as_ref()
            .filter(|session| session.dock == front)
            .map(|session| session.dock.clone())
    }

    /// A press gives the dock window `dock` the focus; `window` is the
    /// window in front at the press ([`return_target`]). Replaces any
    /// earlier session.
    pub fn begin(&self, dock: &str, window: Option<ReturnTo>) {
        *self.0.lock() = Some(FocusSession {
            dock: dock.to_owned(),
            window,
        });
    }

    /// The dock window `label` gives the keyboard focus back (Escape, or a
    /// second press) while `front` is the dock window that is the
    /// foreground window (`None`: no dock is): take the session and say
    /// what to bring back. A session of another dock that is still in front
    /// is that dock's to give back and stays ([`Release::Held`]: a stray
    /// release from a dock that does not have the keyboard cannot lose the
    /// window to go back to); one whose dock is no longer in front is stale
    /// and is dropped too.
    pub fn release(&self, label: &str, front: Option<&str>) -> Release {
        let mut guard = self.0.lock();
        match guard.as_ref() {
            None => return Release::NoSession,
            Some(session) if session.dock != label && front == Some(session.dock.as_str()) => {
                return Release::Held(session.dock.clone());
            }
            Some(_) => {}
        }
        match guard.take() {
            Some(FocusSession {
                dock,
                window: Some(window),
            }) if dock == label => Release::Window(window),
            Some(session) if session.dock == label => Release::Nothing,
            Some(session) => Release::Stale(session.dock),
            None => Release::NoSession,
        }
    }

    /// A click on a dock (`primary_click`, `launch_app`, `window_action`)
    /// activates or launches something, which supersedes the window the
    /// focus shortcut remembered: forget it, whichever dock holds the
    /// session, so a later Escape does not undo the click. The session
    /// stays: when the click leaves that dock in front (a launch whose
    /// window is not up yet, a minimise) it still has the keyboard, and a
    /// second press still lets go; when it brings another window to the
    /// front, the dock's blur ends the session. The dock whose window was
    /// forgotten, if one was.
    pub fn supersede(&self) -> Option<String> {
        let mut guard = self.0.lock();
        let session = guard.as_mut()?;
        session.window.take().map(|_| session.dock.clone())
    }

    /// End the session if it is the dock window `label`'s (it closed, or
    /// its activation failed); whether it was.
    pub fn end(&self, label: &str) -> bool {
        let mut session = self.0.lock();
        if session.as_ref().is_some_and(|s| s.dock == label) {
            *session = None;
            true
        } else {
            false
        }
    }

    /// The dock window `label`'s webview lost the keyboard focus (Tauri's
    /// `Focused(false)`) while `front` is the dock window that is the
    /// foreground window (`None`: no dock is). When `label` is not in front
    /// any more, another window took the activation (a click elsewhere,
    /// Alt+Tab, the window Escape brought back) and its session ends; a dock
    /// that is still in front (its own native menu took the focus for a
    /// moment) keeps it. Whether a session ended.
    pub fn blurred(&self, label: &str, front: Option<&str>) -> bool {
        front != Some(label) && self.end(label)
    }

    /// The session as it is now (for the tests).
    #[cfg(test)]
    fn session(&self) -> Option<FocusSession> {
        self.0.lock().clone()
    }
}

/// The monitor of the window `id` as the engine's snapshot lists it (its
/// `WindowInfo.monitor`, in whichever item shows it); `None` when no item
/// shows the window or its monitor is unknown. Pure.
pub fn window_monitor(state: &DockState, id: WindowId) -> Option<MonitorId> {
    state
        .items
        .iter()
        .flat_map(|item| item.windows.iter())
        .find(|w| w.id == id)
        .and_then(|w| w.monitor.clone())
}

/// The dock window the focus shortcut gives the focus to: with
/// `monitors = all`, the one placed on `monitor`, the monitor of the window
/// in front; otherwise, or when no dock window sits on that monitor (none
/// known for the window, a monitor that got no dock), the primary's
/// ([`DOCK_LABEL`]). `docks` are the dock windows that exist, each with the
/// monitor it was placed on. Pure.
pub fn focus_target(
    mode: MonitorMode,
    monitor: Option<&MonitorId>,
    docks: &[(String, Option<MonitorId>)],
) -> String {
    if mode == MonitorMode::All
        && let Some(monitor) = monitor
        && let Some((label, _)) = docks.iter().find(|(_, m)| m.as_ref() == Some(monitor))
    {
        return label.clone();
    }
    DOCK_LABEL.to_owned()
}

/// What a press of the focus shortcut gives the keyboard focus back to:
/// `own` is the label of Stem's own window that is the foreground
/// window at the press, if one is ([`own_foreground`]), and `engine` the
/// engine's active window (`DockState.foreground`). The engine never takes
/// Stem's own windows for the foreground, so while the settings window is
/// in front its active window is the app behind it: the settings window is
/// then the one to go back to. A dock window in front (after an Escape that
/// had nothing to go back to) is not one: the engine's window stays the
/// one. Pure.
pub fn return_target(own: Option<&str>, engine: Option<WindowId>) -> Option<ReturnTo> {
    match own {
        Some(label) if !label.starts_with(DOCK_LABEL) => Some(ReturnTo::Own(label.to_owned())),
        _ => engine.map(ReturnTo::App),
    }
}

/// The monitor whose bounds are `bounds` in `monitors` (the platform's
/// list); `None` when none has them. Tauri's `current_monitor` gives a
/// window's monitor by the same rectangle the platform reports as
/// `MonitorInfo.bounds` (the monitor's full rect, physical px). Pure.
pub fn monitor_with_bounds(monitors: &[MonitorInfo], bounds: Rect) -> Option<MonitorId> {
    monitors
        .iter()
        .find(|monitor| monitor.bounds == bounds)
        .map(|monitor| monitor.id.clone())
}

/// The monitor Stem's own window `label` is on (the one with the largest
/// part of it, Tauri's `current_monitor`) as `monitors` lists it; `None`
/// when the window is gone or its monitor is not in the list.
fn own_window_monitor(app: &AppHandle, label: &str, monitors: &[MonitorInfo]) -> Option<MonitorId> {
    let monitor = app.get_webview_window(label)?.current_monitor().ok()??;
    let position = monitor.position();
    let size = monitor.size();
    let bounds = Rect::new(
        position.x,
        position.y,
        i32::try_from(size.width).unwrap_or(i32::MAX),
        i32::try_from(size.height).unwrap_or(i32::MAX),
    );
    monitor_with_bounds(monitors, bounds)
}

/// The label of Stem's own window (a dock window, the settings window) that
/// is the foreground window right now, if one is (the platform's view;
/// never on a host without native handles).
fn own_foreground(app: &AppHandle, platform: &dyn Platform) -> Option<String> {
    let foreground = platform.foreground_native_window()?;
    app.webview_windows()
        .into_iter()
        .find(|(_, window)| native_handle(window) == Some(foreground))
        .map(|(label, _)| label)
}

/// The dock window that is the foreground window right now, if one is (the
/// platform's view; never on a host without native handles).
fn focused_dock(app: &AppHandle, platform: &dyn Platform) -> Option<String> {
    let foreground = platform.foreground_native_window()?;
    dock_windows(app)
        .into_iter()
        .find(|window| native_handle(window) == Some(foreground))
        .map(|window| window.label().to_owned())
}

/// The focus shortcut was pressed (`docs/ipc/keyboard-entry.md`).
/// Main thread: the plugin's handler, inside the `WM_HOTKEY` that gives
/// Stem the right to take the foreground.
///
/// A press while a dock window has the keyboard from an earlier press (it
/// is in front and the session is its own, [`DockFocus::holder`]) tells
/// its page `dock://release-focus`: the page does what Escape does, lets go
/// of its item and calls `release_dock_focus`, which brings the remembered
/// window back if there is one. Otherwise, also while a dock is in front
/// without the session: remember the window in front ([`return_target`]:
/// the settings window when it is the foreground window, else the engine's
/// active window; [`DockFocus::begin`]); pick the dock window on its
/// monitor ([`focus_target`]); show that window if the tray or the toggle hid it,
/// or ask its page for the panel (`dock://show`) if auto-hide slid it out;
/// then, off the main thread, make it the foreground window
/// (`Platform::focus_dock_window`) and tell its page
/// (`dock://focus`), which focuses its toolbar ([`activated`]).
pub fn on_focus_shortcut(app: &AppHandle) {
    let (Some(service), Some(focus)) = (
        app.try_state::<crate::AppService>(),
        app.try_state::<DockFocus>(),
    ) else {
        return;
    };
    let service = std::sync::Arc::clone(&service.0);
    // Which of our windows is in front, if one is: a dock (the holder of an
    // earlier press, or one in front without the session) or the settings
    // window (the window to go back to).
    let own = own_foreground(app, service.platform().as_ref());
    let front = own.as_deref().filter(|label| label.starts_with(DOCK_LABEL));
    if let Some(label) = focus.holder(front) {
        log::debug!("focus shortcut: dock window {label:?} has the keyboard; it lets go");
        if let Err(e) = app.emit_to(&label, crate::events::RELEASE_FOCUS, ()) {
            log::warn!("focus shortcut: telling dock window {label:?} to let go failed: {e}");
        }
        return;
    }

    let state = service.state();
    let window = return_target(own.as_deref(), state.foreground);
    let monitor = match &window {
        Some(ReturnTo::App(id)) => window_monitor(&state, *id),
        Some(ReturnTo::Own(label)) => own_window_monitor(app, label, &service.monitors()),
        None => None,
    };
    let docks: Vec<(String, Option<MonitorId>)> = app
        .try_state::<DockGeometry>()
        .map(|geometry| {
            geometry
                .labels()
                .into_iter()
                .map(|label| {
                    let monitor = geometry.dock(&label).and_then(|dock| dock.monitor);
                    (label, monitor)
                })
                .collect()
        })
        .unwrap_or_default();
    let label = focus_target(service.settings().monitors, monitor.as_ref(), &docks);
    let Some(dock) = app.get_webview_window(&label) else {
        log::warn!("focus shortcut: no dock window {label:?}");
        return;
    };
    bring_on_screen(app, &dock);
    let Some(handle) = native_handle(&dock) else {
        return;
    };
    log::info!(
        "focus shortcut: the keyboard goes to dock window {label:?} (monitor {}); Escape goes back to {}",
        monitor.as_ref().map_or("unknown", |m| m.0.as_str()),
        match &window {
            Some(ReturnTo::App(id)) => format!("{id:?}"),
            Some(ReturnTo::Own(own)) => format!("our window {own:?}"),
            None => "nothing".to_owned(),
        }
    );
    focus.begin(&label, window);
    let app = app.clone();
    // Off the main thread: the activation's fallbacks touch other threads'
    // input state (docs/development/windows-rules.md). The page is told
    // once its window is in front; until the WebView has the focus the
    // browser holds the page's `focus()` back, so either order ends with
    // the item focused.
    drop(tauri::async_runtime::spawn_blocking(move || {
        let result = service.platform().focus_dock_window(handle);
        let Some(focus) = app.try_state::<DockFocus>() else {
            return;
        };
        if activated(&focus, &label, result)
            && let Err(e) = app.emit_to(&label, crate::events::FOCUS, ())
        {
            log::warn!("focus shortcut: telling dock window {label:?} failed: {e}");
        }
    }));
}

/// The activation of the dock window `label` at a press ended with
/// `result`: whether its page is to be told (`dock://focus`). A failure is
/// a warning and ends the session if it is still that dock's (a later press
/// may have given another dock the focus meanwhile); the page is not told.
fn activated(focus: &DockFocus, label: &str, result: Result<(), PlatformError>) -> bool {
    match result {
        Ok(()) => true,
        Err(e) => {
            log::warn!(
                "focus shortcut: dock window {label:?} could not take the keyboard focus: {e}"
            );
            if focus.end(label) {
                log::debug!(
                    "dock window {label:?}: its activation failed; the window to go back to is forgotten"
                );
            }
            false
        }
    }
}

/// Step 3 of a press: a dock window the tray or the toggle hid is shown; one
/// whose panel auto-hide slid out gets `dock://show`, and the page's
/// `focus` hold keeps the panel up once an item has the focus.
fn bring_on_screen(app: &AppHandle, dock: &WebviewWindow) {
    let label = dock.label();
    match dock.is_visible() {
        Ok(false) => match show_dock_window(dock) {
            Ok(()) => log::debug!("focus shortcut: dock window {label:?} shown"),
            Err(e) => log::warn!("focus shortcut: showing dock window {label:?} failed: {e}"),
        },
        Ok(true) if is_auto_hidden(app, label) => {
            log::debug!("focus shortcut: dock reveal requested for {label:?}");
            if let Err(e) = app.emit_to(label, crate::events::SHOW, ()) {
                log::warn!("focus shortcut: revealing dock window {label:?} failed: {e}");
            }
        }
        Ok(true) => {}
        Err(e) => log::warn!("focus shortcut: dock window {label:?}: {e}"),
    }
}

/// `release_dock_focus` from the dock window `label` (Escape, or the page's
/// answer to a second press's `dock://release-focus`): [`give_focus_back`]
/// with the dock window that is in front now, an app's window brought back
/// through the platform and Stem's own window through [`focus_own_window`].
/// Blocking (bounded): never on the main thread.
pub fn release_dock_focus(
    app: &AppHandle,
    focus: &DockFocus,
    platform: &dyn Platform,
    label: &str,
) -> Result<(), String> {
    let front = focused_dock(app, platform);
    give_focus_back(focus, platform, label, front.as_deref(), |own| {
        focus_own_window(app, label, own)
    })
}

/// The dock window `label` gives the keyboard focus back while `front` is
/// the dock window in front: take the session ([`DockFocus::release`]) and
/// bring the remembered window back, an app's with [`return_focus`], Stem's
/// own with `focus_own`; with nothing to bring back, or a session another
/// dock still holds, the foreground stays. Blocking (bounded): never on the
/// main thread.
pub fn give_focus_back(
    focus: &DockFocus,
    platform: &dyn Platform,
    label: &str,
    front: Option<&str>,
    focus_own: impl FnOnce(&str) -> Result<(), String>,
) -> Result<(), String> {
    match focus.release(label, front) {
        Release::Window(ReturnTo::App(window)) => return_focus(platform, label, window),
        Release::Window(ReturnTo::Own(own)) => focus_own(&own),
        Release::Nothing => {
            log::debug!(
                "dock window {label:?} gives the focus back; nothing was in front at the press, or a click superseded it"
            );
            Ok(())
        }
        Release::Stale(other) => {
            log::debug!(
                "dock window {label:?} gives the focus back; the session of {other:?} is stale and dropped"
            );
            Ok(())
        }
        Release::Held(other) => {
            log::debug!(
                "dock window {label:?} gives the focus back, but the session is {other:?}'s, which is still in front; it stays"
            );
            Ok(())
        }
        Release::NoSession => {
            log::debug!("dock window {label:?} gives the focus back; nothing to go back to");
            Ok(())
        }
    }
}

/// Bring Stem's own window `own` (the settings window) back to the
/// foreground for the dock window `label` that gives the focus back:
/// Tauri's `set_focus`, which runs on the main thread (`SetForegroundWindow`;
/// the dock in front belongs to that thread, so Windows allows it). A
/// window that closed meanwhile leaves the foreground alone (debug); a
/// failure is returned. Any thread.
fn focus_own_window(app: &AppHandle, label: &str, own: &str) -> Result<(), String> {
    let Some(window) = app.get_webview_window(own) else {
        log::debug!(
            "dock window {label:?} gives the focus back, but {own:?} is gone; the foreground stays"
        );
        return Ok(());
    };
    match window.set_focus() {
        Ok(()) => {
            log::debug!("dock window {label:?} gave the focus back to {own:?}");
            Ok(())
        }
        Err(e) => Err(format!(
            "dock window {label:?} could not give the focus back to {own:?}: {e}"
        )),
    }
}

/// Bring `window` back to the foreground for the dock window `label` that
/// gives the focus back: `Activate` through the platform.
/// A window that is gone leaves the foreground alone (debug); any other
/// failure is returned. Blocking (bounded): never on the main thread.
pub fn return_focus(platform: &dyn Platform, label: &str, window: WindowId) -> Result<(), String> {
    match platform.window_action(window, WindowAction::Activate) {
        Ok(()) => {
            log::debug!("dock window {label:?} gave the focus back to {window:?}");
            Ok(())
        }
        Err(PlatformError::WindowNotFound(_)) => {
            log::debug!(
                "dock window {label:?} gives the focus back, but {window:?} is gone; the foreground stays"
            );
            Ok(())
        }
        Err(e) => Err(format!(
            "dock window {label:?} could not give the focus back to {window:?}: {e}"
        )),
    }
}

/// A click on a dock (`primary_click`, `launch_app`, `window_action`)
/// activates or launches something, which supersedes the window the focus
/// shortcut remembered ([`DockFocus::supersede`]: the window is
/// forgotten, the session stays). Any thread.
pub fn forget_dock_focus(app: &AppHandle, why: &str) {
    let Some(focus) = app.try_state::<DockFocus>() else {
        return;
    };
    if let Some(dock) = focus.supersede() {
        log::debug!("dock window {dock:?}: {why}; the window to go back to is forgotten");
    }
}

/// End the session if it is the dock window `label`'s
/// ([`DockFocus::end`]). Any thread.
fn forget_session_of(app: &AppHandle, label: &str, why: &str) {
    let Some(focus) = app.try_state::<DockFocus>() else {
        return;
    };
    if focus.end(label) {
        log::debug!("dock window {label:?}: {why}; the window to go back to is forgotten");
    }
}

/// The dock window `label`'s webview lost the keyboard focus (Tauri's
/// `Focused(false)`; [`DockFocus::blurred`] with the dock window
/// that is in front now). Main thread.
fn on_dock_blurred(app: &AppHandle, label: &str) {
    let (Some(service), Some(focus)) = (
        app.try_state::<crate::AppService>(),
        app.try_state::<DockFocus>(),
    ) else {
        return;
    };
    let front = focused_dock(app, service.0.platform().as_ref());
    if focus.blurred(label, front.as_deref()) {
        log::debug!(
            "dock window {label:?}: it lost the activation; the window to go back to is forgotten"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(id: &str, primary: bool, x: i32, scale: f64) -> MonitorInfo {
        MonitorInfo {
            id: MonitorId(id.into()),
            name: id.trim_start_matches(r"\\.\").to_owned(),
            bounds: Rect::new(x, 0, 2560, 1440),
            work_area: Rect::new(x, 0, 2560, 1392),
            scale_factor: scale,
            is_primary: primary,
        }
    }

    #[test]
    fn dock_labels_keep_only_safe_characters_and_pages_carry_the_encoded_id() {
        let display3 = MonitorId(r"\\.\DISPLAY3".into());
        assert_eq!(dock_label(&display3), "dock-DISPLAY3");
        assert_eq!(
            dock_label(&MonitorId("mock-primary".into())),
            "dock-mock-primary"
        );
        assert_eq!(dock_label(&MonitorId("a b/c:d_e".into())), "dock-abcd_e");
        assert_eq!(dock_label(&MonitorId(String::new())), "dock-");
        assert_eq!(query_value(r"\\.\DISPLAY3"), "%5C%5C.%5CDISPLAY3");
        assert_eq!(query_value("mock-primary_1.~"), "mock-primary_1.~");
        assert_eq!(query_value("a b&c=d"), "a%20b%26c%3Dd");
        assert_eq!(query_value("é"), "%C3%A9");
        // The primary's page has no query string, whatever its monitor.
        assert_eq!(dock_page(DOCK_LABEL, Some(&display3)), "dock.html");
        assert_eq!(dock_page(DOCK_LABEL, None), "dock.html");
        assert_eq!(
            dock_page("dock-DISPLAY3", Some(&display3)),
            "dock.html?monitor=%5C%5C.%5CDISPLAY3"
        );
        assert_eq!(dock_page("dock-DISPLAY3", None), "dock.html");
    }

    #[test]
    fn plan_docks_gives_the_primary_one_window_and_all_one_per_monitor() {
        let primary = monitor(r"\\.\DISPLAY2", true, 0, 1.0);
        let left = monitor(r"\\.\DISPLAY1", false, -1920, 1.5);
        let right = monitor(r"\\.\DISPLAY3", false, 2560, 1.0);
        let monitors = vec![left.clone(), primary.clone(), right.clone()];

        // The default: exactly today's one window, on the primary.
        assert_eq!(
            plan_docks(MonitorMode::Primary, &monitors),
            vec![DockPlan {
                label: "dock".into(),
                monitor: Some(primary.clone()),
            }]
        );
        // No monitor known yet: still the one window, unplaced.
        assert_eq!(
            plan_docks(MonitorMode::Primary, &[]),
            vec![DockPlan {
                label: "dock".into(),
                monitor: None,
            }]
        );
        assert_eq!(
            plan_docks(MonitorMode::All, &[]),
            vec![DockPlan {
                label: "dock".into(),
                monitor: None,
            }]
        );

        // All: the primary keeps `dock`; the others follow in list order.
        assert_eq!(
            plan_docks(MonitorMode::All, &monitors),
            vec![
                DockPlan {
                    label: "dock".into(),
                    monitor: Some(primary.clone()),
                },
                DockPlan {
                    label: "dock-DISPLAY1".into(),
                    monitor: Some(left.clone()),
                },
                DockPlan {
                    label: "dock-DISPLAY3".into(),
                    monitor: Some(right.clone()),
                },
            ]
        );
        // One monitor only: `All` is the same as `Primary`.
        assert_eq!(
            plan_docks(MonitorMode::All, std::slice::from_ref(&primary)),
            plan_docks(MonitorMode::Primary, std::slice::from_ref(&primary))
        );
    }

    #[test]
    fn plan_docks_falls_back_to_the_first_monitor_and_skips_label_collisions() {
        // Nothing flagged primary: the first monitor is the primary's.
        let a = monitor(r"\\.\DISPLAY1", false, 0, 1.0);
        let b = monitor(r"\\.\DISPLAY2", false, 2560, 1.0);
        let plan = plan_docks(MonitorMode::All, &[a.clone(), b.clone()]);
        assert_eq!(plan[0].label, "dock");
        assert_eq!(plan[0].monitor, Some(a));
        assert_eq!(plan[1].label, "dock-DISPLAY2");
        assert_eq!(plan[1].monitor, Some(b));

        // Two ids that sanitise to the same label: the second is left out.
        let primary = monitor(r"\\.\DISPLAY1", true, 0, 1.0);
        let x = monitor(r"\\.\DISPLAY2", false, 2560, 1.0);
        let twin = monitor("DISPLAY2", false, 5120, 1.0);
        let plan = plan_docks(MonitorMode::All, &[primary, x.clone(), twin]);
        assert_eq!(plan.len(), 2);
        assert_eq!(plan[1].monitor, Some(x));
    }

    #[test]
    fn dock_geometry_lists_the_primary_first() {
        let geometry = DockGeometry::default();
        for label in ["dock-DISPLAY3", "dock", "dock-DISPLAY1"] {
            geometry
                .docks
                .lock()
                .insert(label.to_owned(), DockWindowState::default());
        }
        assert_eq!(
            geometry.labels(),
            vec!["dock", "dock-DISPLAY1", "dock-DISPLAY3"]
        );
        assert!(geometry.dock("dock-DISPLAY3").is_some());
        assert!(geometry.dock("settings").is_none());
        let content = DockContent {
            width: 640.0,
            height: 96.0,
            edge: DockPosition::Bottom,
        };
        assert_eq!(
            geometry.update("dock", |dock| {
                dock.content = Some(content);
                1
            }),
            Some(1)
        );
        assert_eq!(geometry.update("nope", |_| 1), None);
        assert_eq!(geometry.dock("dock").unwrap().content, Some(content));
        assert_eq!(geometry.dock("dock").unwrap().edge, None);
    }

    const EDGES: [DockPosition; 4] = [
        DockPosition::Bottom,
        DockPosition::Top,
        DockPosition::Left,
        DockPosition::Right,
    ];

    /// The invariants of the placement rule on any edge: inside the work
    /// area, flush with the edge, as long as the work area along it.
    fn assert_on_edge(work_area: Rect, edge: DockPosition, rect: Rect) {
        assert!(rect.width >= 1 && rect.height >= 1, "{rect:?} is empty");
        assert!(
            rect.x >= work_area.x
                && rect.y >= work_area.y
                && rect.right() <= work_area.right()
                && rect.bottom() <= work_area.bottom(),
            "{rect:?} leaves {work_area:?}"
        );
        if is_vertical(edge) {
            assert_eq!((rect.y, rect.height), (work_area.y, work_area.height));
        } else {
            assert_eq!((rect.x, rect.width), (work_area.x, work_area.width));
        }
        let flush = match edge {
            DockPosition::Bottom => rect.bottom() == work_area.bottom(),
            DockPosition::Top => rect.y == work_area.y,
            DockPosition::Left => rect.x == work_area.x,
            DockPosition::Right => rect.right() == work_area.right(),
        };
        assert!(flush, "{rect:?} is not flush with the {edge:?} edge");
    }

    #[test]
    fn edge_rect_spans_the_work_area_along_each_edge_and_is_flush_with_it() {
        // A work area that starts away from the origin (a taskbar on the
        // left and on top, say).
        let wa = Rect::new(100, 50, 1000, 900);
        assert_eq!(
            edge_rect(wa, DockPosition::Bottom, 80),
            Rect::new(100, 870, 1000, 80)
        );
        assert_eq!(
            edge_rect(wa, DockPosition::Top, 80),
            Rect::new(100, 50, 1000, 80)
        );
        assert_eq!(
            edge_rect(wa, DockPosition::Left, 80),
            Rect::new(100, 50, 80, 900)
        );
        assert_eq!(
            edge_rect(wa, DockPosition::Right, 80),
            Rect::new(1020, 50, 80, 900)
        );
        for edge in EDGES {
            for thickness in [-5, 0, 1, 80, 899, 900, 901, 1000, 5000, i32::MAX] {
                assert_on_edge(wa, edge, edge_rect(wa, edge, thickness));
            }
        }
    }

    #[test]
    fn edge_rect_clamps_the_thickness_to_the_work_area_and_never_empties_the_window() {
        let wa = Rect::new(100, 50, 1000, 900);
        // Thicker than the work area: the whole work area, whichever edge.
        for edge in EDGES {
            assert_eq!(edge_rect(wa, edge, 5000), wa, "{edge:?}");
        }
        // The thickness clamps against the work area's height on a
        // horizontal dock and its width on a vertical one.
        assert_eq!(edge_rect(wa, DockPosition::Top, 950).height, 900);
        assert_eq!(edge_rect(wa, DockPosition::Left, 950).width, 950);
        assert_eq!(edge_rect(wa, DockPosition::Right, 1200).width, 1000);
        // Degenerate thicknesses give one pixel, flush with the edge.
        assert_eq!(
            edge_rect(wa, DockPosition::Bottom, 0),
            Rect::new(100, 949, 1000, 1)
        );
        assert_eq!(
            edge_rect(wa, DockPosition::Right, -5),
            Rect::new(1099, 50, 1, 900)
        );
        // A degenerate work area still gives a 1×1 window on its edge.
        let empty = Rect::new(0, 0, 0, 0);
        assert_eq!(
            edge_rect(empty, DockPosition::Bottom, 10),
            Rect::new(0, -1, 1, 1)
        );
        assert_eq!(
            edge_rect(empty, DockPosition::Top, 10),
            Rect::new(0, 0, 1, 1)
        );
        assert_eq!(
            edge_rect(empty, DockPosition::Left, 10),
            Rect::new(0, 0, 1, 1)
        );
        assert_eq!(
            edge_rect(empty, DockPosition::Right, 10),
            Rect::new(-1, 0, 1, 1)
        );
    }

    #[test]
    fn edge_rect_keeps_a_secondary_monitors_negative_origin() {
        // A monitor left of and above the primary, its taskbar at the
        // bottom: work area (-1920, -1080) .. (0, -48).
        let wa = Rect::new(-1920, -1080, 1920, 1032);
        assert_eq!(
            edge_rect(wa, DockPosition::Bottom, 96),
            Rect::new(-1920, -144, 1920, 96)
        );
        assert_eq!(
            edge_rect(wa, DockPosition::Top, 96),
            Rect::new(-1920, -1080, 1920, 96)
        );
        assert_eq!(
            edge_rect(wa, DockPosition::Left, 96),
            Rect::new(-1920, -1080, 96, 1032)
        );
        assert_eq!(
            edge_rect(wa, DockPosition::Right, 96),
            Rect::new(-96, -1080, 96, 1032)
        );
        for edge in EDGES {
            assert_on_edge(wa, edge, edge_rect(wa, edge, 96));
            assert_on_edge(wa, edge, edge_rect(wa, edge, 4000));
        }
    }

    #[test]
    fn plan_dock_rect_takes_the_thickness_across_the_edge_and_scales_it() {
        let wa = Rect::new(0, 0, 2560, 1400);
        // 150 %: 96 css -> 144 physical across the edge; the value along
        // the edge (the panel's length, 700) does not count.
        assert_eq!(
            plan_dock_rect(wa, 1.5, 700.0, 96.0, DockPosition::Bottom),
            Rect::new(0, 1256, 2560, 144)
        );
        assert_eq!(
            plan_dock_rect(wa, 1.5, 700.0, 96.0, DockPosition::Top),
            Rect::new(0, 0, 2560, 144)
        );
        assert_eq!(
            plan_dock_rect(wa, 1.5, 96.0, 700.0, DockPosition::Left),
            Rect::new(0, 0, 144, 1400)
        );
        assert_eq!(
            plan_dock_rect(wa, 1.5, 96.0, 700.0, DockPosition::Right),
            Rect::new(2416, 0, 144, 1400)
        );
        // Fractional results round to the nearest pixel.
        assert_eq!(
            plan_dock_rect(wa, 1.25, 700.0, 97.0, DockPosition::Bottom).height,
            121
        );
        assert_eq!(
            plan_dock_rect(wa, 1.25, 97.0, 700.0, DockPosition::Right).width,
            121
        );
        assert_eq!(physical_px(641.0, 1.25), 801);
        assert_eq!(physical_px(-3.0, 1.0), 0);
        // A secondary monitor to the right of the primary, at 150 %: its own
        // work area on each edge.
        let right = Rect::new(2560, 0, 1920, 1032);
        assert_eq!(
            plan_dock_rect(right, 1.5, 700.0, 96.0, DockPosition::Bottom),
            Rect::new(2560, 888, 1920, 144)
        );
        assert_eq!(
            plan_dock_rect(right, 1.5, 96.0, 700.0, DockPosition::Left),
            Rect::new(2560, 0, 144, 1032)
        );
        assert_eq!(
            plan_dock_rect(right, 1.5, 96.0, 700.0, DockPosition::Right),
            Rect::new(4336, 0, 144, 1032)
        );
    }

    #[test]
    fn content_keeps_its_thickness_across_an_edge_change() {
        // Reported on the bottom: 700 long, 135 thick.
        let bottom = DockContent {
            width: 700.0,
            height: 135.0,
            edge: DockPosition::Bottom,
        };
        assert_eq!(bottom.on_edge(DockPosition::Bottom), (700.0, 135.0));
        assert_eq!(bottom.on_edge(DockPosition::Top), (700.0, 135.0));
        // On a vertical edge the 135 is the width, the window's thickness.
        assert_eq!(bottom.on_edge(DockPosition::Left), (135.0, 700.0));
        assert_eq!(bottom.on_edge(DockPosition::Right), (135.0, 700.0));
        let wa = Rect::new(0, 0, 2560, 1392);
        for edge in EDGES {
            let (width, height) = bottom.on_edge(edge);
            let rect = plan_dock_rect(wa, 1.0, width, height, edge);
            let thickness = if is_vertical(edge) {
                rect.width
            } else {
                rect.height
            };
            assert_eq!(thickness, 135, "{edge:?}");
        }
        // Reported on the right: the width is the thickness.
        let right = DockContent {
            width: 135.0,
            height: 700.0,
            edge: DockPosition::Right,
        };
        assert_eq!(right.on_edge(DockPosition::Left), (135.0, 700.0));
        assert_eq!(right.on_edge(DockPosition::Top), (700.0, 135.0));
    }

    #[test]
    fn a_window_keeps_the_thickness_it_was_placed_with() {
        // Before its first placement the initial height is the thickness.
        assert_eq!(thickness_of((640, 96), None), 96);
        assert_eq!(thickness_of((2560, 135), Some(DockPosition::Bottom)), 135);
        assert_eq!(thickness_of((2560, 135), Some(DockPosition::Top)), 135);
        assert_eq!(thickness_of((135, 1392), Some(DockPosition::Left)), 135);
        assert_eq!(thickness_of((135, 1392), Some(DockPosition::Right)), 135);
        assert_eq!(
            thickness_of((u32::MAX, 1), Some(DockPosition::Left)),
            i32::MAX
        );
        // Moved from the bottom to the left at that thickness.
        let wa = Rect::new(0, 0, 2560, 1392);
        let placed = edge_rect(wa, DockPosition::Bottom, 135);
        let size = (placed.width.unsigned_abs(), placed.height.unsigned_abs());
        let moved = edge_rect(
            wa,
            DockPosition::Left,
            thickness_of(size, Some(DockPosition::Bottom)),
        );
        assert_eq!(moved, Rect::new(0, 0, 135, 1392));
    }

    #[test]
    fn clamp_content_bounds_the_thickness_and_the_length_by_their_roles() {
        // Horizontal: the height is the thickness.
        assert_eq!(
            clamp_content(DockPosition::Bottom, 10.0, 10.0),
            (LENGTH_RANGE.0, THICKNESS_RANGE.0)
        );
        assert_eq!(
            clamp_content(DockPosition::Top, 1e9, 1e9),
            (LENGTH_RANGE.1, THICKNESS_RANGE.1)
        );
        // Vertical: the width is the thickness, so a drag's very thick
        // window is still allowed across a left or right edge.
        assert_eq!(
            clamp_content(DockPosition::Left, 4096.0, 700.0),
            (4096.0, 700.0)
        );
        assert_eq!(
            clamp_content(DockPosition::Right, 10.0, 10.0),
            (THICKNESS_RANGE.0, LENGTH_RANGE.0)
        );
        assert_eq!(
            clamp_content(DockPosition::Right, 1e9, 1e9),
            (THICKNESS_RANGE.1, LENGTH_RANGE.1)
        );
        assert_eq!(
            clamp_content(DockPosition::Bottom, 700.0, 135.0),
            (700.0, 135.0)
        );
    }

    #[test]
    fn with_min_length_grows_the_work_area_along_the_edge_only() {
        let wa = Rect::new(-1920, 0, 1920, 1040);
        assert_eq!(
            with_min_length(wa, DockPosition::Bottom, 2400),
            Rect::new(-1920, 0, 2400, 1040)
        );
        assert_eq!(
            with_min_length(wa, DockPosition::Top, 100),
            wa,
            "never shrinks"
        );
        assert_eq!(
            with_min_length(wa, DockPosition::Left, 1500),
            Rect::new(-1920, 0, 1920, 1500)
        );
        assert_eq!(
            with_min_length(wa, DockPosition::Right, 100),
            wa,
            "never shrinks"
        );
    }

    #[test]
    fn scale_rect_rounds_outwards() {
        // 150 %: 10..=20 css → 15..=30 physical exactly.
        assert_eq!(
            scale_rect(Rect::new(10, 10, 10, 10), 1.5),
            Rect::new(15, 15, 15, 15)
        );
        // 125 %: left 11*1.25=13.75 → 13; right 21*1.25=26.25 → 27.
        assert_eq!(
            scale_rect(Rect::new(11, 0, 10, 8), 1.25),
            Rect::new(13, 0, 14, 10)
        );
        // A bad scale factor means "as is".
        assert_eq!(
            scale_rect(Rect::new(1, 2, 3, 4), f64::NAN),
            Rect::new(1, 2, 3, 4)
        );
        assert_eq!(
            scale_rect(Rect::new(1, 2, 3, 4), 0.0),
            Rect::new(1, 2, 3, 4)
        );
    }

    #[test]
    fn validate_hit_region_requires_containment_and_sizes() {
        let panel = Rect::new(100, 40, 400, 60);
        assert_eq!(
            validate_hit_region(panel, Rect::new(80, 0, 440, 100)),
            Ok(())
        );
        assert_eq!(validate_hit_region(panel, panel), Ok(()));
        assert!(validate_hit_region(panel, Rect::new(120, 0, 440, 100)).is_err());
        assert!(validate_hit_region(panel, Rect::new(80, 0, 440, 90)).is_err());
        assert!(validate_hit_region(Rect::new(0, 0, -1, 10), Rect::new(0, 0, 10, 10)).is_err());
        assert!(validate_hit_region(Rect::new(0, 0, 1, 1), Rect::new(0, 0, 10, -10)).is_err());
    }

    #[test]
    fn panel_on_screen_scales_like_a_hit_region_and_offsets_by_the_window_origin() {
        // A 561 × 64 panel at (360, 168) in a window placed at (0, 1248) at 1×.
        assert_eq!(
            panel_on_screen((0, 1248), Rect::new(360, 168, 561, 64), 1.0),
            Rect::new(360, 1416, 561, 64)
        );
        // 150 %: the CSS rect rounds outwards before the offset (11 css → 16.5 → 16).
        assert_eq!(
            panel_on_screen((100, 2000), Rect::new(11, 0, 10, 8), 1.5),
            Rect::new(116, 2000, 16, 12)
        );
        // A monitor left of the primary: negative origin, kept.
        assert_eq!(
            panel_on_screen((-1920, 944), Rect::new(600, 20, 500, 70), 1.0),
            Rect::new(-1320, 964, 500, 70)
        );
        assert_eq!(validate_panel_rect(Rect::new(0, 0, 0, 0)), Ok(()));
        assert!(validate_panel_rect(Rect::new(0, 0, -1, 10)).is_err());
        assert!(validate_panel_rect(Rect::new(0, 0, 10, -1)).is_err());
    }

    #[test]
    fn native_theme_forces_light_or_dark_and_follows_windows_for_system() {
        assert_eq!(native_theme(ThemeMode::System), None);
        assert_eq!(native_theme(ThemeMode::Light), Some(tauri::Theme::Light));
        assert_eq!(native_theme(ThemeMode::Dark), Some(tauri::Theme::Dark));
    }

    #[test]
    fn a_new_secondary_dock_window_follows_the_primary_and_the_primary_always_shows() {
        // The primary's window is shown whatever is passed.
        for primary_visible in [None, Some(false), Some(true)] {
            assert!(show_new_dock_window(DOCK_LABEL, primary_visible));
        }
        // A secondary one only while the set is on screen.
        assert!(show_new_dock_window("dock-DISPLAY3", Some(true)));
        assert!(!show_new_dock_window("dock-DISPLAY3", Some(false)));
        assert!(!show_new_dock_window("dock-DISPLAY3", None));
    }

    #[test]
    fn plan_dock_rect_clamps_to_the_work_area() {
        let wa = Rect::new(-1920, 0, 1920, 1040);
        // A drag's very thick window at 200 %: the whole work area.
        assert_eq!(
            plan_dock_rect(wa, 2.0, 700.0, 1024.0, DockPosition::Bottom),
            wa
        );
        assert_eq!(
            plan_dock_rect(wa, 2.0, 1024.0, 700.0, DockPosition::Right),
            wa
        );
        // Degenerate sizes never produce a zero-sized window.
        for edge in EDGES {
            let rect = plan_dock_rect(wa, 1.0, 0.0, 0.0, edge);
            assert_on_edge(wa, edge, rect);
            let rect = plan_dock_rect(Rect::new(0, 0, 0, 0), 1.0, 10.0, 10.0, edge);
            assert_eq!((rect.width, rect.height), (1, 1), "{edge:?}");
        }
    }

    fn inside(inner: Rect, outer: Rect) -> bool {
        inner.x >= outer.x
            && inner.y >= outer.y
            && inner.right() <= outer.right()
            && inner.bottom() <= outer.bottom()
    }

    /// The window's rect after the call `step`, from its rect before.
    fn apply(step: PlaceStep, rect: Rect) -> Rect {
        match step {
            PlaceStep::Size { width, height } => Rect::new(rect.x, rect.y, width, height),
            PlaceStep::Move { x, y } => Rect::new(x, y, rect.width, rect.height),
        }
    }

    /// Run `steps` from `current` and check that every rect the window
    /// passes through lies inside `current` or `target`, and that it ends
    /// at `target`.
    fn assert_steps_stay_inside(current: Rect, target: Rect) {
        let steps = placement_steps(Some(current), target);
        assert!(steps.len() <= 3, "{steps:?}");
        let mut rect = current;
        for step in &steps {
            rect = apply(*step, rect);
            assert!(
                inside(rect, current) || inside(rect, target),
                "{current:?} -> {target:?}: {rect:?} after {step:?} of {steps:?} leaves both"
            );
            assert!(rect.width >= 1 && rect.height >= 1, "{rect:?} is empty");
        }
        assert_eq!(rect, target, "{current:?} -> {target:?} by {steps:?}");
    }

    #[test]
    fn placement_steps_never_pass_through_a_rect_outside_the_old_and_the_new_place() {
        // A 150 % secondary left of the primary, work area x -1920..0: its
        // dock goes from the right edge to the top. Resize-then-move would
        // pass through 1920x144 at x = -144, mostly on the primary.
        let secondary = Rect::new(-1920, 0, 1920, 1032);
        let right = edge_rect(secondary, DockPosition::Right, 144);
        let top = edge_rect(secondary, DockPosition::Top, 144);
        assert_eq!(right, Rect::new(-144, 0, 144, 1032));
        assert_eq!(
            placement_steps(Some(right), top),
            vec![
                PlaceStep::Size {
                    width: 144,
                    height: 144
                },
                PlaceStep::Move { x: -1920, y: 0 },
                PlaceStep::Size {
                    width: 1920,
                    height: 144
                },
            ]
        );
        // The bottom dock growing for a drag moves up first and
        // grows down into its new place, never below the work area (onto a
        // monitor under this one); it shrinks back where it stands, then
        // moves down.
        let wa = Rect::new(0, 0, 2560, 1392);
        let rest = edge_rect(wa, DockPosition::Bottom, 135);
        let grown = edge_rect(wa, DockPosition::Bottom, 1000);
        assert_eq!(
            placement_steps(Some(rest), grown),
            vec![
                PlaceStep::Move { x: 0, y: 392 },
                PlaceStep::Size {
                    width: 2560,
                    height: 1000
                },
            ]
        );
        assert_eq!(
            placement_steps(Some(grown), rest),
            vec![
                PlaceStep::Size {
                    width: 2560,
                    height: 135
                },
                PlaceStep::Move { x: 0, y: 1257 },
            ]
        );
        // A top or left dock keeps its corner: one resize; nothing to do
        // when it is already in place; a pure move is one move.
        let top_rest = edge_rect(wa, DockPosition::Top, 135);
        assert_eq!(
            placement_steps(Some(top_rest), edge_rect(wa, DockPosition::Top, 1000)),
            vec![PlaceStep::Size {
                width: 2560,
                height: 1000
            }]
        );
        assert_eq!(placement_steps(Some(rest), rest), Vec::new());
        let shifted = Rect::new(rest.x - 2560, rest.y, rest.width, rest.height);
        assert_eq!(
            placement_steps(Some(shifted), rest),
            vec![PlaceStep::Move {
                x: rest.x,
                y: rest.y
            }]
        );
        // The window's rect unknown: resize, then move.
        assert_eq!(
            placement_steps(None, rest),
            vec![
                PlaceStep::Size {
                    width: 2560,
                    height: 135
                },
                PlaceStep::Move { x: 0, y: 1257 },
            ]
        );
        // Every edge to every edge, at several thicknesses, on a monitor
        // with a negative origin and on one right of the primary, and from
        // one monitor to another (a reconcile after a new primary).
        let areas = [
            wa,
            secondary,
            Rect::new(2560, -200, 1920, 1032),
            Rect::new(0, 48, 1280, 672),
        ];
        for from_area in areas {
            for to_area in areas {
                for from in EDGES {
                    for to in EDGES {
                        for (a, b) in [(135, 135), (96, 144), (135, 1000), (1000, 135), (1, 5000)] {
                            assert_steps_stay_inside(
                                edge_rect(from_area, from, a),
                                edge_rect(to_area, to, b),
                            );
                        }
                    }
                }
            }
        }
        // A freshly created window at the initial size somewhere on screen.
        assert_steps_stay_inside(
            Rect::new(300, 200, 640, 96),
            edge_rect(wa, DockPosition::Right, 96),
        );
    }

    #[test]
    fn relayout_rect_lays_a_window_out_from_its_content_or_its_current_thickness() {
        let wa = Rect::new(0, 0, 2560, 1392);
        let bottom = DockContent {
            width: 700.0,
            height: 135.0,
            edge: DockPosition::Bottom,
        };
        let placed = edge_rect(wa, DockPosition::Bottom, 135);
        let size = (placed.width.unsigned_abs(), placed.height.unsigned_abs());
        // Bottom -> left with a content size reported on the bottom: the
        // 135 css thickness is now the width.
        assert_eq!(
            relayout_rect(
                wa,
                true,
                DockPosition::Left,
                Some(bottom),
                1.0,
                size,
                Some(DockPosition::Bottom)
            ),
            Rect::new(0, 0, 135, 1392)
        );
        // The same switch before any report: the window's height on the
        // bottom is its thickness, now its width.
        assert_eq!(
            relayout_rect(
                wa,
                true,
                DockPosition::Left,
                None,
                1.0,
                size,
                Some(DockPosition::Bottom)
            ),
            Rect::new(0, 0, 135, 1392)
        );
        // A first placement (never placed): the initial height, on any edge.
        assert_eq!(
            relayout_rect(wa, true, DockPosition::Right, None, 1.5, (960, 144), None),
            Rect::new(2416, 0, 144, 1392)
        );
        // Reported on the right, laid out again on the right at 150 %.
        let right = DockContent {
            width: 96.0,
            height: 700.0,
            edge: DockPosition::Right,
        };
        assert_eq!(
            relayout_rect(
                wa,
                true,
                DockPosition::Right,
                Some(right),
                1.5,
                (135, 1392),
                Some(DockPosition::Right)
            ),
            Rect::new(2416, 0, 144, 1392)
        );
        // No monitor list yet (Tauri's own view of the monitor): the content's
        // length is the lower bound along a vertical edge too, the origin kept.
        let small = Rect::new(0, 0, 1920, 1040);
        let tall = DockContent {
            width: 135.0,
            height: 1500.0,
            edge: DockPosition::Left,
        };
        assert_eq!(
            relayout_rect(
                small,
                false,
                DockPosition::Left,
                Some(tall),
                1.0,
                (135, 1040),
                Some(DockPosition::Left)
            ),
            Rect::new(0, 0, 135, 1500)
        );
        assert_eq!(
            relayout_rect(
                small,
                false,
                DockPosition::Right,
                Some(tall),
                1.25,
                (135, 1040),
                Some(DockPosition::Left)
            ),
            Rect::new(1751, 0, 169, 1875)
        );
        // With a monitor list the work area is the length, whatever the
        // content says.
        assert_eq!(
            relayout_rect(
                small,
                true,
                DockPosition::Left,
                Some(tall),
                1.0,
                (135, 1040),
                Some(DockPosition::Left)
            ),
            Rect::new(0, 0, 135, 1040)
        );
        // A DPI change (100 % -> 150 %) before any report: tao kept the
        // window's logical size (2560x96 -> 3840x144), so the 144 px it has
        // across its bottom edge is kept; with a report, the content's 96 css
        // gives the same 144 px.
        assert_eq!(
            relayout_rect(
                wa,
                true,
                DockPosition::Bottom,
                None,
                1.5,
                (3840, 144),
                Some(DockPosition::Bottom)
            ),
            Rect::new(0, 1248, 2560, 144)
        );
        let bottom_96 = DockContent {
            width: 700.0,
            height: 96.0,
            edge: DockPosition::Bottom,
        };
        assert_eq!(
            relayout_rect(
                wa,
                true,
                DockPosition::Bottom,
                Some(bottom_96),
                1.5,
                (3840, 144),
                Some(DockPosition::Bottom)
            ),
            Rect::new(0, 1248, 2560, 144)
        );
        // Every edge stays flush and full length, with or without content.
        for edge in EDGES {
            for content in [None, Some(bottom), Some(right), Some(tall)] {
                let rect = relayout_rect(
                    wa,
                    true,
                    edge,
                    content,
                    1.5,
                    size,
                    Some(DockPosition::Bottom),
                );
                assert_on_edge(wa, edge, rect);
            }
        }
    }

    #[test]
    fn the_panel_of_a_right_or_bottom_dock_lands_on_its_edge_on_screen() {
        // The conversions of the panel rect and the hit region are generic
        // rects: they only scale and offset by the window's own origin, which
        // on a right dock is not the work area's left. A secondary monitor
        // right of the primary at 150 %, work area 1920×1032 at (2560, 0).
        let wa = Rect::new(2560, 0, 1920, 1032);
        let window = plan_dock_rect(wa, 1.5, 96.0, 600.0, DockPosition::Right);
        assert_eq!(window, Rect::new(4336, 0, 144, 1032));
        // The panel rests against the window's right side: 64 css wide with
        // its right edge at the viewport's right (96 css), centred along.
        let css = Rect::new(32, 210, 64, 268);
        let panel = panel_on_screen((window.x, window.y), css, 1.5);
        assert_eq!(panel, Rect::new(4384, 315, 96, 402));
        assert_eq!(panel.right(), wa.right(), "flush with the screen edge");
        // The hit region stays window-local: the same scale, no offset.
        assert_eq!(scale_rect(css, 1.5), Rect::new(48, 315, 96, 402));
        // A bottom dock on a monitor left of the primary: negative origin.
        let wa = Rect::new(-1920, 0, 1920, 1040);
        let window = plan_dock_rect(wa, 1.0, 700.0, 96.0, DockPosition::Bottom);
        let panel = panel_on_screen((window.x, window.y), Rect::new(610, 26, 700, 70), 1.0);
        assert_eq!(panel, Rect::new(-1310, 970, 700, 70));
        assert_eq!(panel.bottom(), wa.bottom());
        // A top dock: the window's origin is the work area's.
        let wa = Rect::new(0, 48, 2560, 1392);
        let window = plan_dock_rect(wa, 1.0, 700.0, 96.0, DockPosition::Top);
        let panel = panel_on_screen((window.x, window.y), Rect::new(930, 0, 700, 70), 1.0);
        assert_eq!(panel, Rect::new(930, 48, 700, 70));
    }

    // --- Report provenance ---------------------------------------------------

    fn css(width: f64, height: f64) -> CssViewport {
        CssViewport { width, height }
    }

    #[test]
    fn reanchoring_keeps_the_distance_to_the_edge_side_and_the_offset_from_the_centre() {
        use DockPosition::{Bottom, Left, Right, Top};
        // A row 640 css long centred in a 2560 css window, 16 css below the
        // top of a 96 css thick one (8 above its bottom) on `bottom`, 8 css
        // below the top on `top`; a column 640 css long centred in a 1440
        // css tall window, 8 css from the edge side of a 150 css wide one.
        let row = Rect::new(960, 16, 640, 64);
        let top_row = Rect::new(960, 8, 640, 64);
        let column = Rect::new(8, 400, 64, 640);
        let right_column = Rect::new(78, 400, 64, 640);
        let wide = css(2560.0, 96.0);
        let tall = css(150.0, 1440.0);
        let cases = [
            // A thickness change: the window 48 css thicker across the edge.
            (
                Bottom,
                row,
                wide,
                css(2560.0, 144.0),
                Rect::new(960, 64, 640, 64),
            ),
            (Top, top_row, wide, css(2560.0, 144.0), top_row),
            (Left, column, tall, css(198.0, 1440.0), column),
            (
                Right,
                right_column,
                tall,
                css(198.0, 1440.0),
                Rect::new(126, 400, 64, 640),
            ),
            // Thinner again (a drop, a smaller icon size).
            (
                Bottom,
                Rect::new(960, 848, 640, 64),
                css(2560.0, 928.0),
                wide,
                row,
            ),
            (
                Right,
                Rect::new(2488, 400, 64, 640),
                css(2560.0, 1440.0),
                tall,
                right_column,
            ),
            // A length change (another work area): half of it along the edge.
            (
                Bottom,
                row,
                wide,
                css(1920.0, 96.0),
                Rect::new(640, 16, 640, 64),
            ),
            (
                Top,
                top_row,
                wide,
                css(1920.0, 96.0),
                Rect::new(640, 8, 640, 64),
            ),
            (
                Left,
                column,
                tall,
                css(150.0, 1040.0),
                Rect::new(8, 200, 64, 640),
            ),
            (
                Right,
                right_column,
                tall,
                css(150.0, 1040.0),
                Rect::new(78, 200, 64, 640),
            ),
            // Both at once.
            (
                Bottom,
                row,
                wide,
                css(1920.0, 144.0),
                Rect::new(640, 64, 640, 64),
            ),
            (
                Right,
                right_column,
                tall,
                css(198.0, 1040.0),
                Rect::new(126, 200, 64, 640),
            ),
            // Nothing changed.
            (Left, column, tall, tall, column),
        ];
        for (edge, rect, from, to, expected) in cases {
            let moved = reanchor_panel(rect, edge, from, to);
            assert_eq!(
                moved, expected,
                "{edge:?}: {rect:?} from {from:?} to {to:?}"
            );
            assert_eq!((moved.width, moved.height), (rect.width, rect.height));
            let report = PanelReport {
                rect,
                edge,
                viewport: from,
            };
            assert_eq!(report.now(Some(to)), expected);
            assert_eq!(report.now(None), rect, "an unknown viewport: as reported");
        }
    }

    #[test]
    fn reanchoring_at_150_percent_ignores_the_rounding_of_the_viewport() {
        // A bottom dock at 1.5x: a client area of 2560 x 144 px is 1706.67 x
        // 96 css; the page reports whole css px (1707 or 1706, as its
        // `innerWidth` rounds), which is the same viewport.
        let rest = CssViewport::of((2560, 144), 1.5);
        assert!((rest.width - 2560.0 / 1.5).abs() < 1e-9);
        assert_eq!(rest.height, 96.0);
        let rect = Rect::new(534, 16, 640, 64);
        for page in [css(1707.0, 96.0), css(1706.0, 96.0)] {
            assert_eq!(reanchor_panel(rect, DockPosition::Bottom, page, rest), rect);
        }
        // The magnification grows the window to 216 px (144 css): 48 css
        // lower, the same place on screen once the window moved up 72 px.
        let thicker = CssViewport::of((2560, 216), 1.5);
        let moved = reanchor_panel(rect, DockPosition::Bottom, css(1707.0, 96.0), thicker);
        assert_eq!(moved, Rect::new(534, 64, 640, 64));
        assert_eq!(
            panel_on_screen((0, 1176), moved, 1.5),
            panel_on_screen((0, 1248), rect, 1.5)
        );
        assert_eq!(
            panel_on_screen((0, 1248), rect, 1.5),
            Rect::new(801, 1272, 960, 96)
        );
        // Another monitor 2400 px wide (1600 css): half the change along.
        let narrower = CssViewport::of((2400, 144), 1.5);
        assert_eq!(
            reanchor_panel(rect, DockPosition::Bottom, css(1706.0, 96.0), narrower),
            Rect::new(481, 16, 640, 64)
        );

        // A right dock at 1.5x: 225 x 1392 px is 150 x 928 css; 288 px wide
        // is 192 css, and the column keeps its 8 css to the right side.
        let column = Rect::new(78, 144, 64, 640);
        let right = css(150.0, 928.0);
        let wider = CssViewport::of((288, 1392), 1.5);
        let moved = reanchor_panel(column, DockPosition::Right, right, wider);
        assert_eq!(moved, Rect::new(120, 144, 64, 640));
        assert_eq!(
            panel_on_screen((2272, 0), moved, 1.5),
            panel_on_screen((2335, 0), column, 1.5),
            "the same place on screen"
        );
        // A shorter work area (1032 px, 688 css): centred again along it.
        let shorter = CssViewport::of((225, 1032), 1.5);
        assert_eq!(
            reanchor_panel(column, DockPosition::Right, right, shorter),
            Rect::new(78, 24, 64, 640)
        );
    }

    #[test]
    fn a_report_for_another_edge_than_the_windows_is_dropped() {
        use DockPosition::{Bottom, Left};
        // The window was moved to the left edge; the page's reports measured
        // for the bottom edge arrive after that.
        let mut dock = DockWindowState {
            edge: Some(Left),
            ..DockWindowState::default()
        };
        let bottom_size = DockContent {
            width: 1300.0,
            height: 135.0,
            edge: Bottom,
        };
        assert_eq!(dock.take_content(bottom_size, Left), Err(Left));
        assert_eq!(dock.content, None);
        let panel = Rect::new(960, 16, 640, 64);
        let envelope = Rect::new(900, 0, 760, 80);
        assert_eq!(dock.take_hit_region(panel, envelope, Bottom), Err(Left));
        assert_eq!(dock.hit_region_css, None);
        let bottom_panel = PanelReport {
            rect: panel,
            edge: Bottom,
            viewport: css(2560.0, 96.0),
        };
        assert_eq!(dock.take_panel(bottom_panel), Err(Left));
        assert_eq!((dock.panel, dock.reserve_css), (None, None));

        // The reports measured for the left edge are taken.
        let left_size = DockContent {
            width: 135.0,
            height: 800.0,
            edge: Left,
        };
        assert_eq!(dock.take_content(left_size, Left), Ok(None));
        assert_eq!(dock.content, Some(left_size));
        let column = Rect::new(8, 400, 64, 640);
        let column_envelope = Rect::new(8, 340, 110, 760);
        assert_eq!(dock.take_hit_region(column, column_envelope, Left), Ok(()));
        assert_eq!(dock.hit_region_css, Some((column, column_envelope)));
        let left_panel = PanelReport {
            rect: column,
            edge: Left,
            viewport: css(150.0, 1440.0),
        };
        assert_eq!(dock.take_panel(left_panel), Ok(Some(72)));
        assert_eq!((dock.panel, dock.reserve_css), (Some(left_panel), Some(72)));

        // A late one from the bottom edge changes none of them.
        assert_eq!(dock.take_content(bottom_size, Left), Err(Left));
        assert_eq!(dock.take_panel(bottom_panel), Err(Left));
        assert_eq!(dock.take_hit_region(panel, envelope, Bottom), Err(Left));
        assert_eq!(dock.content, Some(left_size));
        assert_eq!((dock.panel, dock.reserve_css), (Some(left_panel), Some(72)));
        assert_eq!(dock.hit_region_css, Some((column, column_envelope)));

        // Before its first placement a window takes any edge.
        let mut fresh = DockWindowState::default();
        assert_eq!(fresh.stale_for(Bottom), None);
        assert_eq!(fresh.take_content(bottom_size, Left), Ok(None));
        assert_eq!(dock.stale_for(Left), None);
        assert_eq!(dock.stale_for(Bottom), Some(Left));
    }

    #[test]
    fn a_content_size_for_the_edge_in_effect_retries_a_failed_move() {
        use DockPosition::{Bottom, Left, Top};
        // The position became `left`, but the window's move there failed: it
        // is still on the bottom edge, with what its page reported there.
        let bottom_size = DockContent {
            width: 1300.0,
            height: 135.0,
            edge: Bottom,
        };
        let mut dock = DockWindowState {
            edge: Some(Bottom),
            content: Some(bottom_size),
            ..DockWindowState::default()
        };
        // The page's hit region and panel rect for the left edge do not
        // belong to a window on the bottom edge: dropped, as before.
        let column = Rect::new(8, 400, 64, 640);
        let column_envelope = Rect::new(8, 340, 110, 760);
        let left_panel = PanelReport {
            rect: column,
            edge: Left,
            viewport: css(150.0, 1440.0),
        };
        assert_eq!(
            dock.take_hit_region(column, column_envelope, Left),
            Err(Bottom)
        );
        assert_eq!(dock.take_panel(left_panel), Err(Bottom));
        // Its content size for the left edge, the edge in effect, is taken,
        // with the edge the window is still on: the layout that follows is
        // the move, retried.
        let left_size = DockContent {
            width: 135.0,
            height: 800.0,
            edge: Left,
        };
        assert_eq!(dock.take_content(left_size, Left), Ok(Some(Bottom)));
        assert_eq!(dock.content, Some(left_size));
        // A late report from an edge that is neither stays dropped.
        let top_size = DockContent {
            edge: Top,
            ..bottom_size
        };
        assert_eq!(dock.take_content(top_size, Left), Err(Bottom));
        assert_eq!(dock.content, Some(left_size));
        // Once that layout has placed the window (`place` remembers the
        // edge), the left edge's other reports are taken and the bottom
        // edge's are the stale ones.
        dock.edge = Some(Left);
        assert_eq!(dock.take_hit_region(column, column_envelope, Left), Ok(()));
        assert_eq!(dock.take_panel(left_panel), Ok(Some(72)));
        assert_eq!(dock.take_content(bottom_size, Left), Err(Left));
        assert_eq!(dock.take_content(left_size, Left), Ok(None));
    }

    #[test]
    fn the_band_is_read_against_the_viewport_the_report_carries() {
        let mut dock = DockWindowState {
            edge: Some(DockPosition::Bottom),
            ..DockWindowState::default()
        };
        // At rest: 80 css from the panel's top to the window's bottom.
        let rest = PanelReport {
            rect: Rect::new(960, 16, 640, 64),
            edge: DockPosition::Bottom,
            viewport: css(2560.0, 96.0),
        };
        assert_eq!(dock.take_panel(rest), Ok(Some(80)));
        // Measured in the drag's grown viewport and handled after the drop
        // shrank the window back: still 80, read against its own viewport
        // (against the shrunk window the rect would lie below its bottom).
        let grown = PanelReport {
            rect: Rect::new(960, 848, 640, 64),
            viewport: css(2560.0, 928.0),
            ..rest
        };
        assert_eq!(dock.take_panel(grown), Ok(Some(80)));
        assert_eq!(dock.reserve_css, Some(80));
        // A rect outside its own viewport (no page sends one) keeps the band.
        let outside = PanelReport {
            rect: Rect::new(960, 100, 640, 64),
            ..rest
        };
        assert_eq!(dock.take_panel(outside), Ok(None));
        assert_eq!(dock.reserve_css, Some(80));
        assert_eq!(dock.panel, Some(outside));

        // A top dock at 1.5x: the page's 1707 x 96 viewport, the panel 8 css
        // from the top: 72 css.
        let mut top = DockWindowState {
            edge: Some(DockPosition::Top),
            ..DockWindowState::default()
        };
        let report = PanelReport {
            rect: Rect::new(534, 8, 640, 64),
            edge: DockPosition::Top,
            viewport: css(1707.0, 96.0),
        };
        assert_eq!(top.take_panel(report), Ok(Some(72)));
    }

    #[test]
    fn a_viewport_must_be_positive_and_finite() {
        assert_eq!(css(1280.0, 240.0).validate(), Ok(()));
        assert_eq!(css(0.5, 0.5).validate(), Ok(()));
        for bad in [
            css(0.0, 240.0),
            css(1280.0, 0.0),
            css(-1.0, 240.0),
            css(f64::NAN, 240.0),
            css(1280.0, f64::INFINITY),
            css(f64::NEG_INFINITY, 240.0),
        ] {
            assert!(bad.validate().is_err(), "{bad:?}");
        }
        // The window's own viewport; an unusable scale counts as 1.
        assert_eq!(CssViewport::of((300, 150), 2.0), css(150.0, 75.0));
        assert_eq!(CssViewport::of((300, 150), f64::NAN), css(300.0, 150.0));
        assert_eq!(CssViewport::of((300, 150), 0.0), css(300.0, 150.0));
        assert_eq!(css(1706.6, 95.5).whole(), (1707, 96));
        // The payload as the page sends it.
        let parsed: CssViewport =
            serde_json::from_str(r#"{ "width": 1280, "height": 240 }"#).expect("parses");
        assert_eq!(parsed, css(1280.0, 240.0));
    }

    fn window_on(id: u64, monitor: Option<&str>) -> stem_core::WindowInfo {
        stem_core::WindowInfo {
            id: WindowId(id),
            title: format!("window {id}"),
            is_minimized: false,
            is_active: false,
            monitor: monitor.map(|m| MonitorId(m.into())),
            icon_key: None,
            needs_attention: false,
        }
    }

    fn app_item(key: &str, windows: Vec<stem_core::WindowInfo>) -> stem_core::DockItem {
        stem_core::DockItem {
            key: key.into(),
            kind: stem_core::DockItemKind::App,
            app_id: None,
            title: key.into(),
            icon_key: None,
            pinned: false,
            windows,
            is_active: false,
            needs_attention: false,
            badge: None,
        }
    }

    #[test]
    fn the_window_in_front_is_found_on_its_monitor_in_whichever_item_shows_it() {
        let state = DockState {
            items: vec![
                app_item("app:a", vec![window_on(1, Some(r"\\.\DISPLAY1"))]),
                app_item(
                    "app:b",
                    vec![
                        window_on(2, Some(r"\\.\DISPLAY1")),
                        window_on(3, Some(r"\\.\DISPLAY3")),
                    ],
                ),
                app_item("app:c", vec![window_on(4, None)]),
            ],
            foreground: Some(WindowId(3)),
            ..DockState::default()
        };
        assert_eq!(
            window_monitor(&state, WindowId(3)),
            Some(MonitorId(r"\\.\DISPLAY3".into()))
        );
        assert_eq!(
            window_monitor(&state, WindowId(1)),
            Some(MonitorId(r"\\.\DISPLAY1".into()))
        );
        // No monitor known, or no item shows the window.
        assert_eq!(window_monitor(&state, WindowId(4)), None);
        assert_eq!(window_monitor(&state, WindowId(9)), None);
    }

    #[test]
    fn the_focus_goes_to_the_dock_on_the_monitor_of_the_window_in_front() {
        let primary = MonitorId(r"\\.\DISPLAY1".into());
        let other = MonitorId(r"\\.\DISPLAY3".into());
        let unplugged = MonitorId(r"\\.\DISPLAY9".into());
        let docks = vec![
            (DOCK_LABEL.to_owned(), Some(primary.clone())),
            ("dock-DISPLAY3".to_owned(), Some(other.clone())),
        ];
        assert_eq!(
            focus_target(MonitorMode::All, Some(&other), &docks),
            "dock-DISPLAY3"
        );
        assert_eq!(
            focus_target(MonitorMode::All, Some(&primary), &docks),
            DOCK_LABEL
        );
        // A monitor without a dock, or no monitor known: the primary's.
        assert_eq!(
            focus_target(MonitorMode::All, Some(&unplugged), &docks),
            DOCK_LABEL
        );
        assert_eq!(focus_target(MonitorMode::All, None, &docks), DOCK_LABEL);
        // With the dock on the primary monitor only, always the primary's,
        // even while a closing secondary window is still listed.
        assert_eq!(
            focus_target(MonitorMode::Primary, Some(&other), &docks),
            DOCK_LABEL
        );
        // No dock window known at all (the set is being rebuilt).
        assert_eq!(
            focus_target(MonitorMode::All, Some(&other), &[]),
            DOCK_LABEL
        );
        // A secondary whose monitor is not known yet is never picked by it.
        let unplaced = vec![
            (DOCK_LABEL.to_owned(), None),
            ("dock-DISPLAY3".to_owned(), None),
        ];
        assert_eq!(
            focus_target(MonitorMode::All, Some(&other), &unplaced),
            DOCK_LABEL
        );
    }

    // --- The focus shortcut's session ---

    const OTHER_DOCK: &str = "dock-DISPLAY3";

    fn app(id: u64) -> Option<ReturnTo> {
        Some(ReturnTo::App(WindowId(id)))
    }

    fn session(dock: &str, window: Option<u64>) -> Option<FocusSession> {
        Some(FocusSession {
            dock: dock.to_owned(),
            window: window.map(|id| ReturnTo::App(WindowId(id))),
        })
    }

    #[test]
    fn a_release_takes_the_session_of_its_own_dock_and_says_what_to_bring_back() {
        let focus = DockFocus::default();
        assert_eq!(
            focus.release(DOCK_LABEL, Some(DOCK_LABEL)),
            Release::NoSession
        );

        focus.begin(DOCK_LABEL, app(7));
        assert_eq!(focus.session(), session(DOCK_LABEL, Some(7)));
        assert_eq!(
            focus.release(DOCK_LABEL, Some(DOCK_LABEL)),
            Release::Window(ReturnTo::App(WindowId(7)))
        );
        // Taken: a second Escape has nothing to give back.
        assert_eq!(focus.session(), None);
        assert_eq!(
            focus.release(DOCK_LABEL, Some(DOCK_LABEL)),
            Release::NoSession
        );

        // Nothing was in front at the press (the desktop, Start).
        focus.begin(DOCK_LABEL, None);
        assert_eq!(
            focus.release(DOCK_LABEL, Some(DOCK_LABEL)),
            Release::Nothing
        );
        assert_eq!(focus.session(), None);

        // The settings window was in front at the press; its own dock's
        // release takes it whether or not the dock is still in front.
        focus.begin(DOCK_LABEL, Some(ReturnTo::Own(SETTINGS_LABEL.to_owned())));
        assert_eq!(
            focus.release(DOCK_LABEL, None),
            Release::Window(ReturnTo::Own(SETTINGS_LABEL.to_owned()))
        );
        assert_eq!(focus.session(), None);
    }

    #[test]
    fn a_release_from_another_dock_drops_a_stale_session_and_brings_nothing_back() {
        let focus = DockFocus::default();
        // The session's dock is not in front any more (its blur has not
        // arrived): stale, whichever dock is in front.
        for front in [None, Some(DOCK_LABEL)] {
            focus.begin(OTHER_DOCK, app(7));
            assert_eq!(
                focus.release(DOCK_LABEL, front),
                Release::Stale(OTHER_DOCK.to_owned())
            );
            // Dropped, so the other dock's own Escape later goes nowhere
            // either.
            assert_eq!(focus.session(), None);
            assert_eq!(
                focus.release(OTHER_DOCK, Some(OTHER_DOCK)),
                Release::NoSession
            );
        }
    }

    #[test]
    fn a_release_from_another_dock_leaves_the_session_of_the_dock_in_front_alone() {
        // Two monitors with `monitors = all`: the second press reached a
        // dock that does not hold the session (or any stray release came
        // from one) while the holder is still in front. The holder's own
        // release, in either order, still brings the window back.
        let focus = DockFocus::default();
        focus.begin(OTHER_DOCK, app(7));
        assert_eq!(
            focus.release(DOCK_LABEL, Some(OTHER_DOCK)),
            Release::Held(OTHER_DOCK.to_owned())
        );
        assert_eq!(focus.session(), session(OTHER_DOCK, Some(7)));
        assert_eq!(focus.holder(Some(OTHER_DOCK)), Some(OTHER_DOCK.to_owned()));
        assert_eq!(
            focus.release(OTHER_DOCK, Some(OTHER_DOCK)),
            Release::Window(ReturnTo::App(WindowId(7)))
        );
        // The late one finds nothing.
        assert_eq!(
            focus.release(DOCK_LABEL, Some(OTHER_DOCK)),
            Release::NoSession
        );
    }

    #[test]
    fn a_later_press_replaces_the_session_whichever_dock_it_picks() {
        let focus = DockFocus::default();
        focus.begin(DOCK_LABEL, app(7));
        focus.begin(OTHER_DOCK, app(8));
        assert_eq!(focus.session(), session(OTHER_DOCK, Some(8)));
        assert_eq!(
            focus.release(DOCK_LABEL, Some(DOCK_LABEL)),
            Release::Stale(OTHER_DOCK.to_owned())
        );
    }

    #[test]
    fn a_press_goes_back_to_the_settings_window_when_it_is_in_front_else_to_the_engines_window() {
        // The engine's active window is the app behind the settings window:
        // it never takes Stem's own windows for the foreground.
        assert_eq!(
            return_target(Some(SETTINGS_LABEL), Some(WindowId(7))),
            Some(ReturnTo::Own(SETTINGS_LABEL.to_owned()))
        );
        assert_eq!(
            return_target(Some(SETTINGS_LABEL), None),
            Some(ReturnTo::Own(SETTINGS_LABEL.to_owned()))
        );
        // An app in front (none of our windows is).
        assert_eq!(return_target(None, Some(WindowId(7))), app(7));
        assert_eq!(return_target(None, None), None);
        // A dock in front without the session: the engine's window, as
        // before.
        assert_eq!(return_target(Some(DOCK_LABEL), Some(WindowId(7))), app(7));
        assert_eq!(return_target(Some(OTHER_DOCK), None), None);
    }

    #[test]
    fn an_own_windows_monitor_is_the_one_with_its_bounds() {
        let left = monitor(r"\\.\DISPLAY1", false, -2560, 1.5);
        let primary = monitor(r"\\.\DISPLAY2", true, 0, 1.0);
        let monitors = vec![left.clone(), primary.clone()];
        assert_eq!(
            monitor_with_bounds(&monitors, Rect::new(-2560, 0, 2560, 1440)),
            Some(left.id)
        );
        assert_eq!(
            monitor_with_bounds(&monitors, Rect::new(0, 0, 2560, 1440)),
            Some(primary.id)
        );
        // Its work area, or a monitor the list does not know yet: none.
        assert_eq!(
            monitor_with_bounds(&monitors, Rect::new(0, 0, 2560, 1392)),
            None
        );
        assert_eq!(monitor_with_bounds(&[], Rect::new(0, 0, 2560, 1440)), None);
    }

    #[test]
    fn a_click_on_any_dock_forgets_the_remembered_window_but_keeps_the_session() {
        let focus = DockFocus::default();
        assert_eq!(focus.supersede(), None);
        focus.begin(OTHER_DOCK, app(7));
        assert_eq!(focus.supersede(), Some(OTHER_DOCK.to_owned()));
        assert_eq!(focus.session(), session(OTHER_DOCK, None));
        // Nothing more to forget.
        assert_eq!(focus.supersede(), None);
        // The dock still has the keyboard: a second press lets go, and the
        // release brings nothing back.
        assert_eq!(focus.holder(Some(OTHER_DOCK)), Some(OTHER_DOCK.to_owned()));
        assert_eq!(
            focus.release(OTHER_DOCK, Some(OTHER_DOCK)),
            Release::Nothing
        );
        assert_eq!(focus.holder(Some(OTHER_DOCK)), None);
    }

    #[test]
    fn a_press_lets_go_only_of_the_dock_in_front_that_holds_the_session() {
        let focus = DockFocus::default();
        // No session: every press gives a dock the focus, also with a dock
        // in front (after an Escape that had nothing to go back to).
        assert_eq!(focus.holder(None), None);
        assert_eq!(focus.holder(Some(DOCK_LABEL)), None);

        focus.begin(DOCK_LABEL, app(7));
        assert_eq!(focus.holder(Some(DOCK_LABEL)), Some(DOCK_LABEL.to_owned()));
        // Not in front (a blur that has not arrived yet), or another dock
        // in front: the press gives the focus again.
        assert_eq!(focus.holder(None), None);
        assert_eq!(focus.holder(Some(OTHER_DOCK)), None);
        // Asking changes nothing.
        assert_eq!(focus.session(), session(DOCK_LABEL, Some(7)));

        // Nothing was in front at the press: the second press still lets
        // go; after the release the third one enters again.
        focus.begin(DOCK_LABEL, None);
        assert_eq!(focus.holder(Some(DOCK_LABEL)), Some(DOCK_LABEL.to_owned()));
        assert_eq!(
            focus.release(DOCK_LABEL, Some(DOCK_LABEL)),
            Release::Nothing
        );
        assert_eq!(focus.holder(Some(DOCK_LABEL)), None);
    }

    #[test]
    fn only_the_docks_own_close_or_failed_activation_ends_its_session() {
        let focus = DockFocus::default();
        assert!(!focus.end(DOCK_LABEL));
        focus.begin(DOCK_LABEL, app(7));
        assert!(!focus.end(OTHER_DOCK));
        assert_eq!(focus.session(), session(DOCK_LABEL, Some(7)));
        assert!(focus.end(DOCK_LABEL));
        assert_eq!(focus.session(), None);
    }

    #[test]
    fn a_blur_ends_the_session_only_once_its_dock_is_no_longer_in_front() {
        let focus = DockFocus::default();
        focus.begin(DOCK_LABEL, app(7));
        // Its own native menu took the focus: the dock is still in front.
        assert!(!focus.blurred(DOCK_LABEL, Some(DOCK_LABEL)));
        assert_eq!(focus.session(), session(DOCK_LABEL, Some(7)));
        // Another dock's blur leaves it alone, wherever the foreground is.
        assert!(!focus.blurred(OTHER_DOCK, None));
        assert!(!focus.blurred(OTHER_DOCK, Some(DOCK_LABEL)));
        assert_eq!(focus.session(), session(DOCK_LABEL, Some(7)));
        // Another dock took the foreground (its menu), or no dock has it (a
        // click elsewhere, Alt+Tab): the session ends.
        assert!(focus.blurred(DOCK_LABEL, Some(OTHER_DOCK)));
        assert_eq!(focus.session(), None);
        focus.begin(DOCK_LABEL, None);
        assert!(focus.blurred(DOCK_LABEL, None));
        assert_eq!(focus.session(), None);
        assert!(!focus.blurred(DOCK_LABEL, None), "nothing left to end");
    }

    #[test]
    fn a_failed_activation_ends_the_session_and_tells_the_page_nothing() {
        let focus = DockFocus::default();
        focus.begin(DOCK_LABEL, app(7));
        assert!(activated(&focus, DOCK_LABEL, Ok(())));
        assert_eq!(focus.session(), session(DOCK_LABEL, Some(7)));
        assert!(!activated(
            &focus,
            DOCK_LABEL,
            Err(PlatformError::Unsupported("keyboard focus"))
        ));
        assert_eq!(focus.session(), None);
        // A later press gave another dock the focus meanwhile: its session
        // stays.
        focus.begin(OTHER_DOCK, app(8));
        assert!(!activated(
            &focus,
            DOCK_LABEL,
            Err(PlatformError::Unsupported("keyboard focus"))
        ));
        assert_eq!(focus.session(), session(OTHER_DOCK, Some(8)));
    }

    #[test]
    fn giving_the_focus_back_activates_the_remembered_window_once_and_only_for_its_dock() {
        use stem_platform::mock::{MockCall, MockPlatform};

        let platform = MockPlatform::with_demo_scenario();
        let focus = DockFocus::default();
        let activate = |id| MockCall::WindowAction(WindowId(id), WindowAction::Activate);
        let front = Some(DOCK_LABEL);
        let no_own = |own: &str| -> Result<(), String> { panic!("no own window to focus: {own}") };

        // Escape after a press from window 1.
        focus.begin(DOCK_LABEL, app(1));
        assert_eq!(
            give_focus_back(&focus, &platform, DOCK_LABEL, front, no_own),
            Ok(())
        );
        assert_eq!(platform.calls(), vec![activate(1)]);
        // A second Escape, and one with nothing in front at the press, or
        // from another dock than the session's, activate nothing.
        platform.clear_calls();
        assert_eq!(
            give_focus_back(&focus, &platform, DOCK_LABEL, front, no_own),
            Ok(())
        );
        focus.begin(DOCK_LABEL, None);
        assert_eq!(
            give_focus_back(&focus, &platform, DOCK_LABEL, front, no_own),
            Ok(())
        );
        focus.begin(OTHER_DOCK, app(1));
        assert_eq!(
            give_focus_back(&focus, &platform, DOCK_LABEL, front, no_own),
            Ok(())
        );
        assert_eq!(platform.calls(), vec![]);
        assert_eq!(focus.session(), None);

        // The remembered window closed meanwhile: the foreground stays, and
        // the command still resolves.
        focus.begin(DOCK_LABEL, app(99));
        assert_eq!(
            give_focus_back(&focus, &platform, DOCK_LABEL, front, no_own),
            Ok(())
        );
        assert_eq!(platform.calls(), vec![activate(99)]);
    }

    #[test]
    fn a_release_from_a_dock_without_the_keyboard_never_loses_the_holders_window() {
        use stem_platform::mock::{MockCall, MockPlatform};

        // `monitors = all`: the holder is in front, and another dock's
        // page calls `release_dock_focus` first (the race of a second press
        // whose `dock://release-focus` every page took).
        let platform = MockPlatform::with_demo_scenario();
        let focus = DockFocus::default();
        let no_own = |own: &str| -> Result<(), String> { panic!("no own window to focus: {own}") };
        focus.begin(OTHER_DOCK, app(1));
        assert_eq!(
            give_focus_back(&focus, &platform, DOCK_LABEL, Some(OTHER_DOCK), no_own),
            Ok(())
        );
        assert_eq!(platform.calls(), vec![]);
        assert_eq!(
            give_focus_back(&focus, &platform, OTHER_DOCK, Some(OTHER_DOCK), no_own),
            Ok(())
        );
        assert_eq!(
            platform.calls(),
            vec![MockCall::WindowAction(WindowId(1), WindowAction::Activate)]
        );
    }

    #[test]
    fn giving_the_focus_back_to_the_settings_window_focuses_it_and_activates_no_app() {
        use stem_platform::mock::MockPlatform;

        let platform = MockPlatform::with_demo_scenario();
        let focus = DockFocus::default();
        let focused = std::cell::RefCell::new(Vec::<String>::new());
        let focus_own = |own: &str| -> Result<(), String> {
            focused.borrow_mut().push(own.to_owned());
            Ok(())
        };
        focus.begin(DOCK_LABEL, Some(ReturnTo::Own(SETTINGS_LABEL.to_owned())));
        assert_eq!(
            give_focus_back(&focus, &platform, DOCK_LABEL, Some(DOCK_LABEL), focus_own),
            Ok(())
        );
        assert_eq!(*focused.borrow(), vec![SETTINGS_LABEL.to_owned()]);
        assert_eq!(platform.calls(), vec![]);
        assert_eq!(focus.session(), None);

        // Its failure is the command's.
        focus.begin(DOCK_LABEL, Some(ReturnTo::Own(SETTINGS_LABEL.to_owned())));
        assert_eq!(
            give_focus_back(&focus, &platform, DOCK_LABEL, Some(DOCK_LABEL), |_| Err(
                "refused".to_owned()
            )),
            Err("refused".to_owned())
        );
    }
}
