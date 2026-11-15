//! The native backdrop under the primary dock's panel, app part
//! (prototype: `docs/research/backdrop/prototype.md`).
//!
//! The prototype reads `appearance.backdrop` from the settings (the settings
//! page keeps *Acrylic* and *Mica* disabled, so the value comes from
//! `settings.json`): `acrylic` and `mica` ask the platform for a native
//! backdrop window under the **primary** dock's panel
//! (`Platform::set_dock_backdrop`), `css` for none. There is no UI or IPC
//! change: the page keeps drawing its CSS tint on top of the material.
//!
//! What it is fed, and from where:
//! - the panel at rest: every conversion of the UI's `set_dock_panel_rect`
//!   that `windows` hands to the service ([`on_panel`], from
//!   `push_dock_panel` and `sync_dock_panel`), rounded inwards
//!   ([`panel_inwards`]: the material must never stick out of the panel);
//!   `None` while the window is hidden by the tray or the toggle shortcut,
//!   and from a position change (`windows::on_position_setting`)
//!   until the UI reports the panel on the new edge.
//!   Every conversion carries the UI's rect re-anchored to the window's
//!   viewport of the moment (`windows::PanelReport::now`), so when
//!   the window grows for a drag, shrinks back, or takes another thickness
//!   (which on `top` and `left` leaves the UI's rect as it was, and the UI
//!   does not report it again), the material follows the panel at once;
//!   it is used only while the panel lies inside that viewport
//!   ([`panel_fits`]);
//! - auto-hide ([`on_auto_hidden`], from `set_dock_auto_hidden`): the
//!   material goes when the slide (or fade) out starts, and comes back
//!   [`REVEAL_DELAY`] after the slide in started, which is when it ends; the
//!   UI reports only the start;
//! - the settings and the theme ([`on_settings_changed`],
//!   [`on_theme_changed`], from the service's events; [`init`] at startup):
//!   the material, the corner radius, light or dark ([`dark`]) and a contrast
//!   theme, which refuses the material ([`effective`]).
//!
//! The facts and what follows from them are a small state machine
//! (`Facts`, unit-tested as sequences): each entry point changes the facts,
//! then `Facts::step` says what to log and what to ask the platform for.
//! [`wanted`] (pure) turns the facts into the request; an unchanged request
//! is not sent again, and none is sent before the window's handle is known.
//! One `info` line per change of the effective look (`css`, a material
//! requested, or a contrast theme's refusal); whether a requested material is
//! applied is the platform's line, the only verdict: the material it
//! applied, or the build or the compositor that refused it.
//!
//! Threads: the facts are a mutex in Tauri's managed state. The platform
//! call never blocks (it posts to the platform's backdrop thread) and is made
//! under the lock, so two threads cannot reorder their requests. The window's
//! native handle is taken on the main thread ([`on_panel`]), so the service
//! worker never reads a window getter.
//!
//! Known limits of the prototype: the material is sized at rest, so a
//! magnified panel grows past it until it settles; secondary docks (*All
//! monitors*) get none; the rule belongs in `stem-core`, not here.

use std::time::Duration;

use parking_lot::Mutex;
use stem_core::settings::{Backdrop, ThemeMode};
use stem_core::{Rect, Settings, ThemeInfo};
use stem_platform::{BackdropMaterial, DockBackdrop, NativeWindowHandle, PlatformError};
use stem_service::DockService;
use tauri::{AppHandle, Manager, WebviewWindow};

use crate::windows::{CssViewport, DOCK_LABEL};

/// How long the panel's slide (or fade) takes: the UI's
/// `AUTO_HIDE_SLIDE_MS` (`src/lib/dock/autohide.ts`, ADR-0008).
pub const REVEAL_DELAY: Duration = Duration::from_millis(200);

/// What the panel gets behind it, from the setting and the theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effective {
    /// The built-in CSS look; no backdrop window.
    Css,
    /// A native material in a window under the panel.
    Native(BackdropMaterial),
    /// A material was asked for, but a contrast theme is on: no material,
    /// and the panel draws its opaque contrast look.
    ContrastTheme(BackdropMaterial),
}

/// `appearance.backdrop` under the current theme. Pure.
pub fn effective(setting: Backdrop, high_contrast: bool) -> Effective {
    let material = match setting {
        Backdrop::Css => return Effective::Css,
        Backdrop::Acrylic => BackdropMaterial::Acrylic,
        Backdrop::Mica => BackdropMaterial::Mica,
    };
    if high_contrast {
        Effective::ContrastTheme(material)
    } else {
        Effective::Native(material)
    }
}

