//! What the native backdrop window under a dock window shows (a prototype).

use stem_core::Rect;

/// The system material of a backdrop window (a prototype,
/// `docs/research/backdrop/backdrop-window.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BackdropMaterial {
    /// Desktop Acrylic: blurs whatever is behind the window.
    Acrylic,
    /// Mica: tinted by the desktop wallpaper.
    Mica,
}

/// What the native backdrop window under a dock window shows (a
/// prototype). Platform-local on purpose: the prototype changes neither the
/// IPC contract nor `stem-core`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DockBackdrop {
    /// The dock's panel at rest, in **physical pixels of the virtual
    /// desktop**, rounded inwards: the material must never stick out of the
    /// panel.
    pub rect: Rect,
    /// The panel's corner radius in physical pixels.
    pub corner_radius: u32,
    pub material: BackdropMaterial,
    /// The dark variant of the material (the dock's effective theme).
    pub dark: bool,
}
