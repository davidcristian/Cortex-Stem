//! The facts the backdrop is computed from and what follows from them.

use stem_core::settings::{Backdrop, ThemeMode};
use stem_core::{Settings, ThemeInfo};
use stem_platform::{DockBackdrop, NativeWindowHandle};

use super::rules::{Effective, PanelConversion, PanelPlace};
use super::rules::{dark, effective, panel_fits, panel_inwards, wanted};

/// The settings the backdrop follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Inputs {
    backdrop: Backdrop,
    radius_css: u32,
    theme_mode: ThemeMode,
}

impl Inputs {
    pub(super) fn of(settings: &Settings) -> Self {
        Self {
            backdrop: settings.appearance.backdrop,
            radius_css: settings.appearance.corner_radius,
            theme_mode: settings.theme,
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct Facts {
    /// The primary dock window, once its first panel report came.
    pub(super) handle: Option<NativeWindowHandle>,
    /// `None` until [`init`].
    ///
    /// [`init`]: super::init
    pub(super) inputs: Option<Inputs>,
    pub(super) theme: ThemeInfo,
    /// `None` before the first panel report, while the window is hidden, and
    /// while the latest conversion does not fit the window ([`panel_fits`]).
    panel: Option<PanelPlace>,
    /// Slid out by auto-hide, or sliding back in for [`REVEAL_DELAY`].
    ///
    /// [`REVEAL_DELAY`]: super::REVEAL_DELAY
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
pub(super) struct Step {
    /// The effective look changed to this: log it.
    pub(super) log: Option<Effective>,
    /// Ask the platform for this backdrop (`None`: none) under this window.
    pub(super) send: Option<(NativeWindowHandle, Option<DockBackdrop>)>,
}

impl Facts {
    /// A conversion of the panel, or `None` while the window is hidden. A
    /// panel that does not fit the window's viewport ([`panel_fits`]), or a
    /// window whose viewport is unknown, counts as none; then this returns
    /// `true`.
    pub(super) fn panel_converted(&mut self, place: Option<PanelConversion>) -> bool {
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
    pub(super) fn auto_hide_reported(&mut self, hidden: bool) -> Option<u64> {
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
    ///
    /// [`REVEAL_DELAY`]: super::REVEAL_DELAY
    pub(super) fn reveal_due(&mut self, report: u64) {
        if self.auto_hide_reports == report {
            self.slid_out = false;
        }
    }

    /// What follows from the facts: the effective look to log when it
    /// changed, and the request to send when it differs from the last one
    /// sent and the window's handle is known. Both count as done.
    pub(super) fn step(&mut self) -> Step {
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

#[cfg(test)]
mod tests;
