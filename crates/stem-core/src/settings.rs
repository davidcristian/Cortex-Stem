//! User settings: the stored document, its defaults, validation (clamping)
//! and version migration. The Tauri app chooses where the file lives
//! (`settings.json` in its config folder), `stem-service` writes it after
//! every change, and this module defines what it contains.
//!
//! The document and each object in it use `#[serde(default)]`, so that a
//! file written by an older Stem or edited by hand still loads: a missing
//! key takes its default. The entries of `pinned` and `hiddenApps` are the
//! exception: each needs its `id`, and a pin also needs a `launch` object
//! with the keys its `kind` requires. Unknown fields are ignored.
//!
//! The doc comments of the settings types are user-facing text. ts-rs copies
//! them into the TypeScript bindings in `src/lib/ipc/generated/`, and
//! `tests/settings_reference.rs` builds the settings reference,
//! `docs/SETTINGS.md`, from them.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::model::{AppId, LaunchTarget};

/// The document format version this build writes. Increase it when a
/// breaking change needs a migration step in [`Settings::migrate`].
pub const SETTINGS_VERSION: u32 = 1;

/// Default of [`Behavior::toggle_shortcut`].
pub const DEFAULT_TOGGLE_SHORTCUT: &str = "Ctrl+Alt+D";

/// Default of [`Behavior::focus_shortcut`].
pub const DEFAULT_FOCUS_SHORTCUT: &str = "Ctrl+Alt+Shift+D";

#[derive(Debug, Error)]
pub enum SettingsError {
    #[error("settings file is not valid JSON: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("settings version {found} is newer than supported version {supported}")]
    TooNew { found: u32, supported: u32 },
}

/// The whole settings document, as stored in `settings.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Format version of the document. Stem writes the version it
    /// supports. A document with a lower version is upgraded when it loads,
    /// and a document with a higher version is unreadable.
    pub version: u32,
    /// Screen edge the dock is on. A change applies at once: the dock
    /// windows move to the new edge.
    pub position: DockPosition,
    /// Base icon size in CSS pixels, before magnification (24..=128).
    pub icon_size: u32,
    /// Magnification of icons near the cursor.
    pub magnification: Magnification,
    /// When and how the dock hides itself.
    pub auto_hide: AutoHide,
    /// Which monitors get a dock. A change applies at once: Stem opens or
    /// closes the docks of the other monitors, and the primary monitor's
    /// dock stays.
    pub monitors: MonitorMode,
    /// Light or dark colours for the dock and the settings window.
    pub theme: ThemeMode,
    /// How the dock looks: theme preset, opacity, corner radius, backdrop,
    /// tooltips and the launch bounce.
    pub appearance: Appearance,
    /// Which fixed items (the items that are not apps) the dock shows.
    pub items: FixedItems,
    /// What clicks, the mouse wheel and the global shortcuts do, and which
    /// windows the dock lists.
    pub behavior: Behavior,
    /// Pinned apps, in dock order: left to right, or top to bottom on a
    /// dock at the left or right edge. Entries whose id is empty or only
    /// whitespace are dropped, and of several entries with the same id only
    /// the first stays.
    pub pinned: Vec<PinnedApp>,
    /// Windows integration: start at sign-in, the stock taskbar, reserved
    /// screen space and the log file.
    pub system: SystemIntegration,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            position: DockPosition::Bottom,
            icon_size: 48,
            magnification: Magnification::default(),
            auto_hide: AutoHide::default(),
            monitors: MonitorMode::Primary,
            theme: ThemeMode::System,
            appearance: Appearance::default(),
            items: FixedItems::default(),
            behavior: Behavior::default(),
            pinned: Vec::new(),
            system: SystemIntegration::default(),
        }
    }
}

impl Settings {
    /// Parse, migrate, and validate a settings document.
    pub fn from_json(json: &str) -> Result<Self, SettingsError> {
        let mut settings: Self = serde_json::from_str(json)?;
        if settings.version > SETTINGS_VERSION {
            return Err(SettingsError::TooNew {
                found: settings.version,
                supported: SETTINGS_VERSION,
            });
        }
        settings.migrate();
        settings.validate();
        Ok(settings)
    }

    pub fn to_json_pretty(&self) -> String {
        // Serialising our own plain data cannot fail: `validate` guarantees
        // every float is finite, which is the only thing JSON cannot encode.
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".to_owned())
    }

    /// Upgrade an older document in place. Each new version adds a step.
    pub fn migrate(&mut self) {
        // v0 (no version field) -> v1: nothing to do beyond defaults.
        self.version = SETTINGS_VERSION;
    }

