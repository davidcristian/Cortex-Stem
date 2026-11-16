//! Re-identification of stale `exe:` pins of packaged apps.
//!
//! Settings written by older builds can pin a packaged app (Windows Terminal,
//! say) by its executable under `\WindowsApps\`, with an `exe:` id and an
//! `Exe` launch target. That app's windows carry an `aumid:` id,
//! so the pin and the windows never merge, and the exe yields neither a name
//! nor an icon. Once per start the service asks the platform about each such
//! pin (`Platform::identify_target`, which knows packages) and
//! rewrites the ones that turn out to be a packaged application, in place:
//! the `aumid:` id, a `StoreApp` launch target, the user's label kept. A pin
//! whose new id is already pinned is dropped instead, so no duplicate
//! appears. What cannot be identified, or fails, stays as it is.
//!
//! The same goes for an `exe:` pin of an inbox executable under
//! `<drive>:\Windows\` that Windows redirects to a packaged app (a
//! `C:\Windows\notepad.exe` dropped on the dock before the platform knew
//! app execution aliases): the platform names the package app, and the pin
//! becomes that app's.
//!
//! This module holds the decisions (which pins qualify, what an answer
//! means, how the pinned list changes); `Inner::reidentify_stale_pins` in
//! `resolver.rs` does the platform calls and the one save.

use std::fmt;

use stem_core::settings::PinnedApp;
use stem_core::{AppId, AppIdentity, LaunchTarget};
use stem_platform::rules::identity::{packaged_exe, windows_dir_exe};

/// Prefix of the ids this module rewrites (`AppId::for_exe`).
const EXE_ID_PREFIX: &str = "exe:";
/// Prefix of the ids a packaged app's windows carry
/// (`AppId::for_app_user_model_id`).
const AUMID_ID_PREFIX: &str = "aumid:";

/// Whether `pin` may be a stale pin of a packaged app: pinned under an
/// `exe:` id and launched by an `Exe` target under `\WindowsApps\<package>\`,
/// or under `<drive>:\Windows\` (an inbox exe an app execution alias may
/// redirect), without arguments (a `StoreApp` target cannot carry them, and
/// the re-identification must not lose anything the user set).
pub(crate) fn is_candidate(pin: &PinnedApp) -> bool {
    if !pin.id.as_str().starts_with(EXE_ID_PREFIX) {
        return false;
    }
    match &pin.launch {
        LaunchTarget::Exe { path, args, .. } => {
            let path = path.to_string_lossy();
            args.is_empty() && (packaged_exe(&path).is_some() || windows_dir_exe(&path))
        }
        LaunchTarget::Shortcut { .. }
        | LaunchTarget::StoreApp { .. }
        | LaunchTarget::Uri { .. } => false,
    }
}

/// What the platform's answer for a candidate means: `Some` when it names a
/// packaged application under a different id than the pin's, `None` when
/// the pin is to be left alone (the platform kept the `exe:` identity, or
/// answered with the id the pin already has).
pub(crate) fn reidentified(pin: &PinnedApp, identity: &AppIdentity) -> Option<Reidentified> {
    let aumid = identity.id.as_str().strip_prefix(AUMID_ID_PREFIX)?.trim();
    if aumid.is_empty() || identity.id == pin.id {
        return None;
    }
    Some(Reidentified {
        old: pin.id.clone(),
        aumid: aumid.to_owned(),
    })
}

/// A stale pin and the packaged application it turned out to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Reidentified {
    /// The id the pin was found under.
    pub old: AppId,
    /// The application's AppUserModelID.
    pub aumid: String,
}

impl Reidentified {
    /// The id the pin is rewritten to.
    pub fn new_id(&self) -> AppId {
        AppId::for_app_user_model_id(&self.aumid)
    }
}

/// What happened to one pin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PinRewrite {
    /// The pin carries the new id and a `StoreApp` target, in its old place.
    Replaced { old: AppId, new: AppId },
    /// The new id was already pinned; the stale pin was removed.
    Dropped { old: AppId, new: AppId },
}

impl fmt::Display for PinRewrite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Replaced { old, new } => {
                write!(f, "re-identified pin {} as {}", old.as_str(), new.as_str())
            }
            Self::Dropped { old, new } => write!(
                f,
                "dropped pin {}: it is {}, which is already pinned",
                old.as_str(),
                new.as_str()
            ),
        }
    }
}

/// Apply `found` to `pinned`, in order, and say what changed. A pin is
/// rewritten in place (same position, label kept) to the new id with a
/// `StoreApp` target; when the new id is already pinned (by the user, or by
/// an earlier entry of `found`) the stale pin is removed instead. An entry
/// whose pin is no longer there (unpinned meanwhile) changes nothing.
pub(crate) fn rewrite_pins(pinned: &mut Vec<PinnedApp>, found: &[Reidentified]) -> Vec<PinRewrite> {
    let mut rewrites = Vec::new();
    for entry in found {
        let Some(at) = pinned.iter().position(|p| p.id == entry.old) else {
            continue;
        };
        let new = entry.new_id();
        if pinned.iter().any(|p| p.id == new) {
            pinned.remove(at);
            rewrites.push(PinRewrite::Dropped {
                old: entry.old.clone(),
                new,
            });
            continue;
        }
        let pin = &mut pinned[at];
        pin.id = new.clone();
        pin.launch = LaunchTarget::StoreApp {
            app_user_model_id: entry.aumid.clone(),
        };
        rewrites.push(PinRewrite::Replaced {
            old: entry.old.clone(),
            new,
        });
    }
    rewrites
}

#[cfg(test)]
mod tests;