/// Whether the material is the dark one: `settings.theme` when it forces
/// light or dark, else Windows' app theme, as for the panel itself. Pure.
pub fn dark(mode: ThemeMode, theme: &ThemeInfo) -> bool {
    match mode {
        ThemeMode::Light => false,
        ThemeMode::Dark => true,
        ThemeMode::System => theme.dark,
    }
}

/// Where the panel rests, as the backdrop needs it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelPlace {
    /// Physical px of the virtual desktop, rounded inwards.
    pub rect: Rect,
    /// The window's scale factor the rect was converted at.
    pub scale: f64,
}

/// The resting panel `css` (CSS px of the viewport) in physical pixels of
/// the virtual desktop, for a window whose top-left corner is `origin`
/// (physical px) at `scale`: like `windows::panel_on_screen`, but rounded
/// **inwards** (left and top up, right and bottom down), so the material
/// never covers a pixel the panel does not. Pure.
pub fn panel_inwards(origin: (i32, i32), css: Rect, scale: f64) -> Rect {
    let scale = usable_scale(scale);
    let left = (f64::from(css.x) * scale).ceil();
    let top = (f64::from(css.y) * scale).ceil();
    let right = (f64::from(css.right()) * scale).floor();
    let bottom = (f64::from(css.bottom()) * scale).floor();
    let clamp = |v: f64| v.clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32;
    Rect::new(
        origin.0.saturating_add(clamp(left)),
        origin.1.saturating_add(clamp(top)),
        clamp((right - left).max(0.0)),
        clamp((bottom - top).max(0.0)),
    )
}

/// One conversion of the primary dock's resting panel, from `windows`
/// (`push_dock_panel`): after a panel report, a move or resize of the
/// window, or a show.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelConversion {
    /// The window's top-left corner, physical px of the virtual desktop.
    pub origin: (i32, i32),
    /// The UI's resting panel, CSS px, re-anchored to `viewport`:
    /// where the panel rests in the window now.
    pub css: Rect,
    /// The window's scale factor.
    pub scale: f64,
    /// The window's viewport now; `None` when the window could not say (and
    /// `css` is the UI's rect as reported).
    pub viewport: Option<CssViewport>,
}

/// How far (CSS px) a panel may reach past any side of its viewport and
/// still lie inside it: the UI rounds the panel outwards to whole CSS px,
/// and a client area of whole physical px is fractional in CSS px.
const INSIDE_SLACK: f64 = 1.5;

/// Whether the resting panel `css`, re-anchored to the window's viewport
/// `now`, lies inside it on both axes, so the material under it
/// covers only what the window shows of the panel: a panel longer than its
/// window along the edge, or one cut by a window thinner than it across the
/// edge, has none. (Earlier this also required the viewport to have
/// the size the report was measured in, because the rect was not
/// re-anchored: a resize took the material away until the UI reported the
/// panel again, which on `top` and `left` it does not do when only the
/// thickness changed.) Pure.
pub fn panel_fits(css: Rect, now: CssViewport) -> bool {
    f64::from(css.x) >= -INSIDE_SLACK
        && f64::from(css.right()) <= now.width + INSIDE_SLACK
        && f64::from(css.y) >= -INSIDE_SLACK
        && f64::from(css.bottom()) <= now.height + INSIDE_SLACK
}

/// The request for the platform: a backdrop only for a native material, a
/// known panel with an area, and a panel that is not slid out. The corner
/// radius is `appearance.cornerRadius` at the panel's scale. Pure.
pub fn wanted(
    effective: Effective,
    panel: Option<PanelPlace>,
    slid_out: bool,
    radius_css: u32,
    dark: bool,
) -> Option<DockBackdrop> {
    let Effective::Native(material) = effective else {
        return None;
    };
    let panel = panel?;
    if slid_out || panel.rect.width <= 0 || panel.rect.height <= 0 {
        return None;
    }
    let radius = (f64::from(radius_css) * usable_scale(panel.scale))
        .round()
        .clamp(0.0, f64::from(u32::MAX)) as u32;
    Some(DockBackdrop {
        rect: panel.rect,
        corner_radius: radius,
        material,
        dark,
    })
}

fn usable_scale(scale: f64) -> f64 {
    if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// The settings the backdrop follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Inputs {
    backdrop: Backdrop,
    radius_css: u32,
    theme_mode: ThemeMode,
}

impl Inputs {
    fn of(settings: &Settings) -> Self {
        Self {
            backdrop: settings.appearance.backdrop,
            radius_css: settings.appearance.corner_radius,
            theme_mode: settings.theme,
        }
    }
}