    /// Clamp every numeric field into its supported range, replace
    /// non-finite floats with their defaults, and drop invalid pins (blank or
    /// duplicate ids; the first occurrence stays). Idempotent: validating a
    /// validated document changes nothing.
    pub fn validate(&mut self) {
        self.icon_size = self.icon_size.clamp(24, 128);
        self.magnification.max_scale = finite_or(
            self.magnification.max_scale,
            Magnification::default().max_scale,
        )
        .clamp(1.0, 2.5);
        self.magnification.range_icons = self.magnification.range_icons.clamp(1, 6);
        self.auto_hide.delay_ms = self.auto_hide.delay_ms.min(5_000);
        self.appearance.opacity =
            finite_or(self.appearance.opacity, Appearance::default().opacity).clamp(0.0, 1.0);
        self.appearance.corner_radius = self.appearance.corner_radius.min(64);
        let mut seen = std::collections::HashSet::new();
        self.pinned
            .retain(|p| !p.id.as_str().trim().is_empty() && seen.insert(p.id.clone()));
    }

    /// Whether replacing `self` by `other` changes where the dock windows
    /// are or which exist: the screen edge (the app moves every window in
    /// place) or the set of monitors that get a dock (windows are
    /// created or closed). Everything else restyles them in place.
    pub fn diff_requires_window_rebuild(&self, other: &Settings) -> bool {
        self.position != other.position || self.monitors != other.monitors
    }

    /// A realistic, fully valid document: a few pinned apps that use every
    /// [`LaunchTarget`] kind, and a handful of non-default options. The
    /// tests of this module check that it validates unchanged and survives
    /// a round trip through JSON.
    pub fn example() -> Self {
        use std::path::PathBuf;

        let firefox = PathBuf::from(r"C:\Program Files\Mozilla Firefox\firefox.exe");
        let code = PathBuf::from(r"C:\Users\me\AppData\Local\Programs\Microsoft VS Code\Code.exe");
        let steam_exe = PathBuf::from(r"C:\Program Files (x86)\Steam\steam.exe");
        let calculator = "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App";
        let settings_app =
            "windows.immersivecontrolpanel_cw5n1h2txyewy!microsoft.windows.immersivecontrolpanel";

        let mut example = Self {
            icon_size: 56,
            ..Self::default()
        };
        example.magnification.max_scale = 1.8;
        example.items.task_view = true;
        example.appearance.corner_radius = 24;
        example.pinned = vec![
            PinnedApp {
                id: AppId::for_exe(&firefox),
                label: None,
                launch: LaunchTarget::Exe {
                    path: firefox,
                    args: vec![],
                    working_dir: None,
                },
            },
            PinnedApp {
                id: AppId::for_exe(&code),
                label: Some("VS Code".into()),
                launch: LaunchTarget::Exe {
                    path: code,
                    args: vec!["--new-window".into()],
                    working_dir: Some(PathBuf::from(r"C:\Users\me\Projects")),
                },
            },
            // A shortcut is pinned under the id of the executable it resolves
            // to, so the running windows merge into the same item.
            PinnedApp {
                id: AppId::for_exe(&steam_exe),
                label: None,
                launch: LaunchTarget::Shortcut {
                    path: PathBuf::from(
                        r"C:\ProgramData\Microsoft\Windows\Start Menu\Programs\Steam\Steam.lnk",
                    ),
                },
            },
            PinnedApp {
                id: AppId::for_app_user_model_id(calculator),
                label: None,
                launch: LaunchTarget::StoreApp {
                    app_user_model_id: calculator.into(),
                },
            },
            // The Settings app is launched by URI, but its windows have the
            // immersive control panel's AppUserModelID.
            PinnedApp {
                id: AppId::for_app_user_model_id(settings_app),
                label: Some("Settings".into()),
                launch: LaunchTarget::Uri {
                    uri: "ms-settings:".into(),
                },
            },
        ];
        example
    }

    pub fn is_pinned(&self, id: &AppId) -> bool {
        self.pinned.iter().any(|p| &p.id == id)
    }

    /// Append a pin. Returns `false` (and changes nothing) if already pinned.
    pub fn pin(&mut self, app: PinnedApp) -> bool {
        if self.is_pinned(&app.id) {
            return false;
        }
        self.pinned.push(app);
        true
    }

    /// Remove a pin. Returns `false` if it was not pinned.
    pub fn unpin(&mut self, id: &AppId) -> bool {
        let before = self.pinned.len();
        self.pinned.retain(|p| &p.id != id);
        self.pinned.len() != before
    }

