//! Domain model shared by every layer: platform adapters, the Tauri app, and
//! (through generated TypeScript bindings) the frontend.
//!
//! Conventions:
//! - Everything is `serde`-serialisable with `camelCase` field names, because
//!   these types are the IPC payloads described in `docs/ipc/`.
//! - Geometry is in **physical pixels**; the Tauri layer converts to CSS pixels.
//! - No platform handle types. A window is a [`WindowId`] (an opaque `u64`),
//!   an icon is an [`IconKey`] (an opaque string the platform knows how to
//!   resolve), a monitor is a [`MonitorId`].

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

// TypeScript bindings are exported to `src/lib/ipc/generated/` (relative to
// the repository root) by `cargo test -p stem-core --features ts`.

/// Opaque identifier of a top-level window. On Windows this is the `HWND`
/// value, but nothing outside `stem-platform` may rely on that.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(transparent)]
pub struct WindowId(#[cfg_attr(feature = "ts", ts(type = "number"))] pub u64);

/// Stable identity of an application, used to group windows and to match
/// pinned entries with running windows. See [`AppId::for_exe`] and
/// [`AppId::for_app_user_model_id`] for the canonical forms.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(transparent)]
pub struct AppId(pub String);

impl AppId {
    /// Identity for a classic desktop application identified by its executable.
    /// The path is normalised (lower-cased, forward slashes) so that the same
    /// binary always yields the same id regardless of how it was spelled.
    pub fn for_exe(path: &Path) -> Self {
        Self(format!("exe:{}", normalize_path(path)))
    }

    /// Identity for an application that declares an AppUserModelID (Store/UWP
    /// apps and desktop apps that opt in, e.g. browsers with profiles).
    pub fn for_app_user_model_id(aumid: &str) -> Self {
        Self(format!("aumid:{}", aumid.trim()))
    }

    /// Last-resort identity based on the window class name.
    pub fn for_window_class(class_name: &str) -> Self {
        Self(format!("class:{}", class_name.trim().to_lowercase()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Normalise a filesystem path for identity purposes.
pub fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/").to_lowercase()
}

/// Opaque key the platform can turn into PNG bytes (see `Platform::icon_png`).
/// Keys starting with `shell:` are rendered by the frontend from built-in
/// vector art (`shell:start`, `shell:task-view`, `shell:show-desktop`,
/// `shell:quick-settings`, `shell:notification-center`,
/// `shell:recycle-bin-empty`, `shell:recycle-bin-full`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(transparent)]
pub struct IconKey(pub String);

impl IconKey {
    pub fn shell(name: &str) -> Self {
        Self(format!("shell:{name}"))
    }

    pub fn is_shell(&self) -> bool {
        self.0.starts_with("shell:")
    }
}

/// Opaque monitor identifier (stable across sessions where the OS allows).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(transparent)]
pub struct MonitorId(pub String);

/// Axis-aligned rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn right(&self) -> i32 {
        self.x + self.width
    }

    pub fn bottom(&self) -> i32 {
        self.y + self.height
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }
}

/// A display attached to the system.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct MonitorInfo {
    pub id: MonitorId,
    pub name: String,
    /// Full bounds of the monitor in the virtual desktop coordinate space.
    pub bounds: Rect,
    /// Bounds minus reserved areas (stock taskbar, app bars).
    pub work_area: Rect,
    /// DPI scale factor (1.0 = 96 DPI, 1.5 = 144 DPI, ...).
    pub scale_factor: f64,
    pub is_primary: bool,
}

/// What a platform knows about a top-level window, with no OS types.
/// Produced by `Platform::windows()` and by platform events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct RawWindow {
    pub id: WindowId,
    pub title: String,
    pub class_name: String,
    pub process_id: u32,
    /// Full path of the owning process image, when it could be read.
    pub exe_path: Option<PathBuf>,
    /// Explicit AppUserModelID set on the window or its process, if any.
    pub app_user_model_id: Option<String>,
    pub is_visible: bool,
    pub is_minimized: bool,
    /// Cloaked by the compositor: on another virtual desktop, a suspended
    /// Store app, a window in a desktop-switch animation. Without
    /// `on_current_desktop` the engine hides cloaked windows while
    /// `behavior.showOnlyCurrentDesktop` is on.
    pub is_cloaked: bool,
    /// Whether the window is on the current virtual desktop (the
    /// shell's `IVirtualDesktopManager::IsWindowOnCurrentVirtualDesktop`):
    /// `Some(false)` = parked on another desktop, `Some(true)` = on this
    /// one (cloaked or not), `None` = unknown (the platform could not ask,
    /// or the window belongs to another session or a protected process).
    /// While `behavior.showOnlyCurrentDesktop` is on the engine hides a
    /// `Some(false)` window and shows a `Some(true)` one whatever its cloak
    /// state; for `None` it falls back to `is_cloaked`.
    #[serde(default)]
    pub on_current_desktop: Option<bool>,
    pub monitor: Option<MonitorId>,
    /// The window's frame on screen, in physical pixels of the virtual
    /// desktop (the DWM extended frame bounds, else `GetWindowRect`), when
    /// the platform could read it. Refreshed whenever the window is queried,
    /// which the platform does about every 150 ms while the window moves.
    /// Nothing renders it: the platform's fullscreen detection reads it, and
    /// the engine's overlap rule (`crate::overlap`) tests it against the
    /// dock's panel.
    #[serde(default)]
    pub rect: Option<Rect>,
    /// Icon of this specific window (may differ from the app icon).
    pub icon_key: Option<IconKey>,
}

