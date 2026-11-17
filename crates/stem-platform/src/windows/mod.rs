//! Windows implementation of [`Platform`](crate::Platform).
//!
//! This directory is the **only** place in the workspace where `unsafe` and
//! the `windows` crate are allowed. Read
//! `docs/development/windows-rules.md` before editing.
//!
//! Modules:
//! - `util`        shared helpers (handles, strings, errors, COM)
//! - `enumerate`   which windows qualify, and their `RawWindow` facts
//! - `events`      WinEvent hook + message loop thread
//! - `actions`     activate / minimise / restore / close; focus a dock
//! - `monitors`    monitors, work areas, DPI
//! - `icons`       HICON/exe/UWP icons -> PNG, with cache
//! - `identity`    display names, AUMIDs, .lnk resolution
//! - `launch`      ShellExecute & friends
//! - `theme`       accent colour, light/dark, contrast
//! - `recycle_bin` SHQueryRecycleBin, open, empty; recycle files
//! - `appbar`      SHAppBarMessage space reservation (a thread)
//! - `shell`       Start menu, Task View, Show Desktop; Settings pages
//! - `power`       power source, battery, battery saver
//! - `locale`      the regional format the clock follows
//! - `volume`      the default output device's volume (a thread)
//! - `dock_window` extended styles + subclass of our own windows
//! - `hit_test`    cursor-tracking thread for the hit region
//! - `taskbar`     the stock taskbar's auto-hide state
//! - `fullscreen`  does the foreground window own the screen
//! - `dialog`      native message boxes (the crash report)
//! - `virtual_desktop` which virtual desktop a window is on
//! - `backdrop`    native backdrop window under a dock (prototype)
//! - `platform`    [`WindowsPlatform`], the `Platform` implementation

pub mod actions;
pub mod appbar;
pub mod backdrop;
pub mod dialog;
pub mod dock_window;
pub mod enumerate;
pub mod events;
pub mod fullscreen;
pub mod hit_test;
pub mod icons;
pub mod identity;
pub mod launch;
pub mod locale;
pub mod monitors;
mod platform;
pub mod power;
#[cfg(feature = "probes")]
pub mod probe;
pub mod recycle_bin;
pub mod shell;
pub mod taskbar;
pub mod theme;
pub mod util;
pub mod virtual_desktop;
pub mod volume;

pub use platform::WindowsPlatform;