    /// Reorder pins to match `order`. Ids not in `order` keep their relative
    /// order after the listed ones; unknown ids are ignored.
    pub fn reorder_pinned(&mut self, order: &[AppId]) {
        let mut remaining = std::mem::take(&mut self.pinned);
        let mut result = Vec::with_capacity(remaining.len());
        for id in order {
            if let Some(pos) = remaining.iter().position(|p| &p.id == id) {
                result.push(remaining.remove(pos));
            }
        }
        result.extend(remaining);
        self.pinned = result;
    }
}

fn finite_or(value: f32, fallback: f32) -> f32 {
    if value.is_finite() { value } else { fallback }
}

/// Screen edge the dock is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum DockPosition {
    /// Bottom edge, as a row centred on it (the default).
    Bottom,
    /// Left edge, as a column centred on it. Magnified icons grow towards
    /// the middle of the screen, and the name tooltips appear beside them.
    Left,
    /// Right edge, as a column centred on it. Magnified icons grow towards
    /// the middle of the screen, and the name tooltips appear beside them.
    Right,
    /// Top edge, as a row centred on it.
    Top,
}

/// Icon magnification near the cursor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct Magnification {
    /// Magnify icons as the cursor comes near them.
    pub enabled: bool,
    /// Scale of the icon directly under the cursor (1.0..=2.5).
    pub max_scale: f32,
    /// How many neighbouring icons on each side of that icon grow too
    /// (1..=6).
    pub range_icons: u32,
}

impl Default for Magnification {
    fn default() -> Self {
        Self {
            enabled: true,
            max_scale: 1.6,
            range_icons: 3,
        }
    }
}

/// When the dock hides itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum AutoHideMode {
    /// The dock stays on screen.
    Off,
    /// The dock hides whenever the cursor leaves it, and comes back when
    /// the cursor touches the dock's screen edge.
    Always,
    /// The dock hides only while a window of another program overlaps the
    /// panel, and comes back as soon as none does.
    WhenOverlapped,
}

/// Auto-hide: when the dock hides itself, and how long it waits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct AutoHide {
    /// Whether and when the dock hides. Whatever the mode, a fullscreen
    /// app hides the dock of its monitor.
    pub mode: AutoHideMode,
    /// How long the dock waits before it hides, in milliseconds
    /// (0..=5000). The wait starts when the cursor leaves the dock, or when
    /// a window starts to overlap it.
    pub delay_ms: u32,
}

impl Default for AutoHide {
    fn default() -> Self {
        Self {
            mode: AutoHideMode::Off,
            delay_ms: 250,
        }
    }
}

/// Which monitors get a dock window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum MonitorMode {
    /// One dock, on the primary monitor.
    Primary,
    /// One dock per monitor.
    All,
}

/// Light or dark colours for the dock and the settings window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum ThemeMode {
    /// Follow the Windows app mode, light or dark (`ThemeInfo.dark`).
    System,
    /// Always light.
    Light,
    /// Always dark.
    Dark,
}

/// What is drawn behind the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum Backdrop {
    /// A translucent CSS background only. It works on every Windows version
    /// and on every dock.
    Css,
    /// The acrylic material of DWM (the Desktop Window Manager, which draws
    /// the windows on screen) behind the panel. Experimental, and the
    /// settings page does not offer it. On Windows 11 22H2 or later, a
    /// native window under the primary dock's panel shows the material (a
    /// prototype). On older Windows versions, under a contrast theme and on
    /// every secondary dock, the panel draws the `css` backdrop instead.
    Acrylic,
    /// The mica material of DWM behind the panel. Experimental, and the
    /// settings page does not offer it. On Windows 11 22H2 or later, a
    /// native window under the primary dock's panel shows the material (a
    /// prototype). On older Windows versions, under a contrast theme and on
    /// every secondary dock, the panel draws the `css` backdrop instead.
    Mica,
}

/// The dock's theme preset: how the panel is drawn. Every preset takes
/// light or dark from `Settings::theme`, and its running dots use the
/// Windows accent colour. The user's `dock.css`, in the folder of the
/// settings file, can change any preset further through the variables
/// that `docs/THEMING.md` lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum ThemePreset {
    /// A translucent, blurred panel with a thin outline and a soft shadow
    /// (the default). `opacity` sets how translucent it is.
    Standard,
    /// An opaque panel without blur, with a stronger outline and a tighter
    /// shadow. It is easy to read over any wallpaper, and the best choice
    /// while Windows' *Transparency effects* are off.
    Solid,
    /// No panel fill, outline or shadow: only the icons, the running dots
    /// and the separators.
    Minimal,
}