/// How to start an application.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum LaunchTarget {
    /// A classic desktop executable.
    #[serde(rename_all = "camelCase")]
    Exe {
        /// Path of the executable.
        path: PathBuf,
        /// Command-line arguments, one string per argument.
        #[serde(default)]
        args: Vec<String>,
        /// Working directory to start the app in, or `null` to start it in
        /// the executable's own folder.
        #[serde(default)]
        working_dir: Option<PathBuf>,
    },
    /// A `.lnk` shortcut. Windows resolves it when Stem launches it, and
    /// Stem reads the shortcut to identify the app it starts.
    #[serde(rename_all = "camelCase")]
    Shortcut {
        /// Path of the `.lnk` file.
        path: PathBuf,
    },
    /// A packaged (Store or UWP) app, launched by its AppUserModelID.
    #[serde(rename_all = "camelCase")]
    StoreApp {
        /// The app's AppUserModelID.
        app_user_model_id: String,
    },
    /// Anything that `ShellExecute` can open, such as `ms-settings:`,
    /// `shell:AppsFolder` or a web URL.
    #[serde(rename_all = "camelCase")]
    Uri {
        /// The URI to open.
        uri: String,
    },
}

/// Resolved facts about an application: how to show it and how to start it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct AppIdentity {
    pub id: AppId,
    pub display_name: String,
    pub icon_key: Option<IconKey>,
    /// `None` when the app cannot be (re)launched, e.g. identity came only
    /// from a window class.
    pub launch: Option<LaunchTarget>,
}

/// A window as presented inside a dock item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct WindowInfo {
    pub id: WindowId,
    pub title: String,
    pub is_minimized: bool,
    pub is_active: bool,
    pub monitor: Option<MonitorId>,
    pub icon_key: Option<IconKey>,
    /// This window requested attention (taskbar flash) and was not
    /// activated since; never set on the foreground window. The item's
    /// `needs_attention` is this flag of any of its windows.
    pub needs_attention: bool,
}

/// Kind of a dock item. Everything that is not `App` is a fixed item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum DockItemKind {
    App,
    Launcher,
    TaskView,
    ShowDesktop,
    /// Opens Windows' quick settings.
    QuickSettings,
    /// Opens Windows' notification center and calendar.
    NotificationCenter,
    RecycleBin,
    Separator,
}

/// One slot in the dock, ready to render.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct DockItem {
    /// Stable, unique key for keyed rendering (`app:<id>`, `fixed:launcher`, `sep:1`).
    pub key: String,
    pub kind: DockItemKind,
    pub app_id: Option<AppId>,
    /// Tooltip / accessible name.
    pub title: String,
    pub icon_key: Option<IconKey>,
    pub pinned: bool,
    /// Running windows of this app, in first-seen order. Empty for pinned-only
    /// apps and for fixed items.
    pub windows: Vec<WindowInfo>,
    /// One of this item's windows is the foreground window.
    pub is_active: bool,
    /// A window of this app requested attention (taskbar flash): the
    /// `needs_attention` of any of `windows`.
    pub needs_attention: bool,
    /// Small text badge (e.g. unread count). Reserved; currently unused.
    pub badge: Option<String>,
}

/// Full snapshot of what the dock should show. Emitted as `dock://state`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct DockState {
    /// Monotonic counter; the UI may drop snapshots with a lower revision.
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub revision: u64,
    pub items: Vec<DockItem>,
    /// The OS foreground window, when it is one the engine tracks. It may be a
    /// window no item shows (e.g. on another virtual desktop); use
    /// `DockItem.isActive` / `WindowInfo.isActive` for highlighting.
    pub foreground: Option<WindowId>,
    /// The per-dock facts: one entry per dock window whose monitor
    /// the app layer has told the engine (`EngineInput::DockPanelChanged`),
    /// sorted by monitor id. A dock page finds its own entry by the monitor
    /// it sits on; a dock with no entry (yet) takes both facts as `false`.
    #[serde(default)]
    pub docks: Vec<DockFacts>,
}

