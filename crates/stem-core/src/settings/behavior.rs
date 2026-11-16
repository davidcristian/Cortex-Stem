//! What clicks, the mouse wheel and the global shortcuts do.

use serde::{Deserialize, Serialize};

/// Default of [`Behavior::toggle_shortcut`].
pub const DEFAULT_TOGGLE_SHORTCUT: &str = "Ctrl+Alt+D";

/// Default of [`Behavior::focus_shortcut`].
pub const DEFAULT_FOCUS_SHORTCUT: &str = "Ctrl+Alt+Shift+D";

/// What clicks, the mouse wheel and the global shortcuts do, and which
/// windows the dock lists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct Behavior {
    /// A click on the icon of the app in front minimises its window when
    /// the app has one window. Off, the click brings that window to the
    /// front again. When the app in front has several windows, the click
    /// brings its next window to the front, whatever this setting.
    pub click_active_minimizes: bool,
    /// Hide windows that are on another virtual desktop.
    pub show_only_current_desktop: bool,
    /// Each monitor's dock shows only the windows on that monitor.
    pub show_only_current_monitor: bool,
    /// Show the dots between a running app's icon and the screen edge.
    pub running_indicators: bool,
    /// Let clicks outside the panel reach the desktop and the windows
    /// behind the dock. The dock window spans the whole length of its
    /// screen edge, so with this off the whole dock window takes every
    /// click, including on its empty part. Off is meant for diagnosis, not
    /// as a preference.
    pub click_through: bool,
    /// Global keyboard shortcut that shows or hides the dock from any app,
    /// like a left click on the tray icon. It is written as modifiers
    /// first, then one key, joined by `+`, such as `Ctrl+Alt+D`; letter case
    /// does not matter. The modifiers are `Ctrl`, `Alt`, `Shift` and `Super`
    /// (the Windows key). The keys are `A`..`Z`, `0`..`9`, `F1`..`F24`,
    /// `Space`, `Enter`, `Tab`, `Up`, `Down`, `Left`, `Right`, `Home`, `End`,
    /// `PageUp`, `PageDown`, `Insert`, `Delete` and punctuation such as `,`
    /// or `/`.
    /// Unless the key is one of `F1`..`F24`, the shortcut needs `Ctrl`,
    /// `Alt` or `Super`, so that ordinary typing never triggers it. Empty
    /// means no shortcut. A value that Stem cannot register (an unknown
    /// key, or one another app already holds) is kept as written, but no
    /// shortcut is active and the log has a warning.
    pub toggle_shortcut: String,
    /// Global keyboard shortcut that moves the keyboard focus into the dock
    /// from any app, like Win+T for the stock taskbar. The arrow keys then
    /// move from item to item, Enter or Space opens the focused one, and
    /// Escape (or the shortcut again) goes back to the window that was in
    /// front. The dock on that window's monitor receives the focus, or the
    /// primary monitor's dock when that monitor has no dock. Same format
    /// and rules as `toggleShortcut`. A value equal to
    /// `toggleShortcut`, ignoring case and modifier order, is not
    /// registered, and the toggle shortcut keeps those keys. Empty means no
    /// shortcut.
    pub focus_shortcut: String,
    /// What turning the mouse wheel over an app's icon does. A change
    /// applies at once.
    pub scroll_action: ScrollAction,
}

impl Default for Behavior {
    fn default() -> Self {
        Self {
            click_active_minimizes: true,
            show_only_current_desktop: true,
            show_only_current_monitor: false,
            running_indicators: true,
            click_through: true,
            toggle_shortcut: DEFAULT_TOGGLE_SHORTCUT.to_owned(),
            focus_shortcut: DEFAULT_FOCUS_SHORTCUT.to_owned(),
            scroll_action: ScrollAction::None,
        }
    }
}

/// What the mouse wheel does over an app's icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum ScrollAction {
    /// The wheel does nothing (the default).
    None,
    /// Over an app with open windows, each wheel notch brings the app's
    /// next window to the front (wheel turned towards you) or its previous
    /// one (turned away). After the last window comes the first again, and
    /// a minimised window is restored. The dock switches windows at most
    /// once every 150 ms. Only the windows that this dock lists take part,
    /// as for a click.
    CycleWindows,
}
