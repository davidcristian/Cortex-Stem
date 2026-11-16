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

mod changes;
mod create;
mod focus_release;
mod focus_session;
mod focus_shortcut;
mod focus_target;
mod hit_region;
mod layout;
mod lookup;
mod panel;
mod placement;
mod plan;
mod reconcile;
mod relayout;
mod settings_window;
mod state;
mod viewport;
mod visibility;

pub use changes::{on_monitors_changed, on_settings_changed};
pub use focus_release::{forget_dock_focus, release_dock_focus};
pub use focus_session::DockFocus;
pub use focus_shortcut::on_focus_shortcut;
pub use focus_target::non_dock_window_in_front;
pub use hit_region::{apply_hit_region, validate_hit_region};
pub use layout::apply_content_size;
pub(crate) use lookup::dock_windows;
pub use lookup::native_handle;
pub use panel::{apply_dock_panel, validate_panel_rect};
pub use reconcile::create_dock_windows;
pub(crate) use relayout::{app_bar_inputs, relayout_later};
pub use settings_window::{SettingsWindowTheme, open_settings_window};
pub use state::DockGeometry;
pub use viewport::{CssViewport, PanelReport};
pub use visibility::{DockAutoHidden, reveal_all, set_auto_hidden, toggle_dock};

pub const DOCK_LABEL: &str = "dock";
pub const SETTINGS_LABEL: &str = "settings";

#[cfg(test)]
mod tests;