/// What one dock window needs to know about its own monitor, part
/// of [`DockState`]. The two facts are independent; the UI combines them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct DockFacts {
    /// The monitor this dock window sits on (`dock.html?monitor=<id>`; the
    /// primary monitor for the `dock` window).
    pub monitor: MonitorId,
    /// The foreground window owns the whole of this monitor: a game, a
    /// browser after F11, a video in fullscreen, a slideshow. The
    /// platform decides (`PlatformEvent::FullscreenChanged`, with the
    /// window's monitor); the engine sets it on the dock of that monitor,
    /// or on the primary monitor's dock when the platform could not tell
    /// the monitor; the UI gets that dock out of the way while it is set,
    /// whatever `autoHide.mode` says.
    pub fullscreen: bool,
    /// A window of another process overlaps this dock's resting panel:
    /// visible, not minimised, not cloaked, with a frame that
    /// intersects the panel rect the dock's page reported through
    /// `set_dock_panel_rect` (`crate::overlap`). `false` while no panel
    /// rect is known for the dock (before its page's first report, or
    /// while its window is hidden). The UI hides the dock after
    /// `autoHide.delayMs` while it is set and `autoHide.mode` is
    /// `whenOverlapped`, and shows it again when it clears.
    pub overlapped: bool,
}

/// Current OS theme facts, emitted as `dock://theme`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct ThemeInfo {
    /// Accent colour as `#RRGGBB`.
    pub accent: String,
    pub dark: bool,
    pub high_contrast: bool,
    /// Whether the OS has transparency effects enabled.
    pub transparency: bool,
}

impl Default for ThemeInfo {
    fn default() -> Self {
        Self {
            accent: "#0078D4".to_owned(),
            dark: true,
            high_contrast: false,
            transparency: true,
        }
    }
}

/// Recycle Bin contents summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct RecycleBinState {
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub item_count: u64,
    #[cfg_attr(feature = "ts", ts(type = "number"))]
    pub size_bytes: u64,
}

impl RecycleBinState {
    pub fn is_empty(&self) -> bool {
        self.item_count == 0
    }
}

/// Something the user can do to a window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum WindowAction {
    Activate,
    Minimize,
    Restore,
    Close,
}

/// Shell-level actions behind the fixed items.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum ShellAction {
    OpenStartMenu,
    OpenTaskView,
    ToggleShowDesktop,
    OpenRecycleBin,
    EmptyRecycleBin,
    /// Windows' quick settings flyout, as Win+A opens it.
    OpenQuickSettings,
    /// Windows' notification center and calendar, as Win+N opens them.
    OpenNotificationCenter,
}

/// Facts about this installation for the settings page's *About* section:
/// the answer to `get_app_info`. Paths are shown, never parsed, by
/// the UI; `open_app_folder` opens the folders behind them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    /// The app's package version (`0.1.0`).
    pub version: String,
    /// The settings file the app reads and writes, as a path; or a short
    /// note in parentheses when nothing is saved this run (the mock
    /// platform, a profile that could not be used).
    pub settings_path: String,
    /// The log file (`stem.log` in the app's log folder), or `""` while file
    /// logging is off (`system.fileLogging` at startup).
    pub log_path: String,
    /// The platform implementation's name: `"windows"`, or `"mock"` in a
    /// demo run.
    pub platform: String,
}

/// A folder of the app's own that `open_app_folder` opens in Explorer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub enum AppFolder {
    /// The log folder (`AppInfo::log_path`'s folder), or its parent while
    /// none exists yet.
    Logs,
    /// The settings file's folder (`AppInfo::settings_path`'s folder).
    Settings,
}