/// How the dock looks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct Appearance {
    /// Theme preset of the panel. A change restyles the dock at once.
    pub theme: ThemePreset,
    /// Panel background opacity of the `standard` theme (0.0..=1.0). The
    /// `solid` panel is always opaque, and the `minimal` one has no fill.
    pub opacity: f32,
    /// Panel corner radius in CSS pixels (0..=64).
    pub corner_radius: u32,
    /// Backdrop material behind the panel.
    pub backdrop: Backdrop,
    /// Show an item's name in a tooltip while the cursor is over the item
    /// or the item has the keyboard focus.
    pub tooltips: bool,
    /// Bounce the icon of an app that the dock launched while the app
    /// starts. No icon bounces while Windows' *Animation effects* are off.
    pub bounce_on_launch: bool,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            theme: ThemePreset::Standard,
            opacity: 0.72,
            corner_radius: 22,
            backdrop: Backdrop::Css,
            tooltips: true,
            bounce_on_launch: true,
        }
    }
}

/// Which fixed items (the items that are not apps) the dock shows. The
/// items at the left come before the apps and the items at the right come
/// after them; on a dock at the left or right edge, left means the top and
/// right means the bottom.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct FixedItems {
    /// The Start item at the left, which opens the Start menu.
    pub launcher: bool,
    /// The Task View item at the left.
    pub task_view: bool,
    /// The Show Desktop item at the left.
    pub show_desktop: bool,
    /// The Quick Settings item at the right, before the Recycle Bin. A
    /// click opens what Win+A opens.
    pub quick_settings: bool,
    /// The Notification Center item at the right, before the Recycle Bin.
    /// A click opens what Win+N opens.
    pub notification_center: bool,
    /// The battery item at the right, after Notification Center. It appears
    /// only on a PC with a battery. A click opens what Win+A opens.
    pub battery: bool,
    /// The clock item at the right, after the battery. A click opens what
    /// Win+N opens.
    pub clock: bool,
    /// The Recycle Bin item, last at the right.
    pub recycle_bin: bool,
    /// Draw thin separators between the item groups: the items at the
    /// left, the pinned apps, the other running apps and the items at the
    /// right.
    pub separators: bool,
}

impl Default for FixedItems {
    fn default() -> Self {
        Self {
            launcher: true,
            task_view: false,
            show_desktop: false,
            quick_settings: false,
            notification_center: false,
            battery: false,
            clock: false,
            recycle_bin: true,
            separators: true,
        }
    }
}

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
        }
    }
}

/// An app pinned to the dock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct PinnedApp {
    /// Must match the id Stem gives the app's windows, so that the pin and
    /// the running windows share one item: `exe:` followed by the
    /// program's path in lower case with forward slashes
    /// (`AppId::for_exe`), or `aumid:` followed by the app's AppUserModelID
    /// (`AppId::for_app_user_model_id`). An AppUserModelID is the
    /// identifier Windows uses to group an app's windows on the taskbar.
    pub id: AppId,
    /// A name that replaces the app's own name on the dock, or `null` to
    /// use the app's own name.
    #[serde(default)]
    pub label: Option<String>,
    /// How to start the app when it has no window.
    pub launch: LaunchTarget,
}

/// Windows integration settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct SystemIntegration {
    /// Start Stem when the user signs in.
    pub autostart: bool,
    /// Hide the stock Windows taskbar while Stem runs, by turning on
    /// Windows' own *Automatically hide the taskbar* option. Turning this
    /// off, or quitting Stem, sets that option back to what it was.
    pub hide_windows_taskbar: bool,
    /// Register each dock window as an AppBar (an application desktop
    /// toolbar: Windows takes a band at the dock's screen edge out of the
    /// work area, as it does for the stock taskbar), so that maximised
    /// windows end at the dock instead of going under it. This works only
    /// while `autoHide.mode` is `off`: an auto-hiding dock reserves
    /// nothing. A change applies at once.
    pub reserve_screen_space: bool,
    /// Write a log file, `stem.log`, in Stem's `logs` folder under
    /// `%LOCALAPPDATA%`. Stem reads this setting when it starts, so a change
    /// applies after a restart.
    pub file_logging: bool,
}

