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
//!   and from a position change (`windows::changes::on_position_setting`)
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
//!
//! [`dark`]: rules::dark
//! [`effective`]: rules::effective
//! [`panel_fits`]: rules::panel_fits
//! [`panel_inwards`]: rules::panel_inwards
//! [`wanted`]: rules::wanted

mod rules;
mod state;

use std::time::Duration;

use parking_lot::Mutex;
use stem_core::{Settings, ThemeInfo};
use stem_platform::{BackdropMaterial, PlatformError};
use stem_service::DockService;
use tauri::{AppHandle, Manager, WebviewWindow};

use rules::Effective;
pub use rules::PanelConversion;
use state::{Facts, Inputs};

use crate::windows::DOCK_LABEL;

/// How long the panel's slide (or fade) takes: the UI's
/// `AUTO_HIDE_SLIDE_MS` (`ui/src/lib/dock/autohide/state.ts`, ADR-0008).
pub const REVEAL_DELAY: Duration = Duration::from_millis(200);

/// What the primary dock's backdrop is computed from, and what the platform
/// was last asked for. Kept in Tauri's managed state.
#[derive(Debug, Default)]
pub struct DockBackdropState(Mutex<Facts>);

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
/// (`windows::panel::push_dock_panel`): `Some` after a report, a move or resize of
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