/// What the primary dock's backdrop is computed from, and what the platform
/// was last asked for. Kept in Tauri's managed state.
#[derive(Debug, Default)]
pub struct DockBackdropState(Mutex<Facts>);

#[derive(Debug, Default)]
struct Facts {
    /// The primary dock window, once its first panel report came.
    handle: Option<NativeWindowHandle>,
    /// `None` until [`init`].
    inputs: Option<Inputs>,
    theme: ThemeInfo,
    /// `None` before the first panel report, while the window is hidden, and
    /// while the latest conversion does not fit the window ([`panel_fits`]).
    panel: Option<PanelPlace>,
    /// Slid out by auto-hide, or sliding back in for [`REVEAL_DELAY`].
    slid_out: bool,
    /// Counts the auto-hide reports, so a delayed reveal only acts when no
    /// report came after the one that scheduled it.
    auto_hide_reports: u64,
    /// The effective look last logged.
    logged: Option<Effective>,
    /// What the platform was last asked for (`None`: no backdrop).
    sent: Option<DockBackdrop>,
}

/// What to do after a change of the [`Facts`] ([`Facts::step`]).
#[derive(Debug, Default, PartialEq)]
struct Step {
    /// The effective look changed to this: log it.
    log: Option<Effective>,
    /// Ask the platform for this backdrop (`None`: none) under this window.
    send: Option<(NativeWindowHandle, Option<DockBackdrop>)>,
}

impl Facts {
    /// A conversion of the panel, or `None` while the window is hidden. A
    /// panel that does not fit the window's viewport ([`panel_fits`]), or a
    /// window whose viewport is unknown, counts as none; then this returns
    /// `true`.
    fn panel_converted(&mut self, place: Option<PanelConversion>) -> bool {
        let Some(place) = place else {
            self.panel = None;
            return false;
        };
        let fits = place.viewport.is_some_and(|now| panel_fits(place.css, now));
        self.panel = fits.then(|| PanelPlace {
            rect: panel_inwards(place.origin, place.css, place.scale),
            scale: place.scale,
        });
        !fits
    }

    /// An auto-hide report. Out (`hidden`): the panel counts as slid out at
    /// once. Back: it still does; the returned number is the one the delayed
    /// [`Facts::reveal_due`] must carry, so that a later report overtakes it.
    fn auto_hide_reported(&mut self, hidden: bool) -> Option<u64> {
        self.auto_hide_reports = self.auto_hide_reports.wrapping_add(1);
        if hidden {
            self.slid_out = true;
            None
        } else {
            Some(self.auto_hide_reports)
        }
    }

    /// [`REVEAL_DELAY`] has passed since the report numbered `report` said
    /// the panel is coming back: it is back, unless another report came
    /// meanwhile.
    fn reveal_due(&mut self, report: u64) {
        if self.auto_hide_reports == report {
            self.slid_out = false;
        }
    }

    /// What follows from the facts: the effective look to log when it
    /// changed, and the request to send when it differs from the last one
    /// sent and the window's handle is known. Both count as done.
    fn step(&mut self) -> Step {
        let Some(inputs) = self.inputs else {
            return Step::default();
        };
        let look = effective(inputs.backdrop, self.theme.high_contrast);
        let log = (self.logged != Some(look)).then_some(look);
        self.logged = Some(look);
        let request = wanted(
            look,
            self.panel,
            self.slid_out,
            inputs.radius_css,
            dark(inputs.theme_mode, &self.theme),
        );
        let send = match self.handle {
            Some(handle) if request != self.sent => {
                self.sent = request;
                Some((handle, request))
            }
            _ => None,
        };
        Step { log, send }
    }
}

/// At startup, before the dock window exists: the settings and the theme
/// as the service has them, and one `info` line for the effective look.
pub fn init(app: &AppHandle, service: &DockService) {
    let Some(state) = app.try_state::<DockBackdropState>() else {
        return;
    };
    let mut facts = state.0.lock();
    facts.inputs = Some(Inputs::of(&service.settings()));
    facts.theme = service.theme();
    evaluate(service, &mut facts);
}

/// The settings changed. Any thread; never blocks.
pub fn on_settings_changed(app: &AppHandle, settings: &Settings) {
    update(app, |facts| facts.inputs = Some(Inputs::of(settings)));
}

/// Windows' theme changed (light or dark, a contrast theme). Any thread;
/// never blocks.
pub fn on_theme_changed(app: &AppHandle, theme: &ThemeInfo) {
    update(app, |facts| facts.theme = theme.clone());
}

