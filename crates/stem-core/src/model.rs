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

mod actions;
mod app;
mod dock;
mod ids;
mod install;
mod system;
#[cfg(test)]
mod tests;
mod window;

// TypeScript bindings are exported to `src/lib/ipc/generated/` (relative to
// the repository root) by `cargo test -p stem-core --features ts`.

pub use actions::{ScrollDirection, ShellAction, WindowAction};
pub use app::{AppIdentity, LaunchTarget};
pub use dock::{
    ClockFormat, DockFacts, DockItem, DockItemKind, DockState, PowerStatus, VolumeStatus,
};
pub use ids::{AppId, IconKey, MonitorId, WindowId, normalize_path};
pub use install::{AppFolder, AppInfo, UserCss, UserCssProblem};
pub use system::{MonitorInfo, Rect, RecycleBinState, ThemeInfo};
pub use window::{RawWindow, WindowInfo};
