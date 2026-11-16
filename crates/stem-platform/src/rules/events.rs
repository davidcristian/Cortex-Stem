//! WinEvent / shell-hook translation table.
//!
//! `windows::events` installs hooks for exactly the ranges in
//! [`HOOK_RANGES`], filters each callback with [`win_event_intent`], and acts
//! on the resulting [`Intent`]. The shell hook window's messages go through
//! [`shell_hook_intent`]. Title-change storms are coalesced per window with
//! [`TitleDebouncer`]. Everything here is plain data so it is unit-tested on
//! every host.
//!
//! Translation table (the callback additionally requires
//! `idObject == OBJID_WINDOW`, `idChild == CHILDID_SELF` and a top-level
//! window; see [`win_event_intent`]):
//!
//! | Source     | Event                                                     | Intent         | Emitted as |
//! |------------|-----------------------------------------------------------|----------------|------------|
//! | WinEvent   | `EVENT_OBJECT_CREATE`, `EVENT_OBJECT_SHOW`                | `Added`        | `WindowAdded` when `query_window_checked` says eligible (`WindowUpdated` if already known); re-checks when it is only not ready yet |
//! | WinEvent   | `EVENT_OBJECT_DESTROY`, `EVENT_OBJECT_HIDE`               | `Removed`      | `WindowRemoved` when the window was known; a destroy also drops its pending re-checks |
//! | WinEvent   | `EVENT_OBJECT_NAMECHANGE`                                 | `TitleChanged` | `WindowUpdated` after the [`TitleDebouncer`] window (≤ 50 ms) |
//! | WinEvent   | `EVENT_SYSTEM_MINIMIZESTART`, `EVENT_SYSTEM_MINIMIZEEND`, `EVENT_OBJECT_CLOAKED`, `EVENT_OBJECT_UNCLOAKED` | `Updated` | `WindowUpdated` (or `WindowRemoved` when no longer eligible); the two cloak events are also counted by the [`DesktopSwitchDetector`] |
//! | WinEvent   | `EVENT_SYSTEM_FOREGROUND`                                 | `Foreground`   | `ForegroundChanged` |
//! | Shell hook | `HSHELL_WINDOWCREATED`                                    | `Added`        | as for `EVENT_OBJECT_SHOW` |
//! | Shell hook | `HSHELL_WINDOWACTIVATED`, `HSHELL_RUDEAPPACTIVATED`       | `Foreground`   | `ForegroundChanged` |
//! | Shell hook | `HSHELL_FLASH`                                            | `Attention`    | `WindowAttention { flashing: true }` |
//! | Shell hook | `HSHELL_WINDOWDESTROYED`, `HSHELL_REDRAW`, every other code | `Ignore`     | — (a WinEvent already covers each; see `windows::events`, *Sources*); the message is still named by [`shell_hook_name`] in one trace line |
//! | any        | anything else                                             | `Ignore`       | — |
//!
//! `Added` from either source is a *show-type* event: when it finds the
//! window rejected only because it is not visible or cloaked yet, the window
//! is queried again later (`rules::recheck`). `HSHELL_WINDOWCREATED`
//! (documented for a new top-level, unowned window) reaches us on a path of
//! its own, so it is one more chance to judge a window whose create/show
//! WinEvents were evaluated too early.
//!
//! The constants mirror `winuser.h`; `windows/events.rs` asserts at compile
//! time that they equal the SDK values from the `windows` crate.

mod codes;
mod desktop;
mod intent;
mod title;

pub use codes::*;
pub use desktop::{DesktopQueryFailures, DesktopSwitchDetector};
pub use intent::{
    Intent, TopLevel, event_intent, event_name, location_change_tracked, shell_hook_intent,
    shell_hook_name, win_event_intent,
};
pub use title::TitleDebouncer;

#[cfg(test)]
mod tests;