/// The panel of the dock window `window` was converted for the engine
/// (`windows::push_dock_panel`): `Some` after a report, a move or resize of
/// the window or a show, with the UI's latest resting panel re-anchored to
/// the window's viewport, or `None` while the window is hidden and
/// while its page's panel rect is withdrawn (a position change, until the
/// page reports the panel on the new edge). Only the primary dock has a
/// backdrop. Main thread (the window's handle is read here).
pub fn on_panel(app: &AppHandle, window: &WebviewWindow, place: Option<PanelConversion>) {
    if window.label() != DOCK_LABEL {
        return;
    }
    let Some(state) = app.try_state::<DockBackdropState>() else {
        return;
    };
    let mut facts = state.0.lock();
    if facts.handle.is_none() && place.is_some() {
        facts.handle = crate::windows::native_handle(window);
    }
    if facts.panel_converted(place) {
        log::debug!(
            "dock backdrop: the panel {:?} css does not lie inside the window's viewport {:?}; no material until it does",
            place.map(|p| p.css),
            place.and_then(|p| p.viewport)
        );
    }
    if let Some(service) = app.try_state::<crate::AppService>() {
        evaluate(&service.0, &mut facts);
    }
}

/// The UI of `window` reported that auto-hide slid its panel out (`true`)
/// or is bringing it back (`false`). Out: the material goes at once. Back:
/// it comes [`REVEAL_DELAY`] later, unless another report came meanwhile.
/// Main thread (a command); the wait runs on Tauri's blocking pool.
pub fn on_auto_hidden(app: &AppHandle, window: &WebviewWindow, hidden: bool) {
    if window.label() != DOCK_LABEL {
        return;
    }
    let mut reveal = None;
    update(app, |facts| reveal = facts.auto_hide_reported(hidden));
    let Some(report) = reveal else {
        return;
    };
    let handle = app.clone();
    drop(tauri::async_runtime::spawn_blocking(move || {
        std::thread::sleep(REVEAL_DELAY);
        update(&handle, |facts| facts.reveal_due(report));
    }));
}

/// Change the facts and send what follows from them.
fn update(app: &AppHandle, change: impl FnOnce(&mut Facts)) {
    let (Some(state), Some(service)) = (
        app.try_state::<DockBackdropState>(),
        app.try_state::<crate::AppService>(),
    ) else {
        return;
    };
    let mut facts = state.0.lock();
    change(&mut facts);
    evaluate(&service.0, &mut facts);
}

/// Log a change of the effective look, then ask the platform for the
/// backdrop the facts call for, unless that is what it has.
fn evaluate(service: &DockService, facts: &mut Facts) {
    let step = facts.step();
    if let Some(look) = step.log {
        log_look(look);
    }
    let Some((handle, request)) = step.send else {
        return;
    };
    match service.platform().set_dock_backdrop(handle, request) {
        Ok(()) => log::trace!("dock backdrop request: {request:?}"),
        Err(PlatformError::Unsupported(task)) => {
            static LOGGED: std::sync::Once = std::sync::Once::new();
            LOGGED.call_once(|| {
                log::debug!(
                    "native backdrops are not supported by this platform (feature: {task})"
                );
            });
        }
        Err(e) => log::warn!(
            "the native backdrop could not be {}: {e}",
            if request.is_some() { "shown" } else { "hidden" }
        ),
    }
}

fn log_look(look: Effective) {
    match look {
        Effective::Css => log::info!("backdrop: css (the built-in look)"),
        Effective::Native(material) => log::info!(
            "backdrop: {} requested under the primary dock's panel (prototype; the platform logs whether it is applied)",
            material_name(material)
        ),
        Effective::ContrastTheme(material) => log::info!(
            "backdrop: {} refused: a contrast theme is on; the panel keeps its CSS look",
            material_name(material)
        ),
    }
}

fn material_name(material: BackdropMaterial) -> &'static str {
    match material {
        BackdropMaterial::Acrylic => "acrylic",
        BackdropMaterial::Mica => "mica",
    }
}

#[cfg(test)]
mod tests {
    use stem_core::settings::DockPosition;

    use super::*;
    use crate::windows::PanelReport;

    #[test]
    fn the_setting_picks_the_material_and_a_contrast_theme_refuses_it() {
        assert_eq!(effective(Backdrop::Css, false), Effective::Css);
        assert_eq!(effective(Backdrop::Css, true), Effective::Css);
        assert_eq!(
            effective(Backdrop::Acrylic, false),
            Effective::Native(BackdropMaterial::Acrylic)
        );
        assert_eq!(
            effective(Backdrop::Mica, false),
            Effective::Native(BackdropMaterial::Mica)
        );
        assert_eq!(
            effective(Backdrop::Acrylic, true),
            Effective::ContrastTheme(BackdropMaterial::Acrylic)
        );
    }

