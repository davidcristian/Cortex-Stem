//! Top-level windows, as the platform reports them and as a dock item
//! lists them.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::{IconKey, MonitorId, Rect, WindowId};

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