impl Default for SystemIntegration {
    fn default() -> Self {
        Self {
            autostart: false,
            hide_windows_taskbar: false,
            reserve_screen_space: false,
            file_logging: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use pretty_assertions::assert_eq;

    use super::*;

    fn pin(id: &str) -> PinnedApp {
        PinnedApp {
            id: AppId(id.to_owned()),
            label: None,
            launch: LaunchTarget::Exe {
                path: PathBuf::from(id),
                args: vec![],
                working_dir: None,
            },
        }
    }

    /// Tiny deterministic generator (xorshift64*) so the property-style tests
    /// need no extra dependency and reproduce exactly.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }

        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }

        fn bool(&mut self) -> bool {
            self.below(2) == 1
        }

        /// A `u32` that is often an edge value.
        fn u32(&mut self) -> u32 {
            match self.below(6) {
                0 => 0,
                1 => u32::MAX,
                2 => 1,
                _ => self.next() as u32 % 10_000,
            }
        }

        /// An `f32` that is often an edge value, including non-finite ones.
        fn f32(&mut self) -> f32 {
            match self.below(8) {
                0 => f32::NAN,
                1 => f32::INFINITY,
                2 => f32::NEG_INFINITY,
                3 => -1.0,
                4 => 0.0,
                5 => 1e9,
                _ => (self.below(10_000) as f32) / 1_000.0,
            }
        }

        fn pick<T: Copy>(&mut self, options: &[T]) -> T {
            options[self.below(options.len() as u64) as usize]
        }
    }

    fn arbitrary(rng: &mut Rng) -> Settings {
        Settings {
            version: rng.below(2) as u32,
            position: rng.pick(&[
                DockPosition::Bottom,
                DockPosition::Left,
                DockPosition::Right,
                DockPosition::Top,
            ]),
            icon_size: rng.u32(),
            magnification: Magnification {
                enabled: rng.bool(),
                max_scale: rng.f32(),
                range_icons: rng.u32(),
            },
            auto_hide: AutoHide {
                mode: rng.pick(&[
                    AutoHideMode::Off,
                    AutoHideMode::Always,
                    AutoHideMode::WhenOverlapped,
                ]),
                delay_ms: rng.u32(),
            },
            monitors: rng.pick(&[MonitorMode::Primary, MonitorMode::All]),
            theme: rng.pick(&[ThemeMode::System, ThemeMode::Light, ThemeMode::Dark]),
            appearance: Appearance {
                theme: rng.pick(&[
                    ThemePreset::Standard,
                    ThemePreset::Solid,
                    ThemePreset::Minimal,
                ]),
                opacity: rng.f32(),
                corner_radius: rng.u32(),
                backdrop: rng.pick(&[Backdrop::Css, Backdrop::Acrylic, Backdrop::Mica]),
                tooltips: rng.bool(),
                bounce_on_launch: rng.bool(),
            },
            items: FixedItems {
                launcher: rng.bool(),
                task_view: rng.bool(),
                show_desktop: rng.bool(),
                quick_settings: rng.bool(),
                notification_center: rng.bool(),
                battery: rng.bool(),
                clock: rng.bool(),
                recycle_bin: rng.bool(),
                separators: rng.bool(),
            },
            behavior: Behavior {
                click_active_minimizes: rng.bool(),
                show_only_current_desktop: rng.bool(),
                show_only_current_monitor: rng.bool(),
                running_indicators: rng.bool(),
                click_through: rng.bool(),
                toggle_shortcut: rng
                    .pick(&["", " ", DEFAULT_TOGGLE_SHORTCUT, "ctrl+shift+F9", "Nope+?"])
                    .to_owned(),
                focus_shortcut: rng
                    .pick(&[
                        "",
                        " ",
                        DEFAULT_FOCUS_SHORTCUT,
                        DEFAULT_TOGGLE_SHORTCUT,
                        "alt+ctrl+d",
                        "Nope+?",
                    ])
                    .to_owned(),
            },
            pinned: (0..rng.below(8))
                .map(|_| {
                    let id = rng.pick(&["", " ", "a", "b", "c", "exe:c:/x.exe", "aumid:X!App"]);
                    let mut p = pin(id);
                    p.label = rng.bool().then(|| "Label".to_owned());
                    p
                })
                .collect(),
            system: SystemIntegration {
                autostart: rng.bool(),
                hide_windows_taskbar: rng.bool(),
                reserve_screen_space: rng.bool(),
                file_logging: rng.bool(),
            },
        }
    }

    fn assert_within_ranges(s: &Settings) {
        assert!((24..=128).contains(&s.icon_size));
        assert!((1.0..=2.5).contains(&s.magnification.max_scale));
        assert!((1..=6).contains(&s.magnification.range_icons));
        assert!(s.auto_hide.delay_ms <= 5_000);
        assert!((0.0..=1.0).contains(&s.appearance.opacity));
        assert!(s.appearance.corner_radius <= 64);
        let ids: Vec<&str> = s.pinned.iter().map(|p| p.id.as_str()).collect();
        assert!(ids.iter().all(|id| !id.trim().is_empty()));
        let mut dedup = ids.clone();
        dedup.sort_unstable();
        dedup.dedup();
        assert_eq!(dedup.len(), ids.len(), "duplicate pin ids in {ids:?}");
    }