    #[test]
    fn the_material_is_dark_as_the_theme_setting_says() {
        let light = ThemeInfo {
            dark: false,
            ..ThemeInfo::default()
        };
        let night = ThemeInfo {
            dark: true,
            ..ThemeInfo::default()
        };
        assert!(!dark(ThemeMode::System, &light));
        assert!(dark(ThemeMode::System, &night));
        assert!(dark(ThemeMode::Dark, &light));
        assert!(!dark(ThemeMode::Light, &night));
    }

    #[test]
    fn the_panel_is_rounded_inwards_and_offset_by_the_window() {
        assert_eq!(
            panel_inwards((0, 1248), Rect::new(360, 168, 561, 64), 1.0),
            Rect::new(360, 1416, 561, 64)
        );
        // 11 * 1.5 = 16.5 -> 17; (11 + 10) * 1.5 = 31.5 -> 31.
        assert_eq!(
            panel_inwards((100, 2000), Rect::new(11, 0, 10, 8), 1.5),
            Rect::new(117, 2000, 14, 12)
        );
        assert_eq!(
            panel_inwards((-1920, 944), Rect::new(600, 20, 500, 70), 1.25),
            Rect::new(-1170, 969, 625, 87)
        );
        // An unusable scale counts as 1; a sliver collapses to nothing.
        assert_eq!(
            panel_inwards((0, 0), Rect::new(5, 5, 10, 10), f64::NAN),
            Rect::new(5, 5, 10, 10)
        );
        assert_eq!(
            panel_inwards((0, 0), Rect::new(1, 1, 0, 0), 1.5).width,
            0,
            "never negative"
        );
    }

    fn place() -> Option<PanelPlace> {
        Some(PanelPlace {
            rect: Rect::new(800, 1300, 960, 96),
            scale: 1.5,
        })
    }

    #[test]
    fn a_native_material_on_a_shown_panel_is_requested_at_the_panels_scale() {
        assert_eq!(
            wanted(
                Effective::Native(BackdropMaterial::Acrylic),
                place(),
                false,
                22,
                true
            ),
            Some(DockBackdrop {
                rect: Rect::new(800, 1300, 960, 96),
                corner_radius: 33,
                material: BackdropMaterial::Acrylic,
                dark: true,
            })
        );
    }

    #[test]
    fn nothing_is_requested_without_a_material_a_panel_or_while_slid_out() {
        let acrylic = Effective::Native(BackdropMaterial::Acrylic);
        assert_eq!(wanted(Effective::Css, place(), false, 22, false), None);
        assert_eq!(
            wanted(
                Effective::ContrastTheme(BackdropMaterial::Mica),
                place(),
                false,
                22,
                false
            ),
            None
        );
        assert_eq!(wanted(acrylic, None, false, 22, false), None, "hidden");
        assert_eq!(wanted(acrylic, place(), true, 22, false), None, "slid out");
        let empty = Some(PanelPlace {
            rect: Rect::new(800, 1300, 0, 96),
            scale: 1.0,
        });
        assert_eq!(wanted(acrylic, empty, false, 22, false), None);
    }

    // --- The state machine, as sequences of reports --------------------------

    const DOCK: NativeWindowHandle = NativeWindowHandle(0x1234);

    fn acrylic_inputs() -> Inputs {
        Inputs {
            backdrop: Backdrop::Acrylic,
            radius_css: 22,
            theme_mode: ThemeMode::Dark,
        }
    }

    /// The bottom dock's window at rest at 1.5x: 2560 x 144 px (1706.67 x
    /// 96 css) with its top-left corner at (0, 1248).
    const ORIGIN: (i32, i32) = (0, 1248);
    const REST: (u32, u32) = (2560, 144);

    /// The page's report at rest: 640 x 64 css, 16 css below the window's
    /// top (8 above its bottom), measured in the page's 1707 x 96 viewport
    /// (`innerWidth` is whole css px).
    fn report() -> PanelReport {
        PanelReport {
            rect: Rect::new(534, 16, 640, 64),
            edge: DockPosition::Bottom,
            viewport: CssViewport {
                width: 1707.0,
                height: 96.0,
            },
        }
    }

    /// What `windows::push_dock_panel` hands over for `report` in a window
    /// whose top-left corner is `origin` and whose client area is
    /// `physical` px at `scale`: the rect re-anchored to that viewport.
    fn converted(
        report: PanelReport,
        origin: (i32, i32),
        physical: (u32, u32),
        scale: f64,
    ) -> Option<PanelConversion> {
        let viewport = CssViewport::of(physical, scale);
        Some(PanelConversion {
            origin,
            css: report.now(Some(viewport)),
            scale,
            viewport: Some(viewport),
        })
    }

