//! What a primary click on a dock item does.

use super::DockEngine;
use crate::model::{AppId, ShellAction, WindowId};

/// The effect of a primary click on a dock item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemAction {
    Launch(AppId),
    ActivateWindow(WindowId),
    MinimizeWindow(WindowId),
    Shell(ShellAction),
    /// Nothing sensible to do (unknown item, app without launch target, ...).
    None,
}

impl DockEngine {
    /// Decide what a primary click on `item_key` in the dock window `dock`
    /// should do.
    ///
    /// - fixed items map to their shell action (the volume and battery
    ///   items to quick settings, the clock to the notification center and
    ///   calendar);
    /// - an app with none of the windows that dock lists
    ///   ([`Self::windows_on_dock`]) is launched (if it can be): a pinned app
    ///   whose windows are all on other monitors starts a new one, as that
    ///   dock shows no window for it;
    /// - an app whose window is active is minimised (if configured), otherwise
    ///   activated;
    /// - an app with several windows cycles: the window after the active one,
    ///   or the first one. Only the windows that dock lists take part;
    /// - a hidden, unpinned app has no item, so a click on its key
    ///   (a page that has not seen the new state yet) does nothing.
    pub fn primary_action(&self, item_key: &str, dock: &str) -> ItemAction {
        match item_key {
            "fixed:launcher" => return ItemAction::Shell(ShellAction::OpenStartMenu),
            "fixed:task-view" => return ItemAction::Shell(ShellAction::OpenTaskView),
            "fixed:show-desktop" => return ItemAction::Shell(ShellAction::ToggleShowDesktop),
            "fixed:quick-settings" => return ItemAction::Shell(ShellAction::OpenQuickSettings),
            "fixed:notification-center" | "fixed:clock" => {
                return ItemAction::Shell(ShellAction::OpenNotificationCenter);
            }
            "fixed:volume" | "fixed:battery" => {
                return ItemAction::Shell(ShellAction::OpenQuickSettings);
            }
            "fixed:recycle-bin" => return ItemAction::Shell(ShellAction::OpenRecycleBin),
            _ => {}
        }
        let Some(app_id) = item_key.strip_prefix("app:").map(|s| AppId(s.to_owned())) else {
            return ItemAction::None;
        };
        if self.settings.is_hidden(&app_id) && !self.settings.is_pinned(&app_id) {
            return ItemAction::None;
        }
        let windows = self.windows_on_dock(&app_id, dock);
        if windows.is_empty() {
            let launchable = self.settings.is_pinned(&app_id)
                || self
                    .identities
                    .get(&app_id)
                    .is_some_and(|i| i.launch.is_some());
            return if launchable {
                ItemAction::Launch(app_id)
            } else {
                ItemAction::None
            };
        }
        let active_pos = self
            .foreground
            .and_then(|fg| windows.iter().position(|w| *w == fg));
        match active_pos {
            Some(pos) if windows.len() == 1 => {
                if self.settings.behavior.click_active_minimizes {
                    ItemAction::MinimizeWindow(windows[pos])
                } else {
                    ItemAction::ActivateWindow(windows[pos])
                }
            }
            Some(pos) => ItemAction::ActivateWindow(windows[(pos + 1) % windows.len()]),
            None => ItemAction::ActivateWindow(windows[0]),
        }
    }
}
