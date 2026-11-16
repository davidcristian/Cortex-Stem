//! The user's stylesheet (`docs/ipc/themes.md`, `docs/THEMING.md`):
//! `dock.css` in the settings file's folder
//! (`%APPDATA%\dev.davidcristian.stem\dock.css` for an installed Stem).
//!
//! Read in `setup`, before the dock windows exist, and again on
//! `reload_user_css`; never on its own (no file watching). The text the
//! page gets ([`UserCss::css`]) is the file's, a leading byte order mark
//! dropped, or `None` when the file does not exist (an `info` line: the
//! normal case), cannot be read, is not UTF-8 or is larger than
//! [`MAX_BYTES`] (a `warn` line each, saying which); [`UserCss::problem`]
//! says which of the four it was, for the settings page. The read is bounded:
//! the file's size is checked before and while reading, so a huge or
//! growing file costs at most [`MAX_BYTES`] + 1 bytes.
//!
//! `open_user_css` writes [`TEMPLATE`] when there is no file yet (never over
//! an existing one) and hands the path to `Platform::launch`, which opens it
//! with the default app for `.css`, or with Notepad when that fails
//! ([`open_file`]): a stock Windows may have no app for `.css`, and the
//! shell, asked without UI, then refuses instead of offering its picker.
//!
//! Reloads are serialised ([`UserCssState::reload_and`]): the read, the
//! stored result and what the docks are sent always belong to the same
//! read, and the last Reload pressed is the one that stays.
//!
//! A run without a settings file (the mock platform, a profile that could
//! not be used) has no `dock.css` either: the path is a note in parentheses,
//! nothing is read and `open_user_css` rejects.
//!
//! [`TEMPLATE`]: template::TEMPLATE

mod open;
mod read;
mod template;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use parking_lot::Mutex;
use stem_core::{UserCss, UserCssProblem};
use tauri::{AppHandle, Emitter, EventTarget};

pub use open::{notepad_for, open_file};
use read::{read_file, report};
use template::write_template_if_missing;

/// The stylesheet's file name, next to `settings.json`.
pub const FILE_NAME: &str = "dock.css";

/// The largest file Stem reads (64 KiB); anything larger is ignored whole.
pub const MAX_BYTES: u64 = 64 * 1024;

/// `UserCss::path` when nothing is saved this run.
const NO_FILE_DEMO: &str = "(demo: no dock.css)";
const NO_FILE: &str = "(no dock.css this session)";

/// `dock.css` next to the settings file, or `None` when there is no
/// settings file this run or it has no folder.
pub fn path_for(settings_file: Option<&Path>) -> Option<PathBuf> {
    settings_file
        .and_then(Path::parent)
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(|dir| dir.join(FILE_NAME))
}

/// The app's view of `dock.css`, in Tauri's managed state: where it is and
/// what the last read gave (`get_user_css`).
#[derive(Debug)]
pub struct UserCssState {
    /// The file; `None` when nothing is saved this run.
    file: Option<PathBuf>,
    current: Mutex<UserCss>,
    /// Held for a whole reload (read, store, publish), so two at once
    /// cannot finish in the wrong order. Separate from `current`, so
    /// `get_user_css` never waits for the disk.
    reloading: Mutex<()>,
}

impl UserCssState {
    /// The state for a run whose settings file is `settings_file` (`None`
    /// for the in-memory store; `demo` says whether that is the mock
    /// platform). Reads nothing yet: [`UserCssState::reload`] does; until
    /// then there is no CSS and the problem is `notFound` (`setup` reads
    /// before any page exists to ask).
    pub fn new(settings_file: Option<&Path>, demo: bool) -> Self {
        let file = path_for(settings_file);
        let path = match &file {
            Some(file) => file.display().to_string(),
            None if demo => NO_FILE_DEMO.to_owned(),
            None => NO_FILE.to_owned(),
        };
        Self {
            file,
            current: Mutex::new(UserCss {
                path,
                css: None,
                problem: Some(UserCssProblem::NotFound),
            }),
            reloading: Mutex::new(()),
        }
    }

    /// What the last read gave (none yet: no CSS).
    pub fn current(&self) -> UserCss {
        self.current.lock().clone()
    }

    /// Read the file again, log what came of it and keep the result.
    /// Blocking (disk): `setup` calls it once, before any dock window
    /// exists, so there is no one to tell.
    pub fn reload(&self) -> UserCss {
        self.reload_and(|_| {})
    }

    /// [`UserCssState::reload`], then `publish` the result (the command
    /// sends it to the docks), all under one lock: a second reload waits
    /// until the first has published, so the stored result and the last
    /// thing the docks were sent are always the same, latest read. Without
    /// it, two Reloads around a save could store and send the new text and
    /// then the old. Blocking (disk): the command runs it on the blocking
    /// pool.
    pub fn reload_and(&self, publish: impl FnOnce(&UserCss)) -> UserCss {
        let _reloading = self.reloading.lock();
        // No settings file this run: no `dock.css` either.
        let result = self
            .file
            .as_deref()
            .map_or(Err(UserCssProblem::NotFound), |file| {
                report(file, read_file(file))
            });
        let read = {
            let mut current = self.current.lock();
            (current.css, current.problem) = match result {
                Ok(css) => (Some(css), None),
                Err(problem) => (None, Some(problem)),
            };
            current.clone()
        };
        publish(&read);
        read
    }

    /// The file `open_user_css` opens, written from [`TEMPLATE`] first if
    /// it does not exist yet. `Err` is the command's error string. Blocking
    /// (disk).
    ///
    /// [`TEMPLATE`]: template::TEMPLATE
    pub fn file_to_open(&self) -> Result<PathBuf, String> {
        let file = self
            .file
            .as_deref()
            .ok_or_else(|| "settings are not saved to a file in this run".to_owned())?;
        match write_template_if_missing(file) {
            Ok(true) => log::info!("dock.css: wrote the template to {}", file.display()),
            Ok(false) => {}
            Err(e) => {
                return Err(format!(
                    "could not create {} from the template: {e}",
                    file.display()
                ));
            }
        }
        Ok(file.to_path_buf())
    }
}

/// Whether `target` names a dock window (`dock`, `dock-<monitor>`).
fn is_dock_target(target: &EventTarget) -> bool {
    match target {
        EventTarget::Window { label }
        | EventTarget::Webview { label }
        | EventTarget::WebviewWindow { label }
        | EventTarget::AnyLabel { label } => label.starts_with(crate::windows::DOCK_LABEL),
        _ => false,
    }
}

/// Send `css` to every dock window as `dock://user-css`. The pages listen
/// on their own window (`TauriBackend.on`, target `WebviewWindow`), so the
/// filter applies: the settings page would not get it even if it listened
/// (a listener without a target receives every event whatever the filter);
/// it takes `reload_user_css`'s answer instead.
pub fn emit_to_docks(app: &AppHandle, css: &UserCss) {
    if let Err(e) = app.emit_filter(crate::events::USER_CSS, css, is_dock_target) {
        log::warn!("failed to emit {}: {e}", crate::events::USER_CSS);
    }
}