    /// The panel at rest (960 x 96 px at 1.5x).
    fn at_rest() -> Option<PanelConversion> {
        converted(report(), ORIGIN, REST, 1.5)
    }

    fn backdrop_at_rest() -> DockBackdrop {
        DockBackdrop {
            rect: Rect::new(801, 1272, 960, 96),
            corner_radius: 33,
            material: BackdropMaterial::Acrylic,
            dark: true,
        }
    }

    /// Facts with acrylic, the window's handle and its panel shown at rest,
    /// the first request already sent.
    fn shown() -> Facts {
        let mut facts = Facts {
            inputs: Some(acrylic_inputs()),
            handle: Some(DOCK),
            ..Facts::default()
        };
        assert!(!facts.panel_converted(at_rest()));
        assert_eq!(
            facts.step().send,
            Some((DOCK, Some(backdrop_at_rest()))),
            "set-up"
        );
        facts
    }

    #[test]
    fn nothing_is_logged_or_sent_before_the_settings_and_the_handle_arrive() {
        let mut facts = Facts::default();
        facts.panel_converted(at_rest());
        assert_eq!(facts.step(), Step::default(), "no settings yet");

        facts.inputs = Some(acrylic_inputs());
        assert_eq!(
            facts.step(),
            Step {
                log: Some(Effective::Native(BackdropMaterial::Acrylic)),
                send: None,
            },
            "the look is logged, but there is no window to ask for"
        );
        assert_eq!(facts.step(), Step::default(), "logged once");

        facts.handle = Some(DOCK);
        assert_eq!(
            facts.step().send,
            Some((DOCK, Some(backdrop_at_rest()))),
            "the handle arrives after the inputs: the request goes now"
        );
        assert_eq!(
            facts.step(),
            Step::default(),
            "an unchanged request is not sent again"
        );
    }

    #[test]
    fn the_panel_moving_or_the_window_hiding_sends_the_new_request_once() {
        let mut facts = shown();
        facts.panel_converted(converted(report(), (0, 1200), REST, 1.5));
        let moved = DockBackdrop {
            rect: Rect::new(801, 1224, 960, 96),
            ..backdrop_at_rest()
        };
        assert_eq!(facts.step().send, Some((DOCK, Some(moved))));
        facts.panel_converted(converted(report(), (0, 1200), REST, 1.5));
        assert_eq!(facts.step().send, None, "the same place again");

        facts.panel_converted(None);
        assert_eq!(facts.step().send, Some((DOCK, None)), "window hidden");
        assert_eq!(facts.step().send, None);
        facts.panel_converted(at_rest());
        assert_eq!(facts.step().send, Some((DOCK, Some(backdrop_at_rest()))));
    }

    #[test]
    fn a_panel_fits_when_it_lies_inside_the_viewport() {
        let rest = CssViewport::of(REST, 1.5);
        assert!(panel_fits(Rect::new(534, 16, 640, 64), rest));
        // Rounded outwards past the bottom edge by less than a px: fits.
        assert!(panel_fits(Rect::new(534, 32, 640, 65), rest));
        // Below the viewport's bottom edge, or above its top.
        assert!(!panel_fits(Rect::new(534, 848, 640, 64), rest));
        assert!(!panel_fits(Rect::new(534, -8, 640, 64), rest));
        // A row longer than its window: material would stick out of it.
        assert!(!panel_fits(Rect::new(-40, 16, 2600, 64), rest));

        // A right dock: a column 8 css from the window's right side.
        let right = CssViewport {
            width: 150.0,
            height: 928.0,
        };
        assert!(panel_fits(Rect::new(78, 144, 64, 640), right));
        assert!(!panel_fits(Rect::new(1572, 144, 64, 640), right));
        assert!(!panel_fits(Rect::new(-8, 144, 64, 640), right));
    }