    #[test]
    fn empty_document_yields_defaults() {
        let s = Settings::from_json("{}").unwrap();
        assert_eq!(s, Settings::default());
    }

    #[test]
    fn unknown_fields_are_ignored_and_partial_structs_fill_in() {
        let s = Settings::from_json(
            r#"{"iconSize": 64, "magnification": {"maxScale": 2.0}, "bogus": 1}"#,
        )
        .unwrap();
        assert_eq!(s.icon_size, 64);
        assert_eq!(s.magnification.max_scale, 2.0);
        assert!(s.magnification.enabled);
    }

    #[test]
    fn toggle_shortcut_defaults_when_missing_and_keeps_what_is_written() {
        assert_eq!(Behavior::default().toggle_shortcut, "Ctrl+Alt+D");

        // A file written before the field existed (here a v0 file, without
        // a version either) gets the default next to its other values.
        let old = Settings::from_json(r#"{"behavior": {"clickActiveMinimizes": false}}"#).unwrap();
        assert!(!old.behavior.click_active_minimizes);
        assert_eq!(old.behavior.toggle_shortcut, DEFAULT_TOGGLE_SHORTCUT);
        assert_eq!(old.version, SETTINGS_VERSION);

        // Empty (no shortcut) and values the app cannot register are kept
        // as written, so the user sees and can fix them.
        for written in ["", "Shift+Alt+F12", "not a shortcut"] {
            let json = format!(r#"{{"behavior": {{"toggleShortcut": {written:?}}}}}"#);
            let s = Settings::from_json(&json).unwrap();
            assert_eq!(s.behavior.toggle_shortcut, written);
            assert!(s.to_json_pretty().contains(r#""toggleShortcut""#));
            assert_eq!(Settings::from_json(&s.to_json_pretty()).unwrap(), s);
        }
    }

    #[test]
    fn focus_shortcut_defaults_when_missing_and_keeps_what_is_written() {
        assert_eq!(Behavior::default().focus_shortcut, "Ctrl+Alt+Shift+D");
        assert_ne!(DEFAULT_FOCUS_SHORTCUT, DEFAULT_TOGGLE_SHORTCUT);

        // A file written before the field existed gets the default, and the
        // toggle shortcut it has is left alone.
        let old =
            Settings::from_json(r#"{"version": 1, "behavior": {"toggleShortcut": "F9"}}"#).unwrap();
        assert_eq!(old.behavior.toggle_shortcut, "F9");
        assert_eq!(old.behavior.focus_shortcut, DEFAULT_FOCUS_SHORTCUT);

        // Empty (no shortcut), the toggle shortcut's own chord and values
        // the app cannot register are kept as written: the app layer
        // decides what registers, so the user sees and can fix the value.
        for written in ["", "ctrl+alt+d", "Shift+Alt+F12", "not a shortcut"] {
            let json = format!(r#"{{"behavior": {{"focusShortcut": {written:?}}}}}"#);
            let s = Settings::from_json(&json).unwrap();
            assert_eq!(s.behavior.focus_shortcut, written);
            assert_eq!(s.behavior.toggle_shortcut, DEFAULT_TOGGLE_SHORTCUT);
            assert!(s.to_json_pretty().contains(r#""focusShortcut""#));
            assert_eq!(Settings::from_json(&s.to_json_pretty()).unwrap(), s);
        }
    }

    #[test]
    fn theme_preset_defaults_to_standard_and_round_trips_in_camel_case() {
        assert_eq!(Appearance::default().theme, ThemePreset::Standard);

        // A file written before the field existed gets the default next to
        // its other appearance values.
        let old = Settings::from_json(r#"{"appearance": {"opacity": 0.5}}"#).unwrap();
        assert_eq!(old.appearance.theme, ThemePreset::Standard);
        assert_eq!(old.appearance.opacity, 0.5);

        for (preset, name) in [
            (ThemePreset::Standard, "standard"),
            (ThemePreset::Solid, "solid"),
            (ThemePreset::Minimal, "minimal"),
        ] {
            assert_eq!(serde_json::to_string(&preset).unwrap(), format!("{name:?}"));
            let json = format!(r#"{{"appearance": {{"theme": {name:?}}}}}"#);
            let s = Settings::from_json(&json).unwrap();
            assert_eq!(s.appearance.theme, preset);
            assert!(
                s.to_json_pretty()
                    .contains(&format!(r#""theme": {name:?}"#))
            );
            assert_eq!(Settings::from_json(&s.to_json_pretty()).unwrap(), s);
        }

        // Like every other enum value: a name Stem does not know makes the
        // file unusable rather than silently standard (the app keeps it as
        // `settings.json.broken`).
        assert!(matches!(
            Settings::from_json(r#"{"appearance": {"theme": "glass"}}"#),
            Err(SettingsError::Parse(_))
        ));
    }

    #[test]
    fn quick_settings_and_notification_center_are_off_unless_written() {
        let items = FixedItems::default();
        assert!(!items.quick_settings);
        assert!(!items.notification_center);

        // A file written before the fields existed keeps its other items and
        // gets both off.
        let old =
            Settings::from_json(r#"{"items": {"launcher": false, "taskView": true}}"#).unwrap();
        assert!(!old.items.launcher);
        assert!(old.items.task_view);
        assert!(!old.items.quick_settings);
        assert!(!old.items.notification_center);

        // Both are camelCase keys of `items` and survive a round trip.
        let on = Settings::from_json(
            r#"{"items": {"quickSettings": true, "notificationCenter": true}}"#,
        )
        .unwrap();
        assert!(on.items.quick_settings);
        assert!(on.items.notification_center);
        let json = on.to_json_pretty();
        assert!(json.contains(r#""quickSettings": true"#), "{json}");
        assert!(json.contains(r#""notificationCenter": true"#), "{json}");
        assert_eq!(Settings::from_json(&json).unwrap(), on);
    }

    #[test]
    fn clock_and_battery_are_off_unless_written() {
        let items = FixedItems::default();
        assert!(!items.clock);
        assert!(!items.battery);

        // A file written before the fields existed keeps its other
        // items and gets both off.
        let old = Settings::from_json(
            r#"{"items": {"launcher": false, "quickSettings": true, "recycleBin": false}}"#,
        )
        .unwrap();
        assert!(!old.items.launcher);
        assert!(old.items.quick_settings);
        assert!(!old.items.recycle_bin);
        assert!(!old.items.clock);
        assert!(!old.items.battery);

        // Both are camelCase keys of `items`, independent of each other, and
        // survive a round trip.
        for (clock, battery) in [(true, false), (false, true), (true, true)] {
            let on = Settings::from_json(&format!(
                r#"{{"items": {{"clock": {clock}, "battery": {battery}}}}}"#
            ))
            .unwrap();
            assert_eq!((on.items.clock, on.items.battery), (clock, battery));
            let json = on.to_json_pretty();
            assert!(json.contains(&format!(r#""clock": {clock}"#)), "{json}");
            assert!(json.contains(&format!(r#""battery": {battery}"#)), "{json}");
            assert_eq!(Settings::from_json(&json).unwrap(), on);
            // `validate` leaves them as written.
            let mut validated = on.clone();
            validated.validate();
            assert_eq!(validated, on);
        }
    }

    #[test]
    fn validate_clamps_and_dedupes() {
        let mut s = Settings {
            icon_size: 999,
            ..Default::default()
        };
        s.magnification.max_scale = 9.0;
        s.pinned = vec![pin("a"), pin("a"), pin(""), pin("b")];
        s.validate();
        assert_eq!(s.icon_size, 128);
        assert_eq!(s.magnification.max_scale, 2.5);
        assert_eq!(
            s.pinned.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            vec!["a", "b"]
        );
    }

    #[test]
    fn validate_drops_blank_pin_ids_and_keeps_the_first_duplicate() {
        let first = PinnedApp {
            label: Some("first".into()),
            ..pin("a")
        };
        let second = PinnedApp {
            label: Some("second".into()),
            ..pin("a")
        };
        let mut s = Settings {
            pinned: vec![pin(" \t"), first, pin("b"), second],
            ..Default::default()
        };
        s.validate();
        assert_eq!(
            s.pinned.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            vec!["a", "b"]
        );
        assert_eq!(s.pinned[0].label.as_deref(), Some("first"));
    }

    #[test]
    fn validate_replaces_non_finite_floats_with_defaults() {
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut s = Settings::default();
            s.magnification.max_scale = bad;
            s.appearance.opacity = bad;
            s.validate();
            assert_eq!(
                s.magnification.max_scale,
                Magnification::default().max_scale
            );
            assert_eq!(s.appearance.opacity, Appearance::default().opacity);
            // And the document stays serialisable (JSON has no NaN/inf).
            assert_ne!(s.to_json_pretty(), "{}");
            assert_eq!(Settings::from_json(&s.to_json_pretty()).unwrap(), s);
        }
    }

    #[test]
    fn validate_is_idempotent_for_default_and_example() {
        for mut s in [Settings::default(), Settings::example()] {
            let original = s.clone();
            s.validate();
            assert_eq!(s, original, "already-valid documents must not change");
            s.validate();
            assert_eq!(s, original);
        }
    }

    #[test]
    fn validate_is_idempotent_on_pseudo_random_documents() {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        for i in 0..2_000 {
            let raw = arbitrary(&mut rng);
            let mut once = raw.clone();
            once.validate();
            assert_within_ranges(&once);
            let mut twice = once.clone();
            twice.validate();
            assert_eq!(once, twice, "iteration {i}: validate is not idempotent");

            // A validated document round-trips through JSON unchanged
            // (modulo the version stamp `from_json` applies).
            let back = Settings::from_json(&once.to_json_pretty())
                .unwrap_or_else(|e| panic!("iteration {i}: {e}"));
            let mut expected = once.clone();
            expected.migrate();
            assert_eq!(
                back, expected,
                "iteration {i}: round trip changed the document"
            );
        }
    }

    #[test]
    fn diff_requires_window_rebuild_only_for_position_and_monitors() {
        let base = Settings::default();
        assert!(!base.diff_requires_window_rebuild(&base));

        let mut restyled = base.clone();
        restyled.icon_size = 96;
        restyled.appearance.opacity = 0.3;
        restyled.auto_hide.mode = AutoHideMode::Always;
        restyled.theme = ThemeMode::Dark;
        restyled.appearance.theme = ThemePreset::Minimal;
        restyled.items.launcher = false;
        restyled.pinned.push(pin("a"));
        assert!(!base.diff_requires_window_rebuild(&restyled));

        let mut moved = base.clone();
        moved.position = DockPosition::Left;
        assert!(base.diff_requires_window_rebuild(&moved));
        assert!(moved.diff_requires_window_rebuild(&base));

        let mut everywhere = base.clone();
        everywhere.monitors = MonitorMode::All;
        assert!(base.diff_requires_window_rebuild(&everywhere));
    }

    #[test]
    fn example_is_valid_pins_every_launch_kind_and_round_trips() {
        let example = Settings::example();
        assert_eq!(example.version, SETTINGS_VERSION);
        assert!((4..=5).contains(&example.pinned.len()));
        let kinds: Vec<&str> = example
            .pinned
            .iter()
            .map(|p| match p.launch {
                LaunchTarget::Exe { .. } => "exe",
                LaunchTarget::Shortcut { .. } => "shortcut",
                LaunchTarget::StoreApp { .. } => "storeApp",
                LaunchTarget::Uri { .. } => "uri",
            })
            .collect();
        for kind in ["exe", "shortcut", "storeApp", "uri"] {
            assert!(
                kinds.contains(&kind),
                "example has no {kind} pin: {kinds:?}"
            );
        }
        // Ids use the canonical forms the engine derives for running windows.
        assert!(
            example.pinned.iter().all(|p| {
                p.id.as_str().starts_with("exe:") || p.id.as_str().starts_with("aumid:")
            })
        );
        assert!(example.is_pinned(&AppId::for_app_user_model_id(
            "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"
        )));

        let json = example.to_json_pretty();
        assert!(json.contains(r#""kind": "shortcut""#));
        assert_eq!(Settings::from_json(&json).unwrap(), example);
        assert_ne!(example, Settings::default());
    }

    #[test]
    fn newer_version_is_rejected() {
        let err = Settings::from_json(r#"{"version": 99}"#).unwrap_err();
        assert!(matches!(err, SettingsError::TooNew { found: 99, .. }));
    }

    #[test]
    fn reorder_keeps_unlisted_pins_at_the_end() {
        let mut s = Settings {
            pinned: vec![pin("a"), pin("b"), pin("c"), pin("d")],
            ..Default::default()
        };
        s.reorder_pinned(&[AppId("c".into()), AppId("zzz".into()), AppId("a".into())]);
        assert_eq!(
            s.pinned.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            vec!["c", "a", "b", "d"]
        );
    }

    #[test]
    fn round_trips_through_json() {
        let mut s = Settings::default();
        s.pinned.push(pin("exe:c:/x.exe"));
        let back = Settings::from_json(&s.to_json_pretty()).unwrap();
        assert_eq!(back, s);
    }
}