/// The user's stylesheet: `dock.css` in the settings file's folder,
/// as the app read it at start or at the last `reload_user_css`. The answer
/// to `get_user_css` and `reload_user_css`, and the payload of
/// `dock://user-css`. The dock page puts `css` in one `<style>` element
/// after its own styles; `docs/THEMING.md` lists the variables it supports.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub struct UserCss {
    /// Where the file is, as a path shown (never parsed) by the UI; or a
    /// short note in parentheses when nothing is saved this run (the mock
    /// platform, a profile that could not be used), which has no file.
    pub path: String,
    /// The file's text, a leading byte order mark dropped; `null` when the
    /// file does not exist, cannot be read, is not UTF-8 or is larger than
    /// 64 KiB (the log says which).
    pub css: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_css_serialises_in_camel_case_with_a_null_css() {
        let none = UserCss {
            path: r"C:\Users\me\AppData\Roaming\dev.davidcristian.stem\dock.css".to_owned(),
            css: None,
        };
        let json = serde_json::to_value(&none).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "path": r"C:\Users\me\AppData\Roaming\dev.davidcristian.stem\dock.css",
                "css": null,
            })
        );
        assert_eq!(serde_json::from_value::<UserCss>(json).unwrap(), none);
        let some = UserCss {
            css: Some(".dock-stage { --stem-dot: red; }".to_owned()),
            ..none
        };
        assert_eq!(
            serde_json::to_value(&some).unwrap()["css"],
            ".dock-stage { --stem-dot: red; }"
        );
    }

    #[test]
    fn app_id_for_exe_is_case_and_slash_insensitive() {
        let a = AppId::for_exe(Path::new(r"C:\Program Files\Foo\Foo.exe"));
        let b = AppId::for_exe(Path::new("c:/program files/foo/foo.exe"));
        assert_eq!(a, b);
        assert_eq!(a.as_str(), "exe:c:/program files/foo/foo.exe");
    }

    #[test]
    fn launch_target_serialises_with_kind_tag() {
        let t = LaunchTarget::StoreApp {
            app_user_model_id: "Microsoft.WindowsCalculator_8wekyb3d8bbwe!App".into(),
        };
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(
            json,
            r#"{"kind":"storeApp","appUserModelId":"Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"}"#
        );
        let back: LaunchTarget = serde_json::from_str(&json).unwrap();
        assert_eq!(back, t);
    }

    #[test]
    fn quick_settings_and_notification_center_serialise_in_camel_case() {
        for (kind, name) in [
            (DockItemKind::QuickSettings, "quickSettings"),
            (DockItemKind::NotificationCenter, "notificationCenter"),
        ] {
            let json = serde_json::to_string(&kind).unwrap();
            assert_eq!(json, format!("{name:?}"));
            assert_eq!(serde_json::from_str::<DockItemKind>(&json).unwrap(), kind);
        }
        for (action, name) in [
            (ShellAction::OpenQuickSettings, "openQuickSettings"),
            (
                ShellAction::OpenNotificationCenter,
                "openNotificationCenter",
            ),
        ] {
            let json = serde_json::to_string(&action).unwrap();
            assert_eq!(json, format!("{name:?}"));
            assert_eq!(serde_json::from_str::<ShellAction>(&json).unwrap(), action);
        }
    }

    #[test]
    fn raw_window_without_a_rect_or_desktop_still_deserialises() {
        // `rect` arrived with fullscreen detection and `onCurrentDesktop`
        // with virtual-desktop support; documents and mocks written before
        // them leave them out.
        let json = r#"{"id":7,"title":"T","className":"C","processId":1,"exePath":null,
            "appUserModelId":null,"isVisible":true,"isMinimized":false,"isCloaked":false,
            "monitor":null,"iconKey":null}"#;
        let raw: RawWindow = serde_json::from_str(json).unwrap();
        assert_eq!(raw.rect, None);
        assert_eq!(raw.on_current_desktop, None);
        let with_rect: RawWindow = serde_json::from_str(&json.replacen(
            r#""monitor":null"#,
            r#""monitor":null,"rect":{"x":1,"y":2,"width":3,"height":4}"#,
            1,
        ))
        .unwrap();
        assert_eq!(with_rect.rect, Some(Rect::new(1, 2, 3, 4)));
        for (value, expected) in [("true", Some(true)), ("false", Some(false)), ("null", None)] {
            let with_desktop: RawWindow = serde_json::from_str(&json.replacen(
                r#""monitor":null"#,
                &format!(r#""monitor":null,"onCurrentDesktop":{value}"#),
                1,
            ))
            .unwrap();
            assert_eq!(with_desktop.on_current_desktop, expected, "{value}");
        }
    }

    #[test]
    fn app_info_and_app_folder_serialise_in_camel_case() {
        let info = AppInfo {
            version: "0.1.0".to_owned(),
            settings_path: r"C:\Users\me\AppData\Roaming\dev.davidcristian.stem\settings.json"
                .to_owned(),
            log_path: String::new(),
            platform: "windows".to_owned(),
        };
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "version": "0.1.0",
                "settingsPath": r"C:\Users\me\AppData\Roaming\dev.davidcristian.stem\settings.json",
                "logPath": "",
                "platform": "windows",
            })
        );
        assert_eq!(serde_json::from_value::<AppInfo>(json).unwrap(), info);
        assert_eq!(
            serde_json::to_string(&AppFolder::Logs).unwrap(),
            r#""logs""#
        );
        assert_eq!(
            serde_json::from_str::<AppFolder>(r#""settings""#).unwrap(),
            AppFolder::Settings
        );
    }

    #[test]
    fn rect_contains_is_half_open() {
        let r = Rect::new(10, 10, 5, 5);
        assert!(r.contains(10, 10));
        assert!(r.contains(14, 14));
        assert!(!r.contains(15, 14));
        assert!(!r.contains(9, 10));
    }
}