    /// A magnification change makes the window
    /// thicker (144 px to 216 px across the edge at 1.5x) while the panel
    /// keeps its size. On `top` and `left` the page's rect stays the same,
    /// so the page does not report it again; read against the resized
    /// window without re-anchoring, that rect counted as stale and the
    /// material was gone until the rect changed. Re-anchored, the panel is
    /// where it was on screen on every edge, and the material stays.
    #[test]
    fn a_thickness_change_keeps_the_material_on_every_edge() {
        struct Case {
            edge: DockPosition,
            /// The page's rect and viewport at rest.
            rect: Rect,
            viewport: (f64, f64),
            /// The window before and after (origin, client area px).
            before: ((i32, i32), (u32, u32)),
            after: ((i32, i32), (u32, u32)),
            /// The material, the same before and after.
            material: Rect,
        }
        let cases = [
            Case {
                edge: DockPosition::Top,
                rect: Rect::new(534, 8, 640, 64),
                viewport: (1707.0, 96.0),
                before: ((0, 0), (2560, 144)),
                after: ((0, 0), (2560, 216)),
                material: Rect::new(801, 12, 960, 96),
            },
            Case {
                edge: DockPosition::Left,
                rect: Rect::new(8, 144, 64, 640),
                viewport: (150.0, 928.0),
                before: ((0, 0), (225, 1392)),
                after: ((0, 0), (288, 1392)),
                material: Rect::new(12, 216, 96, 960),
            },
            Case {
                edge: DockPosition::Bottom,
                rect: Rect::new(534, 16, 640, 64),
                viewport: (1707.0, 96.0),
                before: ((0, 1248), (2560, 144)),
                after: ((0, 1176), (2560, 216)),
                material: Rect::new(801, 1272, 960, 96),
            },
            Case {
                edge: DockPosition::Right,
                rect: Rect::new(78, 144, 64, 640),
                viewport: (150.0, 928.0),
                before: ((2335, 0), (225, 1392)),
                after: ((2272, 0), (288, 1392)),
                material: Rect::new(2452, 216, 96, 960),
            },
        ];
        for case in cases {
            let edge = case.edge;
            let report = PanelReport {
                rect: case.rect,
                edge,
                viewport: CssViewport {
                    width: case.viewport.0,
                    height: case.viewport.1,
                },
            };
            let mut facts = Facts {
                inputs: Some(acrylic_inputs()),
                handle: Some(DOCK),
                ..Facts::default()
            };
            let (origin, size) = case.before;
            assert!(!facts.panel_converted(converted(report, origin, size, 1.5)));
            let material = DockBackdrop {
                rect: case.material,
                ..backdrop_at_rest()
            };
            assert_eq!(facts.step().send, Some((DOCK, Some(material))), "{edge:?}");

            // The resize (`windows::place`) converts the same report again.
            let (origin, size) = case.after;
            assert!(
                !facts.panel_converted(converted(report, origin, size, 1.5)),
                "{edge:?}: the material stays"
            );
            assert_eq!(facts.step().send, None, "{edge:?}: in the same place");
            assert_eq!(facts.panel.map(|p| p.rect), Some(case.material), "{edge:?}");
        }
    }

    #[test]
    fn a_drag_and_the_drop_keep_the_material_under_the_panel() {
        let mut facts = shown();
        // The drag grows the window to the work area (1392 px) with its
        // bottom edge fixed: the report at rest, re-anchored, is 832 css
        // lower in the grown viewport, the same place on screen.
        let grown = ((0, 0), (2560, 1392));
        assert!(!facts.panel_converted(converted(report(), grown.0, grown.1, 1.5)));
        assert_eq!(facts.step().send, None, "the material stays put");

        // The page reports the panel in the grown viewport.
        let in_grown = PanelReport {
            rect: Rect::new(534, 848, 640, 64),
            viewport: CssViewport {
                width: 1707.0,
                height: 928.0,
            },
            ..report()
        };
        assert!(!facts.panel_converted(converted(in_grown, grown.0, grown.1, 1.5)));
        assert_eq!(facts.step().send, None);

        // The drop shrinks the window back before that report is replaced:
        // re-anchored to the short viewport, it is the panel at rest.
        assert!(!facts.panel_converted(converted(in_grown, ORIGIN, REST, 1.5)));
        assert_eq!(facts.step().send, None);
        assert_eq!(
            facts.panel.map(|p| p.rect),
            Some(backdrop_at_rest().rect),
            "where the panel rests"
        );

        // A right dock at 1x: grown to 1920 px with its right side fixed.
        let right = PanelReport {
            rect: Rect::new(78, 144, 64, 640),
            edge: DockPosition::Right,
            viewport: CssViewport {
                width: 150.0,
                height: 1040.0,
            },
        };
        let mut facts = Facts {
            inputs: Some(acrylic_inputs()),
            handle: Some(DOCK),
            ..Facts::default()
        };
        assert!(!facts.panel_converted(converted(right, (1770, 0), (150, 1040), 1.0)));
        let resting = DockBackdrop {
            rect: Rect::new(1848, 144, 64, 640),
            corner_radius: 22,
            ..backdrop_at_rest()
        };
        assert_eq!(facts.step().send, Some((DOCK, Some(resting))));
        assert!(!facts.panel_converted(converted(right, (0, 0), (1920, 1040), 1.0)));
        assert_eq!(facts.step().send, None, "grown: the same place on screen");
        let in_grown = PanelReport {
            rect: Rect::new(1848, 144, 64, 640),
            viewport: CssViewport {
                width: 1920.0,
                height: 1040.0,
            },
            ..right
        };
        assert!(!facts.panel_converted(converted(in_grown, (1770, 0), (150, 1040), 1.0)));
        assert_eq!(facts.step().send, None, "dropped: still the same place");
    }

