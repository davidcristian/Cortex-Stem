//! How a dock window the app layer created behaves at the OS level: its
//! native handle, its styles and where it accepts mouse input.

use stem_core::Rect;

/// Native handle of a window the *app layer* created (a Tauri dock window).
/// On Windows this is the `HWND` value. Opaque everywhere else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NativeWindowHandle(pub isize);

/// How a dock window should behave at the OS level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DockWindowOptions {
    /// Clicking the dock must not take keyboard focus from the active app.
    pub no_activate: bool,
    /// Hide from Alt+Tab and the stock taskbar.
    pub tool_window: bool,
    /// Ask the compositor not to round or shadow the window; we draw our own.
    pub no_system_decorations: bool,
}

impl Default for DockWindowOptions {
    fn default() -> Self {
        Self {
            no_activate: true,
            tool_window: true,
            no_system_decorations: true,
        }
    }
}

/// Where a dock window accepts mouse input. Both rectangles are in
/// **physical pixels, relative to the window's client area**.
///
/// The window becomes clickable when the cursor enters `panel` (the visible
/// dock panel) and click-through again only once the cursor has left
/// `envelope` (the panel plus everything magnification can lift above it),
/// so an icon that grew past the panel's edge stays clickable. `envelope`
/// contains `panel`; the implementation may treat a smaller envelope as
/// equal to the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HitRegion {
    pub panel: Rect,
    pub envelope: Rect,
}