    #[test]
    fn no_material_while_the_viewport_is_unknown() {
        let mut facts = shown();
        let blind = Some(PanelConversion {
            origin: ORIGIN,
            css: report().now(None),
            scale: 1.5,
            viewport: None,
        });
        assert!(facts.panel_converted(blind));
        assert_eq!(facts.step().send, Some((DOCK, None)));
        assert!(!facts.panel_converted(at_rest()));
        assert_eq!(facts.step().send, Some((DOCK, Some(backdrop_at_rest()))));
    }

    #[test]
    fn the_material_goes_with_the_slide_out_and_comes_back_when_the_reveal_is_due() {
        let mut facts = shown();
        assert_eq!(facts.auto_hide_reported(true), None);
        assert_eq!(facts.step().send, Some((DOCK, None)), "out: at once");

        let reveal = facts.auto_hide_reported(false);
        assert!(reveal.is_some());
        assert_eq!(facts.step().send, None, "back: not before the delay");
        facts.reveal_due(reveal.unwrap_or_default());
        assert_eq!(facts.step().send, Some((DOCK, Some(backdrop_at_rest()))));
    }

    #[test]
    fn a_reveal_overtaken_by_a_newer_report_leaves_the_panel_slid_out() {
        let mut facts = shown();
        facts.auto_hide_reported(true);
        assert_eq!(facts.step().send, Some((DOCK, None)));

        // Out, back and out again within the delay: the first reveal is stale.
        let stale = facts.auto_hide_reported(false).unwrap_or_default();
        assert_eq!(facts.auto_hide_reported(true), None);
        facts.reveal_due(stale);
        assert!(facts.slid_out);
        assert_eq!(facts.step().send, None, "still slid out: nothing to send");

        // Back twice within the delay: only the second reveal acts.
        let first = facts.auto_hide_reported(false).unwrap_or_default();
        let second = facts.auto_hide_reported(false).unwrap_or_default();
        facts.reveal_due(first);
        assert_eq!(facts.step().send, None, "overtaken by the second report");
        facts.reveal_due(second);
        assert_eq!(facts.step().send, Some((DOCK, Some(backdrop_at_rest()))));
    }

    #[test]
    fn a_settings_or_theme_change_while_slid_out_shows_nothing_until_the_reveal() {
        let mut facts = shown();
        facts.auto_hide_reported(true);
        assert_eq!(facts.step().send, Some((DOCK, None)));

        facts.inputs = Some(Inputs {
            backdrop: Backdrop::Mica,
            radius_css: 8,
            ..acrylic_inputs()
        });
        assert_eq!(
            facts.step(),
            Step {
                log: Some(Effective::Native(BackdropMaterial::Mica)),
                send: None,
            },
            "the new look is logged; the panel is still out"
        );
        facts.theme.dark = false;
        assert_eq!(facts.step(), Step::default());

        let reveal = facts.auto_hide_reported(false).unwrap_or_default();
        facts.reveal_due(reveal);
        let mica = DockBackdrop {
            corner_radius: 12,
            material: BackdropMaterial::Mica,
            ..backdrop_at_rest()
        };
        assert_eq!(facts.step().send, Some((DOCK, Some(mica))));
    }

    #[test]
    fn a_contrast_theme_or_the_css_look_takes_the_material_away_and_logs_why() {
        let mut facts = shown();
        facts.theme.high_contrast = true;
        assert_eq!(
            facts.step(),
            Step {
                log: Some(Effective::ContrastTheme(BackdropMaterial::Acrylic)),
                send: Some((DOCK, None)),
            }
        );
        facts.theme.high_contrast = false;
        assert_eq!(
            facts.step(),
            Step {
                log: Some(Effective::Native(BackdropMaterial::Acrylic)),
                send: Some((DOCK, Some(backdrop_at_rest()))),
            }
        );
        facts.inputs = Some(Inputs {
            backdrop: Backdrop::Css,
            ..acrylic_inputs()
        });
        assert_eq!(
            facts.step(),
            Step {
                log: Some(Effective::Css),
                send: Some((DOCK, None)),
            }
        );
    }
}
